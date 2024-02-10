mod monitor;
mod window;

use std::collections::HashMap;
use std::sync::atomic::{self, AtomicU64};
use std::sync::Arc;
use std::thread;

use parking_lot::{Condvar, Mutex};
pub use window::Window;
use winit::dpi::PhysicalSize;
use winit::event::Event as WinitEvent;
use winit::event_loop::{EventLoopBuilder, EventLoopProxy};
use winit::window::WindowBuilder;

use crate::renderer::Renderer;
use crate::Basalt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowID(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WMHookID(u64);

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

enum WMEvent {
    AssociateBasalt(Arc<Basalt>),
    OnOpen {
        hook_id: WMHookID,
        method: Box<dyn FnMut(Arc<Window>) + Send + 'static>,
    },
    OnClose {
        hook_id: WMHookID,
        method: Box<dyn FnMut(WindowID) + Send + 'static>,
    },
    RemoveHook(WMHookID),
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

impl std::fmt::Debug for WMEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssociateBasalt(_) => write!(f, "AssociateBasalt(Basalt)"),
            Self::OnOpen {
                hook_id, ..
            } => f.write_fmt(format_args!("OnOpen({:?})", hook_id)),
            Self::OnClose {
                hook_id, ..
            } => f.write_fmt(format_args!("OnClose({:?})", hook_id)),
            Self::RemoveHook(hook_id) => f.write_fmt(format_args!("RemoveHook({:?})", hook_id)),
            Self::WindowEvent {
                id,
                event,
            } => {
                f.debug_struct("WindowEvent")
                    .field("id", id)
                    .field("event", event)
                    .finish()
            },
            Self::CreateWindow {
                options, ..
            } => f.write_fmt(format_args!("CreateWindow({:?})", options)),
        }
    }
}

/// Manages windows and their associated events.
pub struct WindowManager {
    event_proxy: EventLoopProxy<WMEvent>,
    next_hook_id: AtomicU64,
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

    pub fn on_open<F: FnMut(Arc<Window>) + Send + 'static>(&self, method: F) -> WMHookID {
        let hook_id = WMHookID(self.next_hook_id.fetch_add(1, atomic::Ordering::SeqCst));

        self.send_event(WMEvent::OnOpen {
            hook_id,
            method: Box::new(method),
        });

        hook_id
    }

    pub fn on_close<F: FnMut(WindowID) + Send + 'static>(&self, method: F) -> WMHookID {
        let hook_id = WMHookID(self.next_hook_id.fetch_add(1, atomic::Ordering::SeqCst));

        self.send_event(WMEvent::OnClose {
            hook_id,
            method: Box::new(method),
        });

        hook_id
    }

    pub fn remove_hook(&self, hook_id: WMHookID) {
        self.send_event(WMEvent::RemoveHook(hook_id));
    }

    pub(crate) fn associate_basalt(&self, basalt: Arc<Basalt>) {
        self.send_event(WMEvent::AssociateBasalt(basalt));
    }

    pub(crate) fn associate_renderer(&self, window_id: WindowID, renderer: Arc<Renderer>) {
        // TODO: This is how the renderer will receive window events
        todo!()
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
            next_hook_id: AtomicU64::new(1),
        });

        let wm_closure = wm.clone();
        thread::spawn(move || exec(wm_closure));

        let mut basalt_op = None;
        let mut next_window_id = 1;
        let mut winit_to_bst_id = HashMap::new();
        let mut windows = HashMap::new();
        let mut on_open_hooks = HashMap::new();
        let mut on_close_hooks = HashMap::new();

        event_loop
            .run(move |event, elwt| {
                match event {
                    WinitEvent::UserEvent(wm_event) => {
                        match wm_event {
                            WMEvent::AssociateBasalt(basalt) => {
                                basalt_op = Some(basalt);
                            },
                            WMEvent::OnOpen {
                                hook_id,
                                method,
                            } => {
                                on_open_hooks.insert(hook_id, method);
                            },
                            WMEvent::OnClose {
                                hook_id,
                                method,
                            } => {
                                on_close_hooks.insert(hook_id, method);
                            },
                            WMEvent::RemoveHook(hook_id) => {
                                on_open_hooks.remove(&hook_id);
                                on_close_hooks.remove(&hook_id);
                            },
                            WMEvent::WindowEvent {
                                id,
                                event,
                            } => {
                                match &event {
                                    WindowEvent::Opened => {
                                        let window: &Arc<Window> = match windows.get(&id) {
                                            Some(some) => some,
                                            None => return,
                                        };

                                        for method in on_open_hooks.values_mut() {
                                            method(window.clone());
                                        }
                                    },
                                    WindowEvent::Closed => {
                                        for method in on_close_hooks.values_mut() {
                                            method(id);
                                        }

                                        if let Some(window) = windows.remove(&id) {
                                            winit_to_bst_id.remove(&window.winit_id());
                                        }
                                    },
                                    _ => (),
                                }

                                // TODO: Send window events to renderer
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

                                wm.send_event(WMEvent::WindowEvent {
                                    id: window_id,
                                    event: WindowEvent::Opened,
                                });

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
