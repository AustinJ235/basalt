mod monitor;
mod window;

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use parking_lot::{Condvar, Mutex};
pub use window::Window;
use winit::dpi::PhysicalSize;
use winit::event::Event as WinitEvent;
use winit::event_loop::{EventLoopBuilder, EventLoopProxy};
use winit::window::WindowBuilder;

use crate::Basalt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowID(u64);

impl WindowID {
    pub(crate) fn invalid() -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowOptions {
    pub width: u32,
    pub height: u32,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    Opened,
    Closed,
    Resized { width: u32, height: u32 },
    ScaleChanged,
    RedrawRequested,
    EnabledFullscreen,
    DisabledFullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowType {
    Android,
    Macos,
    Ios,
    Wayland,
    Windows,
    Xcb,
    Xlib,
}

#[derive(Debug)]
enum WMEvent {
    AssociateBasalt(Arc<Basalt>),
    WindowEvent {
        id: WindowID,
        event: WindowEvent,
    },
    CreateWindow {
        options: WindowOptions,
        cond: Arc<Condvar>,
        result: Arc<Mutex<Option<Result<Arc<Window>, String>>>>,
    },
}

/// Manages windows and their associated events.
pub struct WindowManager {
    event_proxy: EventLoopProxy<WMEvent>,
}

impl WindowManager {
    /// Creates a window given the options.
    pub fn create(&self, options: WindowOptions) -> Result<Arc<Window>, String> {
        let result = Arc::new(Mutex::new(None));
        let cond = Arc::new(Condvar::new());

        self.event_proxy
            .send_event(WMEvent::CreateWindow {
                options,
                result: result.clone(),
                cond: cond.clone(),
            })
            .map_err(|_| String::from("Failed to create window: event loop is closed."))?;

        let mut result_guard = result.lock();

        while result_guard.is_none() {
            cond.wait(&mut result_guard);
        }

        result_guard.take().unwrap()
    }

    pub(crate) fn associate_basalt(&self, basalt: Arc<Basalt>) {
        self.send_event(WMEvent::AssociateBasalt(basalt));
    }

    pub(crate) fn send_event(&self, event: WMEvent) {
        self.event_proxy.send_event(event).unwrap();
    }

    pub(crate) fn new<F: FnMut(Arc<Self>) + Send + 'static>(mut exec: F) {
        let event_loop = EventLoopBuilder::<WMEvent>::with_user_event()
            .build()
            .unwrap();
        let event_proxy = event_loop.create_proxy();

        let wm = Arc::new(Self {
            event_proxy,
        });

        let wm_closure = wm.clone();
        thread::spawn(move || exec(wm_closure));

        let mut basalt_op = None;
        let mut next_window_id = 1;
        let mut winit_to_bst_id = HashMap::new();
        let mut windows = HashMap::new();

        event_loop
            .run(move |event, elwt| {
                match event {
                    WinitEvent::UserEvent(wm_event) => {
                        match wm_event {
                            WMEvent::AssociateBasalt(basalt) => {
                                basalt_op = Some(basalt);
                            },
                            WMEvent::WindowEvent {
                                id,
                                event,
                            } => {
                                todo!()
                            },
                            WMEvent::CreateWindow {
                                options,
                                cond,
                                result,
                            } => {
                                if basalt_op.is_none() {
                                    *result.lock() = Some(Err(String::from(
                                        "Failed to create window: basalt is not associated.",
                                    )));
                                    cond.notify_one();
                                    return;
                                }

                                let basalt = basalt_op.as_ref().unwrap();

                                let window_builder = WindowBuilder::new()
                                    .with_inner_size(PhysicalSize::new(
                                        options.width,
                                        options.height,
                                    ))
                                    .with_title(options.title);

                                let winit_window = match window_builder.build(&elwt) {
                                    Ok(ok) => Arc::new(ok),
                                    Err(e) => {
                                        *result.lock() =
                                            Some(Err(format!("Failed to create window: {}", e)));
                                        cond.notify_one();
                                        return;
                                    },
                                };

                                let winit_window_id = winit_window.id();
                                let window_id = WindowID(next_window_id);

                                let window = match Window::new(
                                    basalt.clone(),
                                    wm.clone(),
                                    window_id,
                                    winit_window,
                                ) {
                                    Ok(ok) => ok,
                                    Err(e) => {
                                        *result.lock() = Some(Err(e));
                                        cond.notify_one();
                                        return;
                                    },
                                };

                                next_window_id += 1;
                                winit_to_bst_id.insert(winit_window_id, window_id);
                                windows.insert(window_id, window.clone());
                                *result.lock() = Some(Ok(window));
                                cond.notify_one();
                            },
                        }
                    },
                    _ => (),
                }
            })
            .unwrap();
    }
}
