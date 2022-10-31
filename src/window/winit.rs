use std::ops::Deref;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use vulkano::instance::Instance;
use vulkano::swapchain::{Surface, Win32Monitor};

use super::{
    BasaltWindow, BstWindowID, FullScreenBehavior, FullScreenError, Monitor, MonitorHandle,
    MonitorMode, MonitorModeHandle, WindowType,
};
use crate::input::{InputEvent, InputHookID, MouseButton, Qwerty};
use crate::{Basalt, BstEvent, BstOptions, BstWinEv};

mod winit_ty {
    pub use winit::dpi::PhysicalSize;
    pub use winit::event::{
        DeviceEvent, ElementState, Event, KeyboardInput, MouseButton, MouseScrollDelta, WindowEvent,
    };
    pub use winit::event_loop::{ControlFlow, EventLoop};
    pub use winit::monitor::MonitorHandle;
    pub use winit::window::{CursorGrabMode, Fullscreen, Window, WindowBuilder};
}

pub struct WinitWindow {
    id: BstWindowID,
    inner: Arc<winit_ty::Window>,
    basalt: Mutex<Option<Arc<Basalt>>>,
    basalt_ready: Condvar,
    cursor_captured: AtomicBool,
    window_type: Mutex<WindowType>,
    input_hook_ids: Mutex<Vec<InputHookID>>,
}

impl BasaltWindow for WinitWindow {
    fn id(&self) -> BstWindowID {
        self.id
    }

    fn basalt(&self) -> Arc<Basalt> {
        self.basalt
            .lock()
            .deref()
            .clone()
            .expect("Window doesn't have access to Basalt!")
    }

    fn attach_input_hook(&self, id: InputHookID) {
        self.input_hook_ids.lock().push(id);
    }

    fn capture_cursor(&self) {
        let basalt = self
            .basalt
            .lock()
            .deref()
            .clone()
            .expect("Window doesn't have access to Basalt!");

        self.inner.set_cursor_visible(false);
        self.inner
            .set_cursor_grab(winit_ty::CursorGrabMode::Confined)
            .unwrap();
        self.cursor_captured.store(true, atomic::Ordering::SeqCst);

        basalt.input_ref().send_event(InputEvent::CursorCapture {
            win: self.id,
            captured: true,
        });
    }

    fn release_cursor(&self) {
        let basalt = self
            .basalt
            .lock()
            .deref()
            .clone()
            .expect("Window doesn't have access to Basalt!");

        self.inner.set_cursor_visible(true);
        self.inner
            .set_cursor_grab(winit_ty::CursorGrabMode::None)
            .unwrap();
        self.cursor_captured.store(false, atomic::Ordering::SeqCst);

        basalt.input_ref().send_event(InputEvent::CursorCapture {
            win: self.id,
            captured: false,
        });
    }

    fn cursor_captured(&self) -> bool {
        self.cursor_captured.load(atomic::Ordering::SeqCst)
    }

    fn primary_monitor(&self) -> Option<Monitor> {
        self.inner.primary_monitor().and_then(|winit_monitor| {
            let is_current = match self.inner.current_monitor() {
                Some(current) => current == winit_monitor,
                None => false,
            };

            let mut monitor = Monitor::try_from(winit_monitor).ok()?;
            monitor.is_primary = true;
            monitor.is_current = is_current;
            Some(monitor)
        })
    }

    fn current_monitor(&self) -> Option<Monitor> {
        self.inner.current_monitor().and_then(|winit_monitor| {
            let is_primary = match self.inner.primary_monitor() {
                Some(primary) => primary == winit_monitor,
                None => false,
            };

            let mut monitor = Monitor::try_from(winit_monitor).ok()?;
            monitor.is_current = true;
            monitor.is_primary = is_primary;
            Some(monitor)
        })
    }

    fn monitors(&self) -> Vec<Monitor> {
        let current_op = self.inner.current_monitor();
        let primary_op = self.inner.primary_monitor();

        self.inner
            .available_monitors()
            .filter_map(|winit_monitor| {
                let is_current = match current_op.as_ref() {
                    Some(current) => *current == winit_monitor,
                    None => false,
                };

                let is_primary = match primary_op.as_ref() {
                    Some(primary) => *primary == winit_monitor,
                    None => false,
                };

                let mut monitor = Monitor::try_from(winit_monitor).ok()?;
                monitor.is_current = is_current;
                monitor.is_primary = is_primary;
                Some(monitor)
            })
            .collect()
    }

    fn enable_fullscreen(&self, mut behavior: FullScreenBehavior) -> Result<(), FullScreenError> {
        let basalt = self
            .basalt
            .lock()
            .deref()
            .clone()
            .expect("Window doesn't have access to Basalt!");
        let exclusive_supported = basalt.options_ref().exclusive_fullscreen;

        if behavior == FullScreenBehavior::Auto {
            if exclusive_supported {
                behavior = FullScreenBehavior::AutoExclusive;
            } else {
                behavior = FullScreenBehavior::AutoBorderless;
            }
        }

        if behavior.is_exclusive() && !exclusive_supported {
            return Err(FullScreenError::ExclusiveNotSupported);
        }

        if behavior.is_exclusive() {
            let (monitor, mode) = match behavior {
                FullScreenBehavior::AutoExclusive => {
                    let monitor = match self.current_monitor() {
                        Some(some) => some,
                        None => {
                            match self.primary_monitor() {
                                Some(some) => some,
                                None => {
                                    match self.monitors().drain(0..1).next() {
                                        Some(some) => some,
                                        None => return Err(FullScreenError::NoAvailableMonitors),
                                    }
                                },
                            }
                        },
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::AutoExclusivePrimary => {
                    let monitor = match self.primary_monitor() {
                        Some(some) => some,
                        None => return Err(FullScreenError::UnableToDeterminePrimary),
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::AutoExclusiveCurrent => {
                    let monitor = match self.current_monitor() {
                        Some(some) => some,
                        None => return Err(FullScreenError::UnableToDetermineCurrent),
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::ExclusiveAutoMode(monitor) => {
                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::Exclusive(monitor, mode) => (monitor, mode),
                _ => unreachable!(),
            };

            if mode.monitor_handle != monitor.handle {
                return Err(FullScreenError::IncompatibleMonitorMode);
            }

            self.inner
                .set_fullscreen(Some(winit_ty::Fullscreen::Exclusive(
                    mode.handle.into_winit(),
                )));

            basalt.send_event(BstEvent::BstWinEv(BstWinEv::FullScreenExclusive(true)));
        } else {
            let monitor_op = match behavior {
                FullScreenBehavior::AutoBorderless => {
                    match self.current_monitor() {
                        Some(some) => Some(some),
                        None => self.primary_monitor(),
                    }
                },
                FullScreenBehavior::AutoBorderlessPrimary => {
                    match self.primary_monitor() {
                        Some(some) => Some(some),
                        None => return Err(FullScreenError::UnableToDeterminePrimary),
                    }
                },
                FullScreenBehavior::AutoBorderlessCurrent => {
                    match self.current_monitor() {
                        Some(some) => Some(some),
                        None => return Err(FullScreenError::UnableToDetermineCurrent),
                    }
                },
                FullScreenBehavior::Borderless(monitor) => Some(monitor),
                _ => unreachable!(),
            };

            self.inner
                .set_fullscreen(Some(winit_ty::Fullscreen::Borderless(
                    monitor_op.map(|monitor| monitor.handle.into_winit()),
                )));
        }

        Ok(())
    }

    fn disable_fullscreen(&self) {
        self.inner.set_fullscreen(None);
        let basalt = self
            .basalt
            .lock()
            .deref()
            .clone()
            .expect("Window doesn't have access to Basalt!");

        if basalt.options_ref().exclusive_fullscreen {
            basalt.send_event(BstEvent::BstWinEv(BstWinEv::FullScreenExclusive(false)));
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.inner.fullscreen().is_some()
    }

    fn toggle_fullscreen(&self) {
        if self.inner.fullscreen().is_none() {
            let _ = self.enable_fullscreen(Default::default());
        } else {
            self.disable_fullscreen();
        }
    }

    fn request_resize(&self, width: u32, height: u32) {
        self.inner
            .set_inner_size(winit_ty::PhysicalSize::new(width as f64, height as f64));
    }

    unsafe fn attach_basalt(&self, basalt: Arc<Basalt>) {
        *self.basalt.lock() = Some(basalt);
        self.basalt_ready.notify_one();
    }

    fn inner_dimensions(&self) -> [u32; 2] {
        self.inner.inner_size().into()
    }

    fn window_type(&self) -> WindowType {
        *self.window_type.lock()
    }

    fn scale_factor(&self) -> f32 {
        self.inner.scale_factor() as f32
    }

    fn win32_monitor(&self) -> Option<Win32Monitor> {
        #[cfg(target_os = "windows")]
        unsafe {
            use winit::platform::windows::MonitorHandleExtWindows;

            self.inner
                .current_monitor()
                .map(|m| Win32Monitor::new(m.hmonitor() as *const std::ffi::c_void))
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }
}

impl std::fmt::Debug for WinitWindow {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmtr.pad("WinitWindow { .. }")
    }
}

impl TryFrom<winit_ty::MonitorHandle> for Monitor {
    type Error = ();

    fn try_from(winit_monitor: winit_ty::MonitorHandle) -> Result<Self, Self::Error> {
        // Should always be some, "Returns None if the monitor doesn’t exist anymore."
        let name = match winit_monitor.name() {
            Some(some) => some,
            None => return Err(()),
        };

        let physical_size = winit_monitor.size();
        let resolution = [physical_size.width, physical_size.height];
        let physical_position = winit_monitor.position();
        let position = [physical_position.x, physical_position.y];

        let refresh_rate_op = winit_monitor
            .refresh_rate_millihertz()
            .map(|mhz| OrderedFloat::from(mhz as f32 / 1000.0));

        let modes: Vec<MonitorMode> = winit_monitor
            .video_modes()
            .map(|winit_mode| {
                let physical_size = winit_mode.size();
                let resolution = [physical_size.width, physical_size.height];
                let bit_depth = winit_mode.bit_depth();

                let refresh_rate =
                    OrderedFloat::from(winit_mode.refresh_rate_millihertz() as f32 / 1000.0);

                MonitorMode {
                    resolution,
                    bit_depth,
                    refresh_rate,
                    handle: MonitorModeHandle::Winit(winit_mode),
                    monitor_handle: MonitorHandle::Winit(winit_monitor.clone()),
                }
            })
            .collect();

        if modes.is_empty() {
            return Err(());
        }

        let refresh_rate = refresh_rate_op.unwrap_or_else(|| {
            modes
                .iter()
                .max_by_key(|mode| mode.refresh_rate)
                .unwrap()
                .refresh_rate
        });

        let bit_depth = modes
            .iter()
            .max_by_key(|mode| mode.bit_depth)
            .unwrap()
            .bit_depth;

        Ok(Monitor {
            name,
            resolution,
            position,
            refresh_rate,
            bit_depth,
            is_current: false,
            is_primary: false,
            modes,
            handle: MonitorHandle::Winit(winit_monitor),
        })
    }
}

pub fn open_surface(
    ops: BstOptions,
    id: BstWindowID,
    instance: Arc<Instance>,
    result_fn: Box<dyn Fn(Result<(Arc<Surface>, Arc<dyn BasaltWindow>), String>) + Send + Sync>,
) {
    let event_loop = winit_ty::EventLoop::new();

    let inner = match winit_ty::WindowBuilder::new()
        .with_inner_size(winit_ty::PhysicalSize::new(
            ops.window_size[0],
            ops.window_size[1],
        ))
        .with_title(ops.title)
        .build(&event_loop)
    {
        Ok(ok) => Arc::new(ok),
        Err(e) => return result_fn(Err(format!("Failed to build window: {}", e))),
    };

    let window = Arc::new(WinitWindow {
        id,
        inner,
        basalt: Mutex::new(None),
        basalt_ready: Condvar::new(),
        cursor_captured: AtomicBool::new(false),
        window_type: Mutex::new(WindowType::NotSupported),
        input_hook_ids: Mutex::new(Vec::new()),
    });

    match unsafe {
        match window.inner.raw_window_handle() {
            RawWindowHandle::Win32(handle) => {
                match Surface::from_win32(instance, handle.hinstance, handle.hwnd, None) {
                    Ok(ok) => Ok((WindowType::Windows, ok)),
                    Err(e) => Err(format!("Failed to create win32 surface: {}", e)),
                }
            },
            RawWindowHandle::Wayland(handle) => {
                match window.inner.raw_display_handle() {
                    RawDisplayHandle::Wayland(display) => {
                        match Surface::from_wayland(instance, display.display, handle.surface, None)
                        {
                            Ok(ok) => Ok((WindowType::UnixWayland, ok)),
                            Err(e) => Err(format!("Failed to create wayland surface: {}", e)),
                        }
                    },
                    _ => {
                        Err(String::from(
                            "Failed to create wayland surface: invalid display handle",
                        ))
                    },
                }
            },
            RawWindowHandle::Xlib(handle) => {
                match window.inner.raw_display_handle() {
                    RawDisplayHandle::Xlib(display) => {
                        match Surface::from_xlib(instance, display.display, handle.window, None) {
                            Ok(ok) => Ok((WindowType::UnixXlib, ok)),
                            Err(e) => Err(format!("Failed to create xlib surface: {}", e)),
                        }
                    },
                    _ => {
                        Err(String::from(
                            "Failed to create xlib surface: invalid display handle",
                        ))
                    },
                }
            },
            RawWindowHandle::Xcb(handle) => {
                match window.inner.raw_display_handle() {
                    RawDisplayHandle::Xcb(display) => {
                        match Surface::from_xcb(instance, display.connection, handle.window, None) {
                            Ok(ok) => Ok((WindowType::UnixXCB, ok)),
                            Err(e) => Err(format!("Failed to create xcb surface: {}", e)),
                        }
                    },
                    _ => {
                        Err(String::from(
                            "Failed to create xcb surface: invalid display handle",
                        ))
                    },
                }
            },
            // Note: MacOS isn't officially supported, it is unknow whether this code actually works.
            #[allow(unused_variables)]
            RawWindowHandle::UiKit(handle) => {
                #[cfg(target_os = "macos")]
                {
                    use core_graphics_types::base::CGFloat;
                    use core_graphics_types::geometry::CGRect;
                    use objc::runtime::{Object, BOOL, NO, YES};
                    use objc::{class, msg_send, sel, sel_impl};

                    let view: *mut Object = std::mem::transmute(view);
                    let main_layer: *mut Object = msg_send![view, layer];
                    let class = class!(CAMetalLayer);
                    let is_valid_layer: BOOL = msg_send![main_layer, isKindOfClass: class];

                    let layer = if is_valid_layer == NO {
                        let new_layer: *mut Object = msg_send![class, new];
                        let () = msg_send![new_layer, setEdgeAntialiasingMask: 0];
                        let () = msg_send![new_layer, setPresentsWithTransaction: false];
                        let () = msg_send![new_layer, removeAllAnimations];
                        let () = msg_send![view, setLayer: new_layer];
                        let () = msg_send![view, setWantsLayer: YES];
                        let window: *mut Object = msg_send![view, window];

                        if !window.is_null() {
                            let scale_factor: CGFloat = msg_send![window, backingScaleFactor];
                            let () = msg_send![new_layer, setContentsScale: scale_factor];
                        }

                        new_layer
                    } else {
                        main_layer
                    };

                    match Surface::from_mac_os(instance, layer as *const (), None) {
                        Ok(ok) => Ok((WindowType::Macos, ok)),
                        Err(e) => Err(format!("Failed to create UiKit surface: {}", e)),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Err(String::from(
                        "Failed to crate UiKit surface: target_os != 'macos'",
                    ))
                }
            },
            _ => {
                Err(String::from(
                    "Failed to create surface: window is not supported",
                ))
            },
        }
    } {
        Ok((window_type, surface)) => {
            *window.window_type.lock() = window_type;
            let bst_window = window.clone() as Arc<dyn BasaltWindow>;
            thread::spawn(move || result_fn(Ok((surface, bst_window))));
        },
        Err(e) => return result_fn(Err(e)),
    }

    let basalt = {
        let mut lock = window.basalt.lock();
        window
            .basalt_ready
            .wait_for(&mut lock, Duration::from_millis(1000));
        lock.clone().unwrap()
    };

    let window_type = *window.window_type.lock();

    event_loop.run(move |event: winit_ty::Event<'_, ()>, _, control_flow| {
        *control_flow = winit_ty::ControlFlow::Wait;

        match event {
            winit_ty::Event::WindowEvent {
                event: winit_ty::WindowEvent::CloseRequested,
                ..
            } => {
                basalt.exit();
                *control_flow = winit_ty::ControlFlow::Exit;
            },

            winit_ty::Event::WindowEvent {
                event:
                    winit_ty::WindowEvent::CursorMoved {
                        position, ..
                    },
                ..
            } => {
                basalt.input_ref().send_event(InputEvent::Cursor {
                    win: window.id(),
                    x: position.x as f32,
                    y: position.y as f32,
                });
            },

            winit_ty::Event::WindowEvent {
                event:
                    winit_ty::WindowEvent::KeyboardInput {
                        input:
                            winit_ty::KeyboardInput {
                                scancode,
                                state,
                                ..
                            },
                        ..
                    },
                ..
            } => {
                #[cfg(target_os = "windows")]
                {
                    if scancode == 0 {
                        return;
                    }
                }

                match state {
                    winit_ty::ElementState::Pressed => {
                        basalt.input_ref().send_event(InputEvent::Press {
                            win: window.id(),
                            key: Qwerty::from(scancode).into(),
                        });
                    },
                    winit_ty::ElementState::Released => {
                        basalt.input_ref().send_event(InputEvent::Release {
                            win: window.id(),
                            key: Qwerty::from(scancode).into(),
                        });
                    },
                }
            },

            winit_ty::Event::WindowEvent {
                event:
                    winit_ty::WindowEvent::MouseInput {
                        state,
                        button,
                        ..
                    },
                ..
            } => {
                let button = match button {
                    winit_ty::MouseButton::Left => MouseButton::Left,
                    winit_ty::MouseButton::Right => MouseButton::Right,
                    winit_ty::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };

                match state {
                    winit_ty::ElementState::Pressed => {
                        basalt.input_ref().send_event(InputEvent::Press {
                            win: window.id(),
                            key: button.into(),
                        });
                    },
                    winit_ty::ElementState::Released => {
                        basalt.input_ref().send_event(InputEvent::Release {
                            win: window.id(),
                            key: button.into(),
                        });
                    },
                }
            },

            winit_ty::Event::WindowEvent {
                event:
                    winit_ty::WindowEvent::MouseWheel {
                        delta, ..
                    },
                ..
            } => {
                let [v, h] = match &window_type {
                    WindowType::UnixWayland | WindowType::Windows => {
                        match delta {
                            winit_ty::MouseScrollDelta::PixelDelta(logical_position) => {
                                [-logical_position.y as f32, logical_position.x as f32]
                            },
                            winit_ty::MouseScrollDelta::LineDelta(x, y) => [-y as f32, x as f32],
                        }
                    },
                    _ => return,
                };

                basalt.input_ref().send_event(InputEvent::Scroll {
                    win: window.id(),
                    v,
                    h,
                });
            },

            winit_ty::Event::WindowEvent {
                event:
                    winit_ty::WindowEvent::CursorEntered {
                        ..
                    },
                ..
            } => {
                basalt.input_ref().send_event(InputEvent::Enter {
                    win: window.id(),
                });
            },

            winit_ty::Event::WindowEvent {
                event: winit_ty::WindowEvent::CursorLeft {
                    ..
                },
                ..
            } => {
                basalt.input_ref().send_event(InputEvent::Leave {
                    win: window.id(),
                });
            },

            winit_ty::Event::WindowEvent {
                event: winit_ty::WindowEvent::Resized(physical_size),
                ..
            } => {
                basalt.send_event(BstEvent::BstWinEv(BstWinEv::Resized(
                    physical_size.width,
                    physical_size.height,
                )));
            },

            winit_ty::Event::RedrawRequested(_) => {
                basalt.send_event(BstEvent::BstWinEv(BstWinEv::RedrawRequest));
            },

            winit_ty::Event::WindowEvent {
                event:
                    winit_ty::WindowEvent::ScaleFactorChanged {
                        ..
                    },
                ..
            } => {
                let scale = window.inner.scale_factor() as f32;
                basalt.interface_ref().set_window_scale(scale);
            },

            winit_ty::Event::WindowEvent {
                event: winit_ty::WindowEvent::Focused(focused),
                ..
            } => {
                if focused {
                    basalt.input_ref().send_event(InputEvent::Focus {
                        win: window.id(),
                    });
                } else {
                    basalt.input_ref().send_event(InputEvent::FocusLost {
                        win: window.id(),
                    });
                }
            },

            winit_ty::Event::DeviceEvent {
                event:
                    winit_ty::DeviceEvent::Motion {
                        axis,
                        value,
                    },
                ..
            } => {
                match axis {
                    0 => {
                        basalt.input_ref().send_event(InputEvent::Motion {
                            x: -value as f32,
                            y: 0.0,
                        });
                    },
                    1 => {
                        basalt.input_ref().send_event(InputEvent::Motion {
                            x: 0.0,
                            y: -value as f32,
                        });
                    },
                    #[cfg(not(target_os = "windows"))]
                    2 => {
                        basalt.input_ref().send_event(InputEvent::Scroll {
                            win: window.id(),
                            v: 0.0,
                            h: value as f32,
                        });
                    },
                    #[cfg(not(target_os = "windows"))]
                    3 => {
                        basalt.input_ref().send_event(InputEvent::Scroll {
                            win: window.id(),
                            v: value as f32,
                            h: 0.0,
                        });
                    },
                    _ => return,
                }
            },

            winit_ty::Event::WindowEvent {
                event: winit_ty::WindowEvent::ReceivedCharacter(c),
                ..
            } => {
                basalt.input_ref().send_event(InputEvent::Character {
                    win: window.id(),
                    c,
                });
            },

            _ => (),
        }

        if basalt.wants_exit() {
            *control_flow = winit_ty::ControlFlow::Exit;
        }
    });
}
