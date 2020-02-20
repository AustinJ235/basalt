use super::BasaltWindow;
use input::{Event, MouseButton, Qwery};
use interface::hook::{InputEvent, ScrollProps};
use parking_lot::{Condvar, Mutex};
use std::{
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    thread,
};
use vulkano::{instance::Instance, swapchain::Surface};
#[cfg(target_os = "unix")]
use winit::platform::unix::EventLoopExtUnix;
#[cfg(target_os = "windows")]
use winit::platform::windows::EventLoopExtWindows;
#[cfg(target_os = "windows")]
use winit::platform::windows::WindowExtWindows;
use Basalt;
use Options as BasaltOptions;

mod winit_ty {
    pub use winit::{
        dpi::PhysicalSize,
        event::{
            DeviceEvent,
            ElementState,
            Event,
            KeyboardInput,
            MouseButton,
            MouseScrollDelta,
            WindowEvent,
        },
        event_loop::{ControlFlow, EventLoop},
        window::{Fullscreen, Window, WindowBuilder},
    };
}

pub struct WinitWindow {
    inner: Arc<winit_ty::Window>,
    basalt: Mutex<Option<Arc<Basalt>>>,
    basalt_ready: Condvar,
    cursor_captured: AtomicBool,
    window_type: Mutex<WindowType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowType {
    X11,
    Wayland,
    Windows,
    Unknown,
}

impl BasaltWindow for WinitWindow {
    fn capture_cursor(&self) {
        self.inner.set_cursor_grab(true).unwrap();
        self.inner.set_cursor_visible(false);
        self.cursor_captured.store(true, atomic::Ordering::SeqCst);
    }

    fn release_cursor(&self) {
        self.inner.set_cursor_grab(false).unwrap();
        self.inner.set_cursor_visible(false);
        self.cursor_captured.store(false, atomic::Ordering::SeqCst);
    }

    fn cursor_captured(&self) -> bool {
        self.cursor_captured.load(atomic::Ordering::SeqCst)
    }

    fn enable_fullscreen(&self) {
        // Going full screen on current monitor
        let current_monitor = self.inner.current_monitor();
        // Get list of all supported modes on this monitor
        let mut video_modes: Vec<_> = current_monitor.video_modes().collect();
        // Bit depth is the most important so we only want the highest
        let max_bit_depth =
            video_modes.iter().max_by_key(|m| m.bit_depth()).unwrap().bit_depth();
        video_modes.retain(|m| m.bit_depth() == max_bit_depth);
        // After selecting bit depth now choose the mode with the highest refresh rate
        let max_refresh_rate =
            video_modes.iter().max_by_key(|m| m.refresh_rate()).unwrap().refresh_rate();
        video_modes.retain(|m| m.refresh_rate() == max_refresh_rate);
        // After refresh the highest resolution is important
        let video_mode = video_modes
            .into_iter()
            .max_by_key(|m| {
                let size = m.size();
                size.width * size.height
            })
            .unwrap();
        // Now actually go fullscreen with the mode we found
        self.inner.set_fullscreen(Some(winit_ty::Fullscreen::Exclusive(video_mode)));
        self.basalt
            .lock()
            .as_ref()
            .expect("Window has been assigned Basalt YET!")
            .input_ref()
            .send_event(Event::FullscreenExclusive(true));
    }

    fn disable_fullscreen(&self) {
        self.inner.set_fullscreen(None);

        self.basalt.lock().as_ref().map(|basalt| {
            basalt.input_ref().send_event(Event::FullscreenExclusive(false));
        });
    }

    fn toggle_fullscreen(&self) {
        if self.inner.fullscreen().is_none() {
            self.enable_fullscreen();
        } else {
            self.disable_fullscreen();
        }
    }

    fn request_resize(&self, width: u32, height: u32) {
        self.inner.set_inner_size(winit_ty::PhysicalSize::new(width as f64, height as f64));
    }

    fn attach_basalt(&self, basalt: Arc<Basalt>) {
        *self.basalt.lock() = Some(basalt);
        self.basalt_ready.notify_one();
    }

    fn inner_dimensions(&self) -> [u32; 2] {
        self.inner.inner_size().into()
    }
}

pub fn open_surface(
    ops: BasaltOptions,
    instance: Arc<Instance>,
) -> Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String> {
    let result = Arc::new(Mutex::new(None));
    let condvar = Arc::new(Condvar::new());
    let result_cp = result.clone();
    let condvar_cp = condvar.clone();

    thread::spawn(move || {
        let event_loop = winit_ty::EventLoop::new_any_thread();

        let inner = match winit_ty::WindowBuilder::new()
            .with_inner_size(winit_ty::PhysicalSize::new(
                ops.window_size[0],
                ops.window_size[1],
            ))
            .with_title(ops.title.clone())
            .build(&event_loop)
        {
            Ok(ok) => Arc::new(ok),
            Err(e) => {
                *result_cp.lock() = Some(Err(format!("Failed to build window: {}", e)));
                condvar_cp.notify_one();
                return;
            },
        };

        let window = Arc::new(WinitWindow {
            inner,
            basalt: Mutex::new(None),
            basalt_ready: Condvar::new(),
            cursor_captured: AtomicBool::new(false),
            window_type: Mutex::new(WindowType::Unknown),
        });

        *result_cp.lock() = Some(
            unsafe {
                #[cfg(target_os = "windows")]
                {
                    *window.window_type.lock() = WindowType::Windows;

                    Surface::from_hwnd(
                        instance,
                        ::std::ptr::null() as *const (), // FIXME
                        window.inner.hwnd(),
                        window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
                    )
                }
                #[cfg(target_os = "linux")]
                {
                    use winit::os::unix::WindowExt;

                    match (window.inner.wayland_display(), window.inner.wayland_surface()) {
                        (Some(display), Some(surface)) => {
                            *window.window_type.lock() = WindowType::Wayland;

                            Surface::from_wayland(
                                instance,
                                display,
                                surface,
                                window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
                            )
                        },
                        _ => {
                            // No wayland display found, check if we can use xlib.
                            // If not, we use xcb.
                            *window.window_type.lock() = WindowType::X11;

                            if instance.loaded_extensions().khr_xlib_surface {
                                Surface::from_xlib(
                                    instance,
                                    window.inner.xlib_display().unwrap(),
                                    window.inner.xlib_window().unwrap() as _,
                                    window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
                                )
                            } else {
                                Surface::from_xcb(
                                    instance,
                                    window.inner.xcb_connection().unwrap(),
                                    window.inner.xlib_window().unwrap() as _,
                                    window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
                                )
                            }
                        },
                    }
                }
            }
            .map_err(|e| format!("{}", e)),
        );

        condvar_cp.notify_one();

        let mut basalt_lk = window.basalt.lock();

        while basalt_lk.is_none() {
            window.basalt_ready.wait(&mut basalt_lk);
        }

        let basalt = basalt_lk.as_ref().unwrap().clone();
        drop(basalt_lk);
        let mut mouse_inside = true;

        match *window.window_type.lock() {
            WindowType::Wayland | WindowType::Windows => {
                basalt.interface_ref().hook_manager.send_event(InputEvent::SetScrollProps(
                    ScrollProps {
                        smooth: true,
                        accel: false,
                        step_mult: 100.0,
                        accel_factor: 5.0,
                    },
                ));
            },
            _ => (),
        }

        event_loop.run(move |event: winit_ty::Event<()>, _, control_flow| {
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
                            position,
                            ..
                        },
                    ..
                } => {
                    basalt
                        .input_ref()
                        .send_event(Event::MousePosition(position.x as f32, position.y as f32));
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
                    basalt.input_ref().send_event(match state {
                        winit_ty::ElementState::Pressed => {
                            Event::KeyPress(Qwery::from(scancode))
                        },
                        winit_ty::ElementState::Released => {
                            Event::KeyRelease(Qwery::from(scancode))
                        },
                    });
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
                    basalt.input_ref().send_event(match state {
                        winit_ty::ElementState::Pressed => {
                            match button {
                                winit_ty::MouseButton::Left => {
                                    Event::MousePress(MouseButton::Left)
                                },
                                winit_ty::MouseButton::Right => {
                                    Event::MousePress(MouseButton::Right)
                                },
                                winit_ty::MouseButton::Middle => {
                                    Event::MousePress(MouseButton::Middle)
                                },
                                _ => return,
                            }
                        },
                        winit_ty::ElementState::Released => {
                            match button {
                                winit_ty::MouseButton::Left => {
                                    Event::MouseRelease(MouseButton::Left)
                                },
                                winit_ty::MouseButton::Right => {
                                    Event::MouseRelease(MouseButton::Right)
                                },
                                winit_ty::MouseButton::Middle => {
                                    Event::MouseRelease(MouseButton::Middle)
                                },
                                _ => return,
                            }
                        },
                    });
                },

                winit_ty::Event::WindowEvent {
                    event:
                        winit_ty::WindowEvent::MouseWheel {
                            delta,
                            ..
                        },
                    ..
                } => {
                    if mouse_inside {
                        basalt.input_ref().send_event(match *window.window_type.lock() {
                            WindowType::Wayland | WindowType::Windows => {
                                match delta {
                                    winit_ty::MouseScrollDelta::PixelDelta(
                                        logical_position,
                                    ) => Event::MouseScroll(-logical_position.y as f32),
                                    winit_ty::MouseScrollDelta::LineDelta(_, y) => {
                                        Event::MouseScroll(-y as f32)
                                    },
                                }
                            },
                            _ => return,
                        });
                    }
                },

                winit_ty::Event::WindowEvent {
                    event:
                        winit_ty::WindowEvent::CursorEntered {
                            ..
                        },
                    ..
                } => {
                    mouse_inside = true;
                    basalt.input_ref().send_event(Event::MouseEnter);
                },

                winit_ty::Event::WindowEvent {
                    event:
                        winit_ty::WindowEvent::CursorLeft {
                            ..
                        },
                    ..
                } => {
                    mouse_inside = false;
                    basalt.input_ref().send_event(Event::MouseLeave);
                },

                winit_ty::Event::WindowEvent {
                    event: winit_ty::WindowEvent::Resized(physical_size),
                    ..
                } => {
                    basalt.input_ref().send_event(Event::WindowResize(
                        physical_size.width,
                        physical_size.height,
                    ));
                },

                winit_ty::Event::RedrawRequested(_) => {
                    basalt.input_ref().send_event(Event::WindowRedraw);
                },

                winit_ty::Event::WindowEvent {
                    event:
                        winit_ty::WindowEvent::ScaleFactorChanged {
                            ..
                        },
                    ..
                } => {
                    basalt.input_ref().send_event(Event::WindowScale);
                },

                winit_ty::Event::WindowEvent {
                    event: winit_ty::WindowEvent::Focused(focused),
                    ..
                } => {
                    basalt.input_ref().send_event(match focused {
                        true => Event::WindowFocused,
                        false => Event::WindowLostFocus,
                    });
                },

                winit_ty::Event::DeviceEvent {
                    event:
                        winit_ty::DeviceEvent::Motion {
                            axis,
                            value,
                        },
                    ..
                } => {
                    basalt.input_ref().send_event(match axis {
                        0 => Event::MouseMotion(-value as f32, 0.0),
                        1 => Event::MouseMotion(0.0, -value as f32),

                        #[cfg(not(target_os = "windows"))]
                        3 => {
                            if mouse_inside {
                                Event::MouseScroll(value as f32)
                            } else {
                                return;
                            }
                        },

                        _ => return,
                    });
                },

                _ => (),
            }
        });
    });

    let mut result_lk = result.lock();

    while result_lk.is_none() {
        condvar.wait(&mut result_lk);
    }

    result_lk.take().unwrap()
}
