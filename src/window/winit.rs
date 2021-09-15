use super::{BasaltWindow, WindowType};
use crate::input::{Event, MouseButton, Qwery};
use crate::interface::hook::{InputEvent, ScrollProps};
use crate::{Basalt, Options as BasaltOptions};
use parking_lot::{Condvar, Mutex};
use std::ops::Deref;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::thread;
use vulkano::instance::Instance;
use vulkano::swapchain::Surface;

mod winit_ty {
	pub use winit::dpi::PhysicalSize;
	pub use winit::event::{
		DeviceEvent, ElementState, Event, KeyboardInput, MouseButton, MouseScrollDelta,
		WindowEvent,
	};
	pub use winit::event_loop::{ControlFlow, EventLoop};
	pub use winit::window::{Fullscreen, Window, WindowBuilder};
}

pub struct WinitWindow {
	inner: Arc<winit_ty::Window>,
	basalt: Mutex<Option<Arc<Basalt>>>,
	basalt_ready: Condvar,
	cursor_captured: AtomicBool,
	window_type: Mutex<WindowType>,
}

impl BasaltWindow for WinitWindow {
	fn capture_cursor(&self) {
		self.inner.set_cursor_visible(false);
		self.inner.set_cursor_grab(true).unwrap();
		self.cursor_captured.store(true, atomic::Ordering::SeqCst);
	}

	fn release_cursor(&self) {
		self.inner.set_cursor_grab(false).unwrap();
		self.inner.set_cursor_visible(true);
		self.cursor_captured.store(false, atomic::Ordering::SeqCst);
	}

	fn cursor_captured(&self) -> bool {
		self.cursor_captured.load(atomic::Ordering::SeqCst)
	}

	fn enable_fullscreen(&self) {
		let basalt =
			self.basalt.lock().deref().clone().expect("Window doesn't have access to Basalt!");

		if basalt.options_ref().exclusive_fullscreen {
			// Going full screen on current monitor
			let current_monitor = match self.inner.current_monitor() {
				Some(some) => some,
				None => {
					println!(
						"[Basalt]: Unable to go fullscreen: window doesn't have an associated \
						 monitor."
					);
					return;
				},
			};
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
			basalt.input_ref().send_event(Event::FullscreenExclusive(true));
		} else {
			let current_monitor = self.inner.current_monitor();
			self.inner.set_fullscreen(Some(winit_ty::Fullscreen::Borderless(current_monitor)));
		}
	}

	fn disable_fullscreen(&self) {
		self.inner.set_fullscreen(None);
		let basalt =
			self.basalt.lock().deref().clone().expect("Window doesn't have access to Basalt!");

		if basalt.options_ref().exclusive_fullscreen {
			basalt.input_ref().send_event(Event::FullscreenExclusive(false));
		}
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

	fn window_type(&self) -> WindowType {
		*self.window_type.lock()
	}

	fn scale_factor(&self) -> f32 {
		self.inner.scale_factor() as f32
	}
}

pub fn open_surface(
	ops: BasaltOptions,
	instance: Arc<Instance>,
	result_fn: Box<
		dyn Fn(Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String>) + Send + Sync,
	>,
) {
	let event_loop = winit_ty::EventLoop::new();

	let inner = match winit_ty::WindowBuilder::new()
		.with_inner_size(winit_ty::PhysicalSize::new(ops.window_size[0], ops.window_size[1]))
		.with_title(ops.title.clone())
		.build(&event_loop)
	{
		Ok(ok) => Arc::new(ok),
		Err(e) => return result_fn(Err(format!("Failed to build window: {}", e))),
	};

	let window = Arc::new(WinitWindow {
		inner,
		basalt: Mutex::new(None),
		basalt_ready: Condvar::new(),
		cursor_captured: AtomicBool::new(false),
		window_type: Mutex::new(WindowType::NotSupported),
	});

	let surface_result = unsafe {
		#[cfg(target_os = "windows")]
		{
			use winit::platform::windows::WindowExtWindows;
			*window.window_type.lock() = WindowType::Windows;

			Surface::from_hwnd(
				instance,
				::std::ptr::null() as *const (), // FIXME
				window.inner.hwnd(),
				window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
			)
		}

		#[cfg(all(unix, not(target_os = "android"), not(target_os = "macos")))]
		{
			use winit::platform::unix::WindowExtUnix;

			match (window.inner.wayland_display(), window.inner.wayland_surface()) {
				(Some(display), Some(surface)) => {
					*window.window_type.lock() = WindowType::UnixWayland;

					Surface::from_wayland(
						instance,
						display,
						surface,
						window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
					)
				},

				_ =>
					if instance.enabled_extensions().khr_xlib_surface {
						*window.window_type.lock() = WindowType::UnixXlib;

						Surface::from_xlib(
							instance,
							window.inner.xlib_display().unwrap(),
							window.inner.xlib_window().unwrap() as _,
							window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
						)
					} else {
						*window.window_type.lock() = WindowType::UnixXCB;

						Surface::from_xcb(
							instance,
							window.inner.xcb_connection().unwrap(),
							window.inner.xlib_window().unwrap() as _,
							window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
						)
					},
			}
		}

		#[cfg(target_os = "macos")]
		{
			use cocoa::appkit::{NSView, NSWindow};
			use cocoa::base::id as cocoa_id;
			use metal::CoreAnimationLayer;
			use objc::runtime::YES;
			use std::mem;
			use winit::platform::macos::WindowExtMacOS;

			*window.window_type.lock() = WindowType::MacOS;

			let wnd: cocoa_id = mem::transmute(window.inner.borrow().ns_window());
			let layer = CoreAnimationLayer::new();

			layer.set_edge_antialiasing_mask(0);
			layer.set_presents_with_transaction(false);
			layer.remove_all_animations();

			let view = wnd.contentView();

			layer.set_contents_scale(view.backingScaleFactor());
			view.setLayer(mem::transmute(layer.as_ref())); // Bombs here with out of memory
			view.setWantsLayer(YES);

			Surface::from_macos_moltenvk(
				instance,
				window.inner.ns_view() as *const (),
				window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
			)
		}

		#[cfg(target_os = "android")]
		{
			use winit::platform::android::WindowExtAndroid;

			Surface::from_anativewindow(instance, window.inner.native_window(), window)
		}

		#[cfg(all(
			not(target_os = "windows"),
			not(all(unix, not(target_os = "android"), not(target_os = "macos"))),
			not(target_os = "macos"),
			not(target_os = "android")
		))]
		{
			return result_fn(Err(format!(
				"Failed to build surface: platform isn't supported"
			)));
		}
	};

	thread::spawn(move || {
		result_fn(match surface_result {
			Ok(surface) => Ok(surface),
			Err(e) => Err(format!("Failed to build surface: {}", e)),
		});
	});

	let mut basalt_lk = window.basalt.lock();

	while basalt_lk.is_none() {
		window.basalt_ready.wait(&mut basalt_lk);
	}

	let basalt = basalt_lk.as_ref().unwrap().clone();
	drop(basalt_lk);
	let mut mouse_inside = true;
	let window_type = window.window_type.lock().clone();

	match &window_type {
		WindowType::UnixWayland | WindowType::Windows => {
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
				event: winit_ty::WindowEvent::CursorMoved {
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
				#[cfg(target_os = "windows")]
				{
					if scancode == 0 {
						return;
					}
				}

				basalt.input_ref().send_event(match state {
					winit_ty::ElementState::Pressed => Event::KeyPress(Qwery::from(scancode)),
					winit_ty::ElementState::Released =>
						Event::KeyRelease(Qwery::from(scancode)),
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
					winit_ty::ElementState::Pressed =>
						match button {
							winit_ty::MouseButton::Left => Event::MousePress(MouseButton::Left),
							winit_ty::MouseButton::Right =>
								Event::MousePress(MouseButton::Right),
							winit_ty::MouseButton::Middle =>
								Event::MousePress(MouseButton::Middle),
							_ => return,
						},
					winit_ty::ElementState::Released =>
						match button {
							winit_ty::MouseButton::Left =>
								Event::MouseRelease(MouseButton::Left),
							winit_ty::MouseButton::Right =>
								Event::MouseRelease(MouseButton::Right),
							winit_ty::MouseButton::Middle =>
								Event::MouseRelease(MouseButton::Middle),
							_ => return,
						},
				});
			},

			winit_ty::Event::WindowEvent {
				event: winit_ty::WindowEvent::MouseWheel {
					delta,
					..
				},
				..
			} =>
				if mouse_inside {
					basalt.input_ref().send_event(match &window_type {
						WindowType::UnixWayland | WindowType::Windows =>
							match delta {
								winit_ty::MouseScrollDelta::PixelDelta(logical_position) =>
									Event::MouseScroll(-logical_position.y as f32),
								winit_ty::MouseScrollDelta::LineDelta(_, y) =>
									Event::MouseScroll(-y as f32),
							},
						_ => return,
					});
				},

			winit_ty::Event::WindowEvent {
				event: winit_ty::WindowEvent::CursorEntered {
					..
				},
				..
			} => {
				mouse_inside = true;
				basalt.input_ref().send_event(Event::MouseEnter);
			},

			winit_ty::Event::WindowEvent {
				event: winit_ty::WindowEvent::CursorLeft {
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
				basalt
					.input_ref()
					.send_event(Event::WindowResize(physical_size.width, physical_size.height));
			},

			winit_ty::Event::RedrawRequested(_) => {
				basalt.input_ref().send_event(Event::WindowRedraw);
			},

			winit_ty::Event::WindowEvent {
				event: winit_ty::WindowEvent::ScaleFactorChanged {
					..
				},
				..
			} => {
				let scale = window.inner.scale_factor() as f32;
				basalt.input_ref().send_event(Event::WindowScale(scale));
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
				event: winit_ty::DeviceEvent::Motion {
					axis,
					value,
				},
				..
			} => {
				basalt.input_ref().send_event(match axis {
					0 => Event::MouseMotion(-value as f32, 0.0),
					1 => Event::MouseMotion(0.0, -value as f32),

					#[cfg(not(target_os = "windows"))]
					3 =>
						if mouse_inside {
							Event::MouseScroll(value as f32)
						} else {
							return;
						},

					_ => return,
				});
			},

			_ => (),
		}

		if basalt.wants_exit() {
			*control_flow = winit_ty::ControlFlow::Exit;
		}
	});
}
