use std::sync::Arc;
use vulkano::swapchain::Surface;
use super::BasaltWindow;
use ::Options as BasaltOptions;
use std::thread;
use parking_lot::Mutex;
use parking_lot::Condvar;
use vulkano::instance::Instance;
use winit;
use Basalt;
use input::{Event,MouseButton,Qwery};
use winit::WindowEvent;
use winit::DeviceEvent;
use std::sync::atomic::{self,AtomicBool};

pub struct WinitWindow {
	inner: Arc<winit::Window>,
	basalt: Mutex<Option<Arc<Basalt>>>,
	basalt_ready: Condvar,
	cursor_captured: AtomicBool,
}

impl BasaltWindow for WinitWindow {
	fn capture_cursor(&self) {
		self.inner.hide_cursor(true);
		self.inner.grab_cursor(true).unwrap();
		self.cursor_captured.store(true, atomic::Ordering::SeqCst);
	}
	
	fn release_cursor(&self) {
		self.inner.hide_cursor(false);
		self.inner.grab_cursor(false).unwrap();
		self.cursor_captured.store(false, atomic::Ordering::SeqCst);
	}
	
	fn cursor_captured(&self) -> bool {
		self.cursor_captured.load(atomic::Ordering::SeqCst)
	}
	
	fn enable_fullscreen(&self) {
		self.inner.set_fullscreen(Some(self.inner.get_current_monitor()));
	}
	
	fn disable_fullscreen(&self) {
		self.inner.set_fullscreen(None);
	}
	
	fn request_resize(&self, width: u32, height: u32) {
		self.inner.set_inner_size(winit::dpi::LogicalSize::new(width as f64, height as f64));
	}
	
	fn attach_basalt(&self, basalt: Arc<Basalt>) {
		*self.basalt.lock() = Some(basalt);
		self.basalt_ready.notify_one();
	}
	
	fn inner_dimensions(&self) -> [u32; 2] {
		let (x, y) = self.inner.get_inner_size().unwrap().to_physical(self.inner.get_hidpi_factor()).into();
		[x, y]
	}
}

pub fn open_surface(ops: BasaltOptions, instance: Arc<Instance>) -> Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String> {
	let result = Arc::new(Mutex::new(None));
	let condvar = Arc::new(Condvar::new());
	let result_cp = result.clone();
	let condvar_cp = condvar.clone();
	
	thread::spawn(move || {
		let mut events_loop = winit::EventsLoop::new();
	
		let inner = match winit::WindowBuilder::new()
			.with_dimensions((ops.window_size[0], ops.window_size[1]).into())
			.with_title(ops.title.clone())
			.build(&events_loop)
		{
			Ok(ok) => Arc::new(ok),
			Err(e) => {
				*result_cp.lock() = Some(Err(format!("Failed to build window: {}", e)));
				condvar_cp.notify_one();
				return;
			}
		};
		
		let window = Arc::new(WinitWindow {
			inner: inner,
			basalt: Mutex::new(None),
			basalt_ready: Condvar::new(),
			cursor_captured: AtomicBool::new(false),
		});
		
		*result_cp.lock() = Some(unsafe {
			#[cfg(target_os = "windows")]
			{
				use winit::os::windows::WindowExt;
				
				Surface::from_hwnd(
					instance,
					::std::ptr::null() as *const (), // FIXME
					window.inner.get_hwnd(),
					window.clone() as Arc<dyn BasaltWindow + Send + Sync>
				)
			}
			#[cfg(target_os = "linux")]
			{
				use winit::os::unix::WindowExt;
				
				match (
					window.inner.get_wayland_display(),
					window.inner.get_wayland_surface(),
				) {
					(Some(display), Some(surface)) => Surface::from_wayland(
						instance,
						display,
						surface,
						window.clone() as Arc<dyn BasaltWindow + Send + Sync>
					), _ => {
						// No wayland display found, check if we can use xlib.
						// If not, we use xcb.
						if instance.loaded_extensions().khr_xlib_surface {
							Surface::from_xlib(
								instance,
								window.inner.get_xlib_display().unwrap(),
								window.inner.get_xlib_window().unwrap() as _,
								window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
							)
						} else {
							Surface::from_xcb(
								instance,
								window.inner.get_xcb_connection().unwrap(),
								window.inner.get_xlib_window().unwrap() as _,
								window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
							)
						}
					},
				}
			}
		}.map_err(|e| format!("{}", e))); condvar_cp.notify_one();
		
		let mut basalt_lk = window.basalt.lock();
		
		while basalt_lk.is_none() {
			window.basalt_ready.wait(&mut basalt_lk);
		}
		
		let basalt = basalt_lk.take().unwrap();
		drop(basalt_lk);
		let mut mouse_inside = true;
	
		events_loop.run_forever(|ev| {
			match ev {
				winit::Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
					basalt.exit();
					return winit::ControlFlow::Break;
				},
				
				winit::Event::WindowEvent { event: WindowEvent::CursorMoved { position, .. }, .. } => {
					let winit::dpi::PhysicalPosition { x, y }
						= position.to_physical(window.inner.get_hidpi_factor());
					basalt.input_ref().send_event(Event::MousePosition(x as f32, y as f32));
				},
				
				winit::Event::WindowEvent { event: WindowEvent::KeyboardInput { input, .. }, .. } => {
					basalt.input_ref().send_event(match input.state {
						winit::ElementState::Pressed => Event::KeyPress(Qwery::from(input.scancode)),
						winit::ElementState::Released => Event::KeyRelease(Qwery::from(input.scancode)),
					});
				},
				
				winit::Event::WindowEvent { event: WindowEvent::MouseInput { state, button, .. }, .. } => {
					let button = match button {
						winit::MouseButton::Left => MouseButton::Left,
						winit::MouseButton::Right => MouseButton::Right,
						winit::MouseButton::Middle => MouseButton::Middle,
						_ => return winit::ControlFlow::Continue
					};
				
					basalt.input_ref().send_event(match state {
						winit::ElementState::Pressed => Event::MousePress(button),
						winit::ElementState::Released => Event::MouseRelease(button),
					});
				},
				
				#[cfg(target_os = "windows")]
				winit::Event::WindowEvent { event: WindowEvent::MouseWheel { delta, .. }, .. } => {
					if mouse_inside {
						basalt.input_ref().send_event(match delta {
							winit::MouseScrollDelta::LineDelta(_, y) => {
								Event::MouseScroll(-y)
							}, winit::MouseScrollDelta::PixelDelta(data) => {
								println!("WARNING winit::MouseScrollDelta::PixelDelta is untested!");
								Event::MouseScroll(data.y as f32)
							}
						});
					}
				},
				
				winit::Event::WindowEvent { event: WindowEvent::CursorEntered { .. }, .. } => {
					mouse_inside = true;
					basalt.input_ref().send_event(Event::MouseEnter);
				},
				
				winit::Event::WindowEvent { event: WindowEvent::CursorLeft { .. }, .. } => {
					mouse_inside = false;
					basalt.input_ref().send_event(Event::MouseLeave);
				},
				
				winit::Event::WindowEvent { event: WindowEvent::Resized { .. }, .. } => {
					basalt.input_ref().send_event(Event::WindowResized);
				},
				
				winit::Event::WindowEvent { event: WindowEvent::HiDpiFactorChanged(_), .. } => {
					basalt.input_ref().send_event(Event::WindowResized);
				},
				
				winit::Event::WindowEvent { event: WindowEvent::Focused(focused), .. } => {
					basalt.input_ref().send_event(match focused {
						true => Event::WindowFocused,
						false => Event::WindowLostFocus
					});
				},
				
				winit::Event::DeviceEvent { event: DeviceEvent::Motion { axis, value }, .. } => {
					basalt.input_ref().send_event(match axis {
						0 => Event::MouseMotion(-value as f32, 0.0),
						1 => Event::MouseMotion(0.0, -value as f32),
						
						#[cfg(not(target_os = "windows"))]
						3 => if mouse_inside {
							Event::MouseScroll(value as f32)
						} else {
							return winit::ControlFlow::Continue;
						},
						
						_ => return winit::ControlFlow::Continue
					});
				},
				
				_ => ()
			}
		
			winit::ControlFlow::Continue
		});
	});
	
	let mut result_lk = result.lock();
	
	while result_lk.is_none() {
		condvar.wait(&mut result_lk);
	}
	
	result_lk.take().unwrap()
}

