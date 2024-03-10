use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use crate::render::Renderer;
use crate::window::{WMHookID, Window, WindowID};
use crate::Basalt;

/// Automatically creates `Renderer` for each window.
pub struct AutoMultiWindowRenderer {
    basalt: Arc<Basalt>,
    auto_exit: bool,
    hook_ids: Vec<WMHookID>,
    join_handles: HashMap<WindowID, JoinHandle<Result<(), String>>>,
    renderer_method: Option<Box<dyn FnMut(Arc<Window>) -> Renderer + Send + 'static>>,
}

enum AMWREvent {
    Open(Arc<Window>),
    Close(WindowID),
}

impl AutoMultiWindowRenderer {
    /// Create a new `AutoMultiWindowRenderer`.
    ///
    /// ***Note:** There should only ever be one instance of this struct. Having multiple will
    /// result in panics.*
    pub fn new(basalt: Arc<Basalt>) -> Self {
        Self {
            basalt,
            auto_exit: false,
            hook_ids: Vec::new(),
            join_handles: HashMap::new(),
            renderer_method: None,
        }
    }

    /// This methods allows the user to provide a `Renderer` given a window.
    ///
    /// ***Note:** This method is not required to be called. It will default to creating an
    /// interface only renderer.*
    pub fn with_renderer_method<F: FnMut(Arc<Window>) -> Renderer + Send + 'static>(
        mut self,
        method: F,
    ) -> Self {
        self.renderer_method = Some(Box::new(method));
        self
    }

    /// Exit the renderer when all windows have been closed.
    pub fn exit_when_all_windows_closed(mut self, value: bool) -> Self {
        self.auto_exit = value;
        self
    }

    /// Start running the the renderer.
    pub fn run(mut self) -> Result<(), String> {
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
                    let window_id = window.id();

                    if !self.join_handles.contains_key(&window_id) {
                        let renderer = match self.renderer_method.as_mut() {
                            Some(method) => method(window),
                            None => Renderer::new(window).unwrap().with_interface_only(),
                        };

                        self.join_handles
                            .insert(window_id, thread::spawn(move || renderer.run()));
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

                    if self.auto_exit && self.join_handles.is_empty() {
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
