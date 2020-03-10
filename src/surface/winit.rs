use super::{
    BackendRequest,
    BstSurface,
    BstSurfaceBuilder,
    BstSurfaceCaps,
    SurfaceBackend,
    SurfaceRequest,
    WindowType,
};

use crate::Basalt;
use crossbeam::queue::SegQueue;
use input::{Event, MouseButton, Qwery};
use std::{
    borrow::Borrow,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
};
use vulkano::{
    instance::Instance,
    swapchain::{Surface, SurfaceCreationError},
};

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

pub(crate) struct WinitBackend {
    instance: Arc<Instance>,
    event_loop: winit_ty::EventLoop<()>,
    surfaces: Vec<(Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, Arc<WinitSurface>)>,
    backend_req_queue: Arc<SegQueue<BackendRequest>>,
}

pub(crate) struct WinitSurface {
    inner: winit_ty::Window,
    window_ty: WindowType,
    mouse_inside: AtomicBool,
}

impl WinitBackend {
    pub fn new(
        instance: Arc<Instance>,
        backend_req_queue: Arc<SegQueue<BackendRequest>>,
    ) -> Box<dyn SurfaceBackend> {
        Box::new(WinitBackend {
            instance,
            event_loop: winit_ty::EventLoop::new(),
            surfaces: Vec::new(),
            backend_req_queue,
        })
    }
}

impl SurfaceBackend for WinitBackend {
    fn create_surface(
        &mut self,
        builder: BstSurfaceBuilder,
    ) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, String> {
        let winit_window = winit_ty::WindowBuilder::new()
            .with_inner_size(winit_ty::PhysicalSize::<u32>::from(builder.size))
            .with_title(builder.title)
            .build(&self.event_loop)
            .map_err(|e| format!("Failed to create window: {:?}", e))?;

        let winit_surface = WinitSurface {
            inner: winit_window,
            window_ty: WindowType::Unknown,
            mouse_inside: AtomicBool::new(false),
        };

        let (winit_surface, bst_surface) = unsafe {
            winit_to_surface(self.instance.clone(), winit_surface)
                .map_err(|e| format!("Failed to create surface from window: {}", e))
        }?;

        self.surfaces.push((bst_surface.clone(), winit_surface));
        Ok(bst_surface)
    }

    fn run(self: Box<Self>, basalt: Arc<Basalt>) {
        let surfaces = self.surfaces;

        self.event_loop.run(move |event: winit_ty::Event<()>, _, control_flow| {
            *control_flow = winit_ty::ControlFlow::Wait;

            match event {
                winit_ty::Event::WindowEvent {
                    window_id,
                    event,
                } => {
                    let window = match surfaces.iter().find(|s| s.1.inner.id() == window_id) {
                        Some(some) => some.1.clone(),
                        None => return,
                    };

                    match event {
                        winit_ty::WindowEvent::CloseRequested => {
                            basalt.exit();
                            *control_flow = winit_ty::ControlFlow::Exit;
                        },

                        winit_ty::WindowEvent::CursorMoved {
                            position,
                            ..
                        } => {
                            basalt.input_ref().send_event(Event::MousePosition(
                                position.x as f32,
                                position.y as f32,
                            ))
                        },

                        winit_ty::WindowEvent::KeyboardInput {
                            input:
                                winit_ty::KeyboardInput {
                                    scancode,
                                    state,
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

                        winit_ty::WindowEvent::MouseInput {
                            state,
                            button,
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

                        winit_ty::WindowEvent::MouseWheel {
                            delta,
                            ..
                        } => {
                            if window.mouse_inside.load(atomic::Ordering::SeqCst) {
                                basalt.input_ref().send_event(match window.window_ty {
                                    WindowType::UnixWayland | WindowType::Windows => {
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

                        winit_ty::WindowEvent::CursorEntered {
                            ..
                        } => {
                            window.mouse_inside.store(true, atomic::Ordering::SeqCst);
                            basalt.input_ref().send_event(Event::MouseEnter);
                        },

                        winit_ty::WindowEvent::CursorLeft {
                            ..
                        } => {
                            window.mouse_inside.store(false, atomic::Ordering::SeqCst);
                            basalt.input_ref().send_event(Event::MouseLeave);
                        },

                        winit_ty::WindowEvent::Resized(physical_size) => {
                            basalt.input_ref().send_event(Event::WindowResize(
                                physical_size.width,
                                physical_size.height,
                            ));
                        },

                        winit_ty::WindowEvent::ScaleFactorChanged {
                            ..
                        } => {
                            basalt.input_ref().send_event(Event::WindowScale);
                        },

                        winit_ty::WindowEvent::Focused(focused) => {
                            basalt.input_ref().send_event(match focused {
                                true => Event::WindowFocused,
                                false => Event::WindowLostFocus,
                            });
                        },

                        _ => (),
                    }
                },

                winit_ty::Event::RedrawRequested(window_id) => {
                    let _window = match surfaces.iter().find(|s| s.1.inner.id() == window_id) {
                        Some(some) => some.1.clone(),
                        None => return,
                    };

                    basalt.input_ref().send_event(Event::WindowRedraw);
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
    }
}

impl BstSurface for WinitSurface {
    fn capture_cursor(&self) {
        unimplemented!()
    }

    fn release_cursor(&self) {
        unimplemented!()
    }

    fn is_cursor_captured(&self) -> bool {
        unimplemented!()
    }

    fn enable_fullscreen(&self) {
        unimplemented!()
    }

    fn enable_exclusive_fullscreen(&self) {
        unimplemented!()
    }

    fn disable_fullscreen(&self) {
        unimplemented!()
    }

    fn toggle_fullscreen(&self) {
        unimplemented!()
    }

    fn toggle_exclusive_fullscreen(&self) {
        unimplemented!()
    }

    fn is_fullscreen_active(&self) -> bool {
        unimplemented!()
    }

    fn is_exclusive_fullscreen_active(&self) -> bool {
        unimplemented!()
    }

    fn enable_fullscreen_prefer_exclusive(&self) {
        unimplemented!()
    }

    fn toggle_fullscreen_prefer_exclusive(&self) {
        unimplemented!()
    }

    fn capabilities(&self) -> BstSurfaceCaps {
        unimplemented!()
    }

    fn window_type(&self) -> WindowType {
        self.window_ty.clone()
    }
}

#[cfg(target_os = "android")]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    mut winit_surface: WinitSurface,
) -> Result<
    (Arc<WinitSurface>, Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>),
    SurfaceCreationError,
> {
    use winit::platform::android::WindowExtAndroid;

    winit_surface.window_ty = WindowType::Android;
    let winit_surface = Arc::new(winit_surface);

    Ok((
        winit_surface.clone(),
        Surface::from_anativewindow(
            instance,
            winit_surface.inner.borrow().native_window(),
            winit_surface as Arc<dyn BstSurface + Send + Sync>,
        )?,
    ))
}

#[cfg(all(unix, not(target_os = "android"), not(target_os = "macos")))]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    mut winit_surface: WinitSurface,
) -> Result<
    (Arc<WinitSurface>, Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>),
    SurfaceCreationError,
> {
    use winit::platform::unix::WindowExtUnix;

    match (
        winit_surface.inner.borrow().wayland_display(),
        winit_surface.borrow().wayland_surface(),
    ) {
        (Some(display), Some(surface)) => {
            winit_surface.window_ty = WindowType::UnixWayland;
            let winit_surface = Arc::new(winit_surface);

            Ok((
                winit_surface.clone(),
                Surface::from_wayland(
                    instance,
                    display,
                    surface,
                    winit_surface as Arc<dyn BstSurface + Send + Sync>,
                )?,
            ))
        },
        _ => {
            if instance.loaded_extensions().khr_xlib_surface {
                winit_surface.window_ty = WindowType::UnixXLib;
                let winit_surface = Arc::new(winit_surface);

                Ok((
                    winit_surface.clone(),
                    Surface::from_xlib(
                        instance,
                        winit_surface.inner.borrow().xlib_display().unwrap(),
                        winit_surface.inner.borrow().xlib_window().unwrap() as _,
                        winit_surface as Arc<dyn BstSurface + Send + Sync>,
                    )?,
                ))
            } else {
                winit_surface.window_ty = WindowType::UnixXCB;
                let winit_surface = Arc::new(winit_surface);

                Ok((
                    winit_surface.clone(),
                    Surface::from_xcb(
                        instance,
                        winit_surface.inner.borrow().xcb_connection().unwrap(),
                        winit_surface.inner.borrow().xlib_window().unwrap() as _,
                        winit_surface as Arc<dyn BstSurface + Send + Sync>,
                    )?,
                ))
            }
        },
    }
}

#[cfg(target_os = "windows")]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    mut winit_surface: WinitSurface,
) -> Result<
    (Arc<WinitSurface>, Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>),
    SurfaceCreationError,
> {
    use winit::platform::windows::WindowExtWindows;

    winit_surface.window_ty = WindowType::Windows;
    let winit_surface = Arc::new(winit_surface);

    Ok((
        winit_surface.clone(),
        Surface::from_hwnd(
            instance,
            ::std::ptr::null() as *const (), // FIXME
            winit_surface.inner.borrow().hwnd(),
            winit_surface as Arc<dyn BstSurface + Send + Sync>,
        )?,
    ))
}

#[cfg(target_os = "macos")]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    mut win: WinitSurface,
) -> Result<
    (Arc<WinitSurface>, Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>),
    SurfaceCreationError,
> {
    use winit::platform::macos::WindowExtMacOS;

    winit_surface.window_ty = WindowType::MacOS;
    let winit_surface = Arc::new(winit_surface);

    let wnd: cocoa_id = ::std::mem::transmute(winit_surface.inner.borrow().ns_window());
    let layer = CoreAnimationLayer::new();

    layer.set_edge_antialiasing_mask(0);
    layer.set_presents_with_transaction(false);
    layer.remove_all_animations();

    let view = wnd.contentView();

    layer.set_contents_scale(view.backingScaleFactor());
    view.setLayer(mem::transmute(layer.as_ref())); // Bombs here with out of memory
    view.setWantsLayer(YES);

    (
        winit_surface.clone(),
        Surface::from_macos_moltenvk(
            instance,
            winit_surface.innner.borrow().ns_view() as *const (),
            winit_surface,
        ),
    )
}
