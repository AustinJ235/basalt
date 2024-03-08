use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use flume::TryRecvError;

use crate::renderer::Renderer;
use crate::window::{WMHookID, Window, WindowID};
use crate::Basalt;

pub struct AutoMultiWindowRenderer {
    basalt: Arc<Basalt>,
    wm_hook_ids: Vec<WMHookID>,
    window_states: HashMap<WindowID, WindowState>,
}

struct WindowState {
    window: Arc<Window>,
    worker_handle: Option<JoinHandle<Result<(), String>>>,
}

enum AMWREvent {
    Open(Arc<Window>),
    Close(WindowID),
}

impl AutoMultiWindowRenderer {
    pub fn new(basalt: Arc<Basalt>) -> Self {
        Self {
            basalt,
            wm_hook_ids: Vec::new(),
            window_states: HashMap::new(),
        }
    }

    fn add_window(&mut self, window: Arc<Window>) {
        let window_cp = window.clone();

        let join_handle = thread::spawn(move || Renderer::new(window)?.run_interface_only());

        self.window_states.insert(
            window_cp.id(),
            WindowState {
                window: window_cp,
                worker_handle: Some(join_handle),
            },
        );
    }

    fn remove_window(&mut self, window_id: WindowID) {
        self.window_states.remove(&window_id);
    }

    pub fn run(mut self, exit_when_all_windows_closed: bool) -> Result<(), String> {
        for window in self.basalt.window_manager_ref().windows() {
            self.add_window(window);
        }

        let (event_send, event_recv) = flume::unbounded();
        let on_open_event_send = event_send.clone();
        let on_close_event_send = event_send;

        self.wm_hook_ids
            .push(self.basalt.window_manager_ref().on_open(move |window| {
                on_open_event_send.send(AMWREvent::Open(window)).unwrap();
            }));

        self.wm_hook_ids
            .push(self.basalt.window_manager_ref().on_close(move |window_id| {
                on_close_event_send
                    .send(AMWREvent::Close(window_id))
                    .unwrap();
            }));

        let mut event_recv_block = self.window_states.is_empty();

        'main_loop: loop {
            if event_recv_block && exit_when_all_windows_closed {
                break;
            }

            loop {
                let event = match event_recv_block {
                    true => {
                        match event_recv.recv() {
                            Ok(ok) => ok,
                            Err(_) => break 'main_loop,
                        }
                    },
                    false => {
                        match event_recv.try_recv() {
                            Ok(ok) => ok,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => break 'main_loop,
                        }
                    },
                };

                match event {
                    AMWREvent::Open(window) => {
                        self.add_window(window);
                    },
                    AMWREvent::Close(_window_id) => (),
                }
            }

            self.window_states.retain(|window_id, window_state| {
                if window_state.worker_handle.as_ref().unwrap().is_finished() {
                    match window_state.worker_handle.take().unwrap().join() {
                        Ok(worker_result) => {
                            if let Err(e) = worker_result {
                                println!(
                                    "[Basalt][AMWR]: Window with ID of {:?} had its renderer exit \
                                     with an error of: {}",
                                    window_id, e
                                );
                            }
                        },
                        Err(_) => {
                            println!(
                                "[Basalt][AMWR]: Window with ID of {:?} had its renderer panic!",
                                window_id
                            );
                        },
                    }

                    return false;
                }

                // TODO: Trigger render

                true
            });

            event_recv_block = self.window_states.is_empty();
        }

        for wm_hook_id in self.wm_hook_ids.drain(..) {
            self.basalt.window_manager_ref().remove_hook(wm_hook_id);
        }

        Ok(())
    }
}
