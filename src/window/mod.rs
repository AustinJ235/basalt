//! Window creation and management.

mod key;
mod monitor;
mod window;

use std::collections::HashMap;
use std::sync::atomic::{self, AtomicU64};
use std::sync::Arc;
use std::thread;

use flume::{Receiver, Sender};
pub use monitor::{FullScreenBehavior, FullScreenError, Monitor, MonitorMode};
use parking_lot::{Condvar, FairMutex, FairMutexGuard, Mutex};
pub use window::Window;
use winit::dpi::PhysicalSize;
use winit::event::{
    DeviceEvent, ElementState, Event as WinitEvent, MouseButton as WinitMouseButton,
    MouseScrollDelta, WindowEvent as WinitWindowEvent,
};
use winit::event_loop::{EventLoopBuilder, EventLoopProxy};
use winit::window::WindowBuilder;

use crate::input::{InputEvent, MouseButton};
use crate::interface::bin::{Bin, BinID};
use crate::interface::DefaultFont;
use crate::render::{VSync, MSAA};
use crate::Basalt;

/// An ID that is used to identify a `Window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowID(u64);

/// An ID that is used to identify a hook on `WindowManager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WMHookID(u64);

impl WindowID {
    pub(crate) fn invalid() -> Self {
        Self(0)
    }
}

/// Options for creating a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowOptions {
    /// Set this title of the window.
    ///
    /// Default: `"basalt"`
    pub title: String,
    /// Set the position of the window.
    ///
    /// ***Note:** This may vary depending the window backend.*
    ///
    /// Default: `None`
    pub position: Option<[u32; 2]>,
    /// Set the inner size of the window upon creation.
    ///
    /// ***Note:** When this value is `None`, the window backend decides.*
    ///
    /// Default: `None`
    pub inner_size: Option<[u32; 2]>,
    /// Set the minimum inner size of the window.
    ///
    /// Default: `None`
    pub min_inner_size: Option<[u32; 2]>,
    /// Set the maximum inner size of the window.
    ///
    /// Default: `None`
    pub max_inner_size: Option<[u32; 2]>,
    /// If the window is allowed to be resized.
    ///
    /// Default: `true`
    pub resizeable: bool,
    /// Open the window maximized.
    ///
    /// Default: `false`
    pub maximized: bool,
    /// Open the window minimized.
    ///
    /// Default: `false`
    pub minimized: bool,
    /// Open the window full screen with the given behavior.
    ///
    /// Default: `None`
    pub fullscreen: Option<FullScreenBehavior>,
    /// If the window should have decorations.
    ///
    /// Default: `true`
    pub decorations: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: String::from("basalt"),
            position: None,
            inner_size: None,
            min_inner_size: None,
            max_inner_size: None,
            resizeable: true,
            maximized: false,
            minimized: false,
            fullscreen: None,
            decorations: true,
        }
    }
}

#[derive(Clone)]
pub(crate) enum WindowEvent {
    Opened,
    Closed,
    Resized { width: u32, height: u32 },
    ScaleChanged(f32),
    RedrawRequested,
    EnabledFullscreen,
    DisabledFullscreen,
    AssociateBin(Arc<Bin>),
    DissociateBin(BinID),
    UpdateBin(BinID),
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    SetMSAA(MSAA),
    SetVSync(VSync),
}

/// An enum that specifies the backend that a window uses.
///
/// This may be important for implementing backend specific quirks.
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
    WindowEventQueue {
        id: WindowID,
        cond: Arc<Condvar>,
        result: Arc<Mutex<Option<Option<Receiver<WindowEvent>>>>>,
    },
    CreateWindow {
        options: WindowOptions,
        cond: Arc<Condvar>,
        result: Arc<Mutex<Option<Result<Arc<Window>, String>>>>,
    },
    GetPrimaryMonitor {
        cond: Arc<Condvar>,
        result: Arc<Mutex<Option<Option<Monitor>>>>,
    },
    GetMonitors {
        cond: Arc<Condvar>,
        result: Arc<Mutex<Option<Vec<Monitor>>>>,
    },
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    Exit,
}

impl std::fmt::Debug for WMEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssociateBasalt(_) => write!(f, "AssociateBasalt"),
            Self::OnOpen {
                hook_id, ..
            } => f.write_fmt(format_args!("OnOpen({:?})", hook_id)),
            Self::OnClose {
                hook_id, ..
            } => f.write_fmt(format_args!("OnClose({:?})", hook_id)),
            Self::RemoveHook(hook_id) => f.write_fmt(format_args!("RemoveHook({:?})", hook_id)),
            Self::WindowEvent {
                id, ..
            } => f.debug_struct("WindowEvent").field("id", id).finish(),
            Self::WindowEventQueue {
                id, ..
            } => f.debug_struct("WindowEventQueue").field("id", id).finish(),
            Self::CreateWindow {
                options, ..
            } => f.write_fmt(format_args!("CreateWindow({:?})", options)),
            Self::GetPrimaryMonitor {
                ..
            } => write!(f, "GetPrimaryMonitor"),
            Self::GetMonitors {
                ..
            } => write!(f, "GetMonitors"),
            Self::AddBinaryFont(_) => write!(f, "AddBinaryFont"),
            Self::SetDefaultFont(_) => write!(f, "SetDefaultFont"),
            Self::Exit => write!(f, "Exit"),
        }
    }
}

/// Manages windows and their associated events.
pub struct WindowManager {
    event_proxy: EventLoopProxy<WMEvent>,
    next_hook_id: AtomicU64,
    windows: Mutex<HashMap<WindowID, Arc<Window>>>,
    draw_lock: FairMutex<()>,
}

pub(crate) struct DrawGuard<'a> {
    inner: FairMutexGuard<'a, ()>,
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

    /// Retrieves an `Arc<Window>` given a `WindowID`.
    pub fn window(&self, window_id: WindowID) -> Option<Arc<Window>> {
        self.windows.lock().get(&window_id).cloned()
    }

    /// Retrieves all `Arc<Window>`'s.
    pub fn windows(&self) -> Vec<Arc<Window>> {
        self.windows.lock().values().cloned().collect()
    }

    /// Return a list of active monitors on the system.
    pub fn monitors(&self) -> Vec<Monitor> {
        let result = Arc::new(Mutex::new(None));
        let cond = Arc::new(Condvar::new());

        self.event_proxy
            .send_event(WMEvent::GetMonitors {
                result: result.clone(),
                cond: cond.clone(),
            })
            .unwrap();

        let mut result_guard = result.lock();

        while result_guard.is_none() {
            cond.wait(&mut result_guard);
        }

        result_guard.take().unwrap()
    }

    /// Return the primary monitor if the implementation is able to determine it.
    pub fn primary_monitor(&self) -> Option<Monitor> {
        let result = Arc::new(Mutex::new(None));
        let cond = Arc::new(Condvar::new());

        self.event_proxy
            .send_event(WMEvent::GetPrimaryMonitor {
                result: result.clone(),
                cond: cond.clone(),
            })
            .unwrap();

        let mut result_guard = result.lock();

        while result_guard.is_none() {
            cond.wait(&mut result_guard);
        }

        result_guard.take().unwrap()
    }

    /// Create a hook that is called whenever a window is opened.
    pub fn on_open<F: FnMut(Arc<Window>) + Send + 'static>(&self, method: F) -> WMHookID {
        let hook_id = WMHookID(self.next_hook_id.fetch_add(1, atomic::Ordering::SeqCst));

        self.send_event(WMEvent::OnOpen {
            hook_id,
            method: Box::new(method),
        });

        hook_id
    }

    /// Create a hook that is called whenever a window is closed.
    pub fn on_close<F: FnMut(WindowID) + Send + 'static>(&self, method: F) -> WMHookID {
        let hook_id = WMHookID(self.next_hook_id.fetch_add(1, atomic::Ordering::SeqCst));

        self.send_event(WMEvent::OnClose {
            hook_id,
            method: Box::new(method),
        });

        hook_id
    }

    /// Remove a hook given a `WMHookID`
    pub fn remove_hook(&self, hook_id: WMHookID) {
        self.send_event(WMEvent::RemoveHook(hook_id));
    }

    pub(crate) fn associate_basalt(&self, basalt: Arc<Basalt>) {
        self.send_event(WMEvent::AssociateBasalt(basalt));
    }

    pub(crate) fn window_event_queue(&self, window_id: WindowID) -> Option<Receiver<WindowEvent>> {
        let result = Arc::new(Mutex::new(None));
        let cond = Arc::new(Condvar::new());

        self.send_event(WMEvent::WindowEventQueue {
            id: window_id,
            result: result.clone(),
            cond: cond.clone(),
        });

        let mut result_guard = result.lock();

        while result_guard.is_none() {
            cond.wait(&mut result_guard);
        }

        result_guard.take().unwrap()
    }

    pub(crate) fn request_draw(&self) -> DrawGuard {
        DrawGuard {
            inner: self.draw_lock.lock(),
        }
    }

    pub(crate) fn add_binary_font(&self, binary_font: Arc<dyn AsRef<[u8]> + Sync + Send>) {
        self.send_event(WMEvent::AddBinaryFont(binary_font));
    }

    pub(crate) fn set_default_font(&self, default_font: DefaultFont) {
        self.send_event(WMEvent::SetDefaultFont(default_font));
    }

    fn send_window_event(&self, id: WindowID, event: WindowEvent) {
        self.send_event(WMEvent::WindowEvent {
            id,
            event,
        });
    }

    pub(crate) fn exit(&self) {
        self.send_event(WMEvent::Exit);
    }

    fn send_event(&self, event: WMEvent) {
        self.event_proxy.send_event(event).unwrap();
    }

    pub(crate) fn run<F: FnMut(Arc<Self>) + Send + 'static>(mut exec: F) {
        let event_loop = EventLoopBuilder::<WMEvent>::with_user_event()
            .build()
            .unwrap();
        let event_proxy = event_loop.create_proxy();

        let wm = Arc::new(Self {
            event_proxy,
            next_hook_id: AtomicU64::new(1),
            windows: Mutex::new(HashMap::new()),
            draw_lock: FairMutex::new(()),
        });

        let wm_closure = wm.clone();
        thread::spawn(move || exec(wm_closure));

        let mut basalt_op = None;
        let mut next_window_id = 1;
        let mut windows = HashMap::new();
        let mut window_event_senders: HashMap<WindowID, Sender<WindowEvent>> = HashMap::new();
        let mut winit_to_bst_id = HashMap::new();
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

                                        window_event_senders.remove(&id);
                                        wm.windows.lock().remove(&id);
                                    },
                                    _ => (),
                                }

                                if let Some(sender) = window_event_senders.get(&id) {
                                    if sender.send(event).is_err() {
                                        window_event_senders.remove(&id);
                                    }
                                }
                            },
                            WMEvent::WindowEventQueue {
                                id,
                                cond,
                                result,
                            } => {
                                if window_event_senders.contains_key(&id) {
                                    *result.lock() = Some(None);
                                    cond.notify_one();
                                    return;
                                }

                                let (send, recv) = flume::unbounded::<WindowEvent>();
                                window_event_senders.insert(id, send);
                                *result.lock() = Some(Some(recv));
                                cond.notify_one();
                            },
                            WMEvent::CreateWindow {
                                mut options,
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

                                let mut window_builder = WindowBuilder::new()
                                    .with_title(options.title)
                                    .with_resizable(options.resizeable)
                                    .with_maximized(options.maximized)
                                    .with_visible(!options.minimized)
                                    .with_decorations(options.decorations);

                                if let Some(inner_size) = options.inner_size.take() {
                                    window_builder = window_builder.with_inner_size(
                                        PhysicalSize::new(inner_size[0], inner_size[1]),
                                    );
                                }

                                if let Some(min_inner_size) = options.min_inner_size.take() {
                                    window_builder = window_builder.with_min_inner_size(
                                        PhysicalSize::new(min_inner_size[0], min_inner_size[1]),
                                    );
                                }

                                if let Some(max_inner_size) = options.max_inner_size.take() {
                                    window_builder = window_builder.with_max_inner_size(
                                        PhysicalSize::new(max_inner_size[0], max_inner_size[1]),
                                    );
                                }

                                if let Some(fullscreen_behavior) = options.fullscreen {
                                    let primary_op = elwt.primary_monitor();
                                    let mut primary_monitor = None;

                                    let monitors = elwt
                                        .available_monitors()
                                        .filter_map(|winit_monitor| {
                                            let is_primary = match primary_op.as_ref() {
                                                Some(primary) => *primary == winit_monitor,
                                                None => false,
                                            };

                                            let mut monitor = Monitor::from_winit(winit_monitor)?;
                                            monitor.is_primary = is_primary;

                                            if is_primary {
                                                primary_monitor = Some(monitor.clone());
                                            }

                                            Some(monitor)
                                        })
                                        .collect::<Vec<_>>();

                                    if let Ok(winit_fullscreen) = fullscreen_behavior
                                        .determine_winit_fullscreen(
                                            true,
                                            basalt
                                                .device_ref()
                                                .enabled_extensions()
                                                .ext_full_screen_exclusive,
                                            None,
                                            primary_monitor,
                                            monitors,
                                        )
                                    {
                                        window_builder =
                                            window_builder.with_fullscreen(Some(winit_fullscreen));
                                    }
                                }

                                let winit_window = match window_builder.build(elwt) {
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
                                wm.windows.lock().insert(window_id, window.clone());

                                wm.send_event(WMEvent::WindowEvent {
                                    id: window_id,
                                    event: WindowEvent::Opened,
                                });

                                *result.lock() = Some(Ok(window));
                                cond.notify_one();
                            },
                            WMEvent::GetMonitors {
                                result,
                                cond,
                            } => {
                                let primary_op = elwt.primary_monitor();

                                *result.lock() = Some(
                                    elwt.available_monitors()
                                        .filter_map(|winit_monitor| {
                                            let is_primary = match primary_op.as_ref() {
                                                Some(primary) => *primary == winit_monitor,
                                                None => false,
                                            };

                                            let mut monitor = Monitor::from_winit(winit_monitor)?;
                                            monitor.is_primary = is_primary;
                                            Some(monitor)
                                        })
                                        .collect::<Vec<_>>(),
                                );

                                cond.notify_one();
                            },
                            WMEvent::GetPrimaryMonitor {
                                result,
                                cond,
                            } => {
                                *result.lock() =
                                    Some(elwt.primary_monitor().and_then(|winit_monitor| {
                                        let mut monitor = Monitor::from_winit(winit_monitor)?;
                                        monitor.is_primary = true;
                                        Some(monitor)
                                    }));

                                cond.notify_one();
                            },
                            WMEvent::AddBinaryFont(binary_font) => {
                                for window_event_sender in window_event_senders.values() {
                                    let _ = window_event_sender
                                        .send(WindowEvent::AddBinaryFont(binary_font.clone()));
                                }
                            },
                            WMEvent::SetDefaultFont(default_font) => {
                                for window_event_sender in window_event_senders.values() {
                                    let _ = window_event_sender
                                        .send(WindowEvent::SetDefaultFont(default_font.clone()));
                                }
                            },
                            WMEvent::Exit => {
                                elwt.exit();
                            },
                        }
                    },
                    WinitEvent::WindowEvent {
                        window_id: winit_window_id,
                        event: winit_window_event,
                    } => {
                        let basalt = match basalt_op.as_ref() {
                            Some(some) => some,
                            None => return,
                        };

                        let window_id = match winit_to_bst_id.get(&winit_window_id) {
                            Some(some) => some,
                            None => return,
                        };

                        let window = windows.get(window_id).unwrap();

                        match winit_window_event {
                            WinitWindowEvent::Resized(physical_size) => {
                                wm.send_event(WMEvent::WindowEvent {
                                    id: *window_id,
                                    event: WindowEvent::Resized {
                                        width: physical_size.width,
                                        height: physical_size.height,
                                    },
                                });
                            },
                            WinitWindowEvent::CloseRequested | WinitWindowEvent::Destroyed => {
                                window.close();
                            },
                            WinitWindowEvent::Focused(focused) => {
                                basalt.input_ref().send_event(match focused {
                                    true => {
                                        InputEvent::Focus {
                                            win: *window_id,
                                        }
                                    },
                                    false => {
                                        InputEvent::FocusLost {
                                            win: *window_id,
                                        }
                                    },
                                });
                            },
                            WinitWindowEvent::KeyboardInput {
                                event, ..
                            } => {
                                if let Some(qwerty) = key::event_to_qwerty(&event) {
                                    basalt.input_ref().send_event(match event.state {
                                        ElementState::Pressed => {
                                            InputEvent::Press {
                                                win: *window_id,
                                                key: qwerty.into(),
                                            }
                                        },
                                        ElementState::Released => {
                                            InputEvent::Release {
                                                win: *window_id,
                                                key: qwerty.into(),
                                            }
                                        },
                                    });
                                }

                                if let Some(text) = event.text {
                                    for c in text.as_str().chars() {
                                        basalt.input_ref().send_event(InputEvent::Character {
                                            win: *window_id,
                                            c,
                                        });
                                    }
                                }
                            },
                            WinitWindowEvent::CursorMoved {
                                position, ..
                            } => {
                                basalt.input_ref().send_event(InputEvent::Cursor {
                                    win: *window_id,
                                    x: position.x as f32,
                                    y: position.y as f32,
                                });
                            },
                            WinitWindowEvent::CursorEntered {
                                ..
                            } => {
                                basalt.input_ref().send_event(InputEvent::Enter {
                                    win: *window_id,
                                });
                            },
                            WinitWindowEvent::CursorLeft {
                                ..
                            } => {
                                basalt.input_ref().send_event(InputEvent::Leave {
                                    win: *window_id,
                                });
                            },
                            WinitWindowEvent::MouseWheel {
                                delta, ..
                            } => {
                                // TODO: Check consistency across platforms.

                                let [v, h] = match delta {
                                    MouseScrollDelta::LineDelta(x, y) => [-y, x],
                                    MouseScrollDelta::PixelDelta(position) => {
                                        [-position.y as f32, position.x as f32]
                                    },
                                };

                                basalt.input_ref().send_event(InputEvent::Scroll {
                                    win: *window_id,
                                    v: v.clamp(-1.0, 1.0),
                                    h: h.clamp(-1.0, 1.0),
                                });
                            },
                            WinitWindowEvent::MouseInput {
                                state,
                                button,
                                ..
                            } => {
                                let button = match button {
                                    WinitMouseButton::Left => MouseButton::Left,
                                    WinitMouseButton::Right => MouseButton::Right,
                                    WinitMouseButton::Middle => MouseButton::Middle,
                                    _ => return,
                                };

                                basalt.input_ref().send_event(match state {
                                    ElementState::Pressed => {
                                        InputEvent::Press {
                                            win: *window_id,
                                            key: button.into(),
                                        }
                                    },
                                    ElementState::Released => {
                                        InputEvent::Release {
                                            win: *window_id,
                                            key: button.into(),
                                        }
                                    },
                                });
                            },
                            WinitWindowEvent::ScaleFactorChanged {
                                scale_factor,
                                mut inner_size_writer,
                            } => {
                                if window.ignoring_dpi() {
                                    let _ = inner_size_writer
                                        .request_inner_size(window.inner_dimensions().into());
                                }

                                window.set_dpi_scale(scale_factor as f32);
                            },
                            WinitWindowEvent::RedrawRequested => {
                                wm.send_event(WMEvent::WindowEvent {
                                    id: *window_id,
                                    event: WindowEvent::RedrawRequested,
                                });
                            },
                            _ => (),
                        }
                    },
                    WinitEvent::DeviceEvent {
                        event: device_event,
                        ..
                    } => {
                        let basalt = match basalt_op.as_ref() {
                            Some(some) => some,
                            None => return,
                        };

                        if let DeviceEvent::Motion {
                            axis,
                            value,
                        } = device_event
                        {
                            basalt.input_ref().send_event(match axis {
                                0 => {
                                    InputEvent::Motion {
                                        x: -value as f32,
                                        y: 0.0,
                                    }
                                },
                                1 => {
                                    InputEvent::Motion {
                                        x: 0.0,
                                        y: -value as f32,
                                    }
                                },
                                _ => return,
                            });
                        }
                    },
                    _ => (),
                }
            })
            .unwrap();
    }
}
