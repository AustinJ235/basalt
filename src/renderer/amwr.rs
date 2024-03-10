use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use crate::renderer::Renderer;
use crate::window::{WMHookID, Window, WindowID};
use crate::Basalt;

pub struct AutoMultiWindowRenderer {
    basalt: Arc<Basalt>,
    hook_ids: Vec<WMHookID>,
    join_handles: HashMap<WindowID, JoinHandle<Result<(), String>>>,
}

enum AMWREvent {
    Open(Arc<Window>),
    Close(WindowID),
}

impl AutoMultiWindowRenderer {
    pub fn new(basalt: Arc<Basalt>) -> Self {
        Self {
            basalt,
            hook_ids: Vec::new(),
            join_handles: HashMap::new(),
        }
    }

    pub fn run(mut self, exit_when_all_windows_closed: bool) -> Result<(), String> {
        let (event_send, event_recv) = flume::unbounded();

        for window in self.basalt.window_manager_ref().windows() {
            event_send.send(AMWREvent::Open(window)).unwrap();
        }

        let on_open_event_send = event_send.clone();
        let on_close_event_send = event_send;

        self.hook_ids
            .push(self.basalt.window_manager_ref().on_open(move |window| {
                on_open_event_send.send(AMWREvent::Open(window)).unwrap();
            }));

        self.hook_ids
            .push(self.basalt.window_manager_ref().on_close(move |window_id| {
                on_close_event_send
                    .send(AMWREvent::Close(window_id))
                    .unwrap();
            }));

        while let Ok(event) = event_recv.recv() {
            match event {
                AMWREvent::Open(window) => {
                    if !self.join_handles.contains_key(&window.id()) {
                        self.join_handles.insert(
                            window.id(),
                            thread::spawn(move || Renderer::new(window)?.run_interface_only()),
                        );
                    }
                },
                AMWREvent::Close(window_id) => {
                    if let Some(join_handle) = self.join_handles.remove(&window_id) {
                        match join_handle.join() {
                            Ok(Ok(_)) => (),
                            Ok(Err(e)) => {
                                println!(
                                    "[Basalt][AMWR]: {:?} had its renderer exit with an error: {}",
                                    window_id, e
                                );
                            },
                            Err(_) => {
                                println!("[Basalt][AMWR]: {:?} had its renderer panic!", window_id);
                            },
                        }
                    }

                    if exit_when_all_windows_closed && self.join_handles.is_empty() {
                        break;
                    }
                },
            }
        }

        for hook_id in self.hook_ids.drain(..) {
            self.basalt.window_manager_ref().remove_hook(hook_id);
        }

        Ok(())
    }
}
