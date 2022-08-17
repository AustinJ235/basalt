use super::{BasaltWindow, WindowType};
use crate::input::{Event, MouseButton, Qwerty};
use crate::interface::hook::{BinHookEvent, ScrollProps};
use crate::{Basalt, BstOptions};
use parking_lot::{Condvar, Mutex};
use raw_window_handle::{
	HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::ops::Deref;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use vulkano::instance::Instance;
use vulkano::swapchain::{Surface, Win32Monitor};

mod winit_ty {
	pub use winit::dpi::PhysicalSize;
	pub use winit::event::{
		DeviceEvent, ElementState, Event, KeyboardInput, MouseButton, MouseScrollDelta,
		WindowEvent,
	};
	pub use winit::event_loop::{ControlFlow, EventLoop};
	pub use winit::window::{CursorGrabMode, Fullscreen, Window, WindowBuilder};
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
		self.inner.set_cursor_grab(winit_ty::CursorGrabMode::Confined).unwrap();
		self.cursor_captured.store(true, atomic::Ordering::SeqCst);
	}

	fn release_cursor(&self) {
		self.inner.set_cursor_visible(true);
		self.inner.set_cursor_grab(winit_ty::CursorGrabMode::None).unwrap();
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
			let max_refresh_rate = video_modes
				.iter()
				.max_by_key(|m| m.refresh_rate_millihertz())
				.unwrap()
				.refresh_rate_millihertz();
			video_modes.retain(|m| m.refresh_rate_millihertz() == max_refresh_rate);
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

	fn win32_monitor(&self) -> Option<Win32Monitor> {
		#[cfg(target_os = "windows")]
		unsafe {
			use std::ffi::c_void;
			use std::mem::transmute;
			use winit::platform::windows::MonitorHandleExtWindows;

			self.inner
				.current_monitor()
				.map(|m| Win32Monitor::new(transmute::<_, *const c_void>(m.hmonitor())))
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

pub fn open_surface(
	ops: BstOptions,
	instance: Arc<Instance>,
	result_fn: Box<dyn Fn(Result<Arc<Surface<Arc<dyn BasaltWindow>>>, String>) + Send + Sync>,
) {
	let event_loop = winit_ty::EventLoop::new();

	let inner = match winit_ty::WindowBuilder::new()
		.with_inner_size(winit_ty::PhysicalSize::new(ops.window_size[0], ops.window_size[1]))
		.with_title(ops.title)
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

	match unsafe {
		match window.inner.raw_window_handle() {
			RawWindowHandle::Win32(handle) =>
				match Surface::from_win32(
					instance,
					handle.hinstance,
					handle.hwnd,
					window.clone() as Arc<dyn BasaltWindow>,
				) {
					Ok(ok) => Ok((WindowType::Windows, ok)),
					Err(e) => Err(format!("Failed to create win32 surface: {}", e)),
				},
			RawWindowHandle::Wayland(handle) =>
				match window.inner.raw_display_handle() {
					RawDisplayHandle::Wayland(display) =>
						match Surface::from_wayland(
							instance,
							display.display,
							handle.surface,
							window.clone() as Arc<dyn BasaltWindow>,
						) {
							Ok(ok) => Ok((WindowType::UnixWayland, ok)),
							Err(e) => Err(format!("Failed to create wayland surface: {}", e)),
						},
					_ =>
						Err(String::from(
							"Failed to create wayland surface: invalid display handle",
						)),
				},
			RawWindowHandle::Xlib(handle) =>
				match window.inner.raw_display_handle() {
					RawDisplayHandle::Xlib(display) =>
						match Surface::from_xlib(
							instance,
							display.display,
							handle.window,
							window.clone() as Arc<dyn BasaltWindow>,
						) {
							Ok(ok) => Ok((WindowType::UnixXlib, ok)),
							Err(e) => Err(format!("Failed to create xlib surface: {}", e)),
						},
					_ =>
						Err(String::from(
							"Failed to create xlib surface: invalid display handle",
						)),
				},
			RawWindowHandle::Xcb(handle) =>
				match window.inner.raw_display_handle() {
					RawDisplayHandle::Xcb(display) =>
						match Surface::from_xcb(
							instance,
							display.connection,
							handle.window,
							window.clone() as Arc<dyn BasaltWindow>,
						) {
							Ok(ok) => Ok((WindowType::UnixXCB, ok)),
							Err(e) => Err(format!("Failed to create xcb surface: {}", e)),
						},
					_ =>
						Err(String::from(
							"Failed to create xcb surface: invalid display handle",
						)),
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

					match Surface::from_mac_os(
						instance,
						layer as *const (),
						window.clone() as Arc<dyn BasaltWindow>,
					) {
						Ok(ok) => Ok((WindowType::Macos, ok)),
						Err(e) => Err(format!("Failed to create UiKit surface: {}", e)),
					}
				}
				#[cfg(not(target_os = "macos"))]
				{
					Err(String::from("Failed to crate UiKit surface: target_os != 'macos'"))
				}
			},
			_ => Err(String::from("Failed to create surface: window is not supported")),
		}
	} {
		Ok((window_type, surface)) => {
			*window.window_type.lock() = window_type;
			thread::spawn(move || result_fn(Ok(surface)));
		},
		Err(e) => return result_fn(Err(e)),
	}

	let basalt = {
		let mut lock = window.basalt.lock();
		window.basalt_ready.wait_for(&mut lock, Duration::from_millis(500));
		lock.clone().unwrap()
	};

	let mut mouse_inside = true;
	let window_type = *window.window_type.lock();

	match &window_type {
		WindowType::UnixWayland | WindowType::Windows => {
			basalt.interface_ref().hman().send_event(BinHookEvent::SetScrollProps(
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
					winit_ty::ElementState::Pressed => Event::KeyPress(Qwerty::from(scancode)),
					winit_ty::ElementState::Released =>
						Event::KeyRelease(Qwerty::from(scancode)),
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

			winit_ty::Event::WindowEvent {
				event: winit_ty::WindowEvent::ReceivedCharacter(c),
				..
			} => {
				basalt.input_ref().send_event(Event::Character(c));
			},

			_ => (),
		}

		if basalt.wants_exit() {
			*control_flow = winit_ty::ControlFlow::Exit;
		}
	});
}
