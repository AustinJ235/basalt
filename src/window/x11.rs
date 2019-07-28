use std::sync::Arc;
use vulkano::swapchain::Surface;
use super::BasaltWindow;
use ::Options as BasaltOptions;
use vulkano::instance::Instance;
use parking_lot::{Mutex,Condvar};
use Basalt;
use std::thread;
use std::mem::transmute;
use bindings::xinput2 as xi;
use input::qwery::*;
use input::{Event,MouseButton};
use std::os::raw::c_char;
use std::slice;
use std::sync::atomic::{self,AtomicPtr};

pub struct X11Window {
	display: AtomicPtr<xi::Display>,
	window: xi::Window,
	basalt: Mutex<Option<Arc<Basalt>>>,
	basalt_ready: Condvar,
}

impl BasaltWindow for X11Window {
	fn capture_cursor(&self) {
		println!("capture_cursor() not implemented!");
	}
	
	fn release_cursor(&self) {
		println!("release_cursor() not implemented!");
	}
	
	fn enable_fullscreen(&self) {
		println!("enable_fullscreen() not implemented!");
	}
	
	fn disable_fullscreen(&self) {
		println!("disable_fullscreen() not implemented!");
	}
	
	fn request_resize(&self, _width: u32, _height: u32) {
		println!("request_resize() not implemented!");
	}
	
	fn attach_basalt(&self, basalt: Arc<Basalt>) {
		*self.basalt.lock() = Some(basalt);
		self.basalt_ready.notify_one();
	}
}

pub fn open_surface(ops: BasaltOptions, instance: Arc<Instance>) -> Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String> {
	unsafe {
		let display = match xi::XOpenDisplay(::std::ptr::null_mut()) {
			e if e.is_null() => panic!("unable to open x display"),
			d => d
		};
		
		let root_window = match xi::XDefaultRootWindow(display) {
			w if w == 0 => panic!("unable to open root window"),
			w => w
		};
		
		let screen = xi::XDefaultScreen(display);
		let white = xi::XWhitePixel(display, screen);
		let black = xi::XBlackPixel(display, screen);

		let window = xi::XCreateSimpleWindow(
			display, root_window, 0, 0, ops.window_size[0],
			ops.window_size[1], 0, white, black);
				
		let x11window = Arc::new(X11Window {
			display: AtomicPtr::new(display),
			window,
			basalt: Mutex::new(None),
			basalt_ready: Condvar::new()
		});
				
		let surface = Surface::from_xlib(
			instance,
			display,
			window,
			x11window.clone() as Arc<dyn BasaltWindow + Send + Sync>,
		).map_err(|e| format!("failed to create surface: {}", e))?;
		
		println!("got here");
		
		thread::spawn(move || {
			let display = x11window.display.load(atomic::Ordering::Relaxed);
			let window = x11window.window;
			let mut basalt_lk = x11window.basalt.lock();
			
			while basalt_lk.is_none() {
				x11window.basalt_ready.wait(&mut basalt_lk);
			}
			
			let basalt = basalt_lk.take().unwrap();
			drop(basalt_lk);
		
			let mut opcode = 0;
			let mut first_ev_id = 0;
			let mut first_er_id = 0;
			
			if xi::XQueryExtension(
				display,
				b"XInputExtension\0".as_ptr() as *const c_char,
				&mut opcode, &mut first_ev_id, &mut first_er_id
			) != 1 {
				panic!("Failed to query for X Extension");
			}
			
			let mut xi_ver_major = xi::XI_2_Major as i32;
			let mut xi_ver_minor = xi::XI_2_Minor as i32;
			
			match xi::XIQueryVersion(
				display,
				&mut xi_ver_major,
				&mut xi_ver_minor,
			) as u32 {
				xi::Success => (),
				xi::BadRequest => panic!("Unsupported Xinput Version {}.{}. Expected {}.{}",
					xi_ver_major, xi_ver_minor, xi::XI_2_Major, xi::XI_2_Minor),
				_ => panic!("Failed to query extention version.")
			}
			
			let mut mask = xi::XI_RawKeyPressMask
				| xi::XI_RawKeyReleaseMask
				| xi::XI_RawButtonPressMask
				| xi::XI_RawButtonReleaseMask
				| xi::XI_RawMotionMask
				| xi::XI_MotionMask
				| xi::XI_EnterMask
				| xi::XI_LeaveMask
				| xi::XI_FocusInMask
				| xi::XI_FocusOutMask;
				
			let mut mask = xi::XIEventMask {
				deviceid: xi::XIAllDevices as i32,
				mask_len: 4,
				mask: transmute(&mut mask),
			};
			
			match xi::XISelectEvents(display, window, &mut mask, 1) as u32 {
				xi::Success => (),
				e => panic!("XISelectEvents Error: {}", e)
			}
			
			let mut event: xi::_XEvent = ::std::mem::uninitialized();
			let mut window_w = 0;
			let mut window_h = 0;
			
			loop {
				match xi::XNextEvent(transmute(display), transmute(&mut event)) {
					0 => (),
					e => panic!("native input: XNextEvent failed with: {}", e)
				}
				
				match event.type_ as u32 {
					xi::GenericEvent => {
						let cookie: &mut xi::XGenericEventCookie = transmute(&mut event);
						xi::XGetEventData(display, cookie);
						
						match cookie.evtype as u32 {
							xi::XI_RawKeyPress => {
								let ev: &mut xi::XIRawEvent = transmute(cookie.data);
								let keycode = ev.detail - 8;
								
								if keycode < 1 {
									continue;
								}
								
								let key = Qwery::from(keycode as u32);
								basalt.input_ref().send_event(Event::KeyPress(key));
							},
							
							xi::XI_RawKeyRelease => {
								let ev: &mut xi::XIRawEvent = transmute(cookie.data);
								let keycode = ev.detail - 8;
								
								if keycode < 1 {
									continue;
								}
								
								let key = Qwery::from(keycode as u32);
								basalt.input_ref().send_event(Event::KeyRelease(key));
							},
							
							xi::XI_RawButtonPress => {
								let ev: &mut xi::XIRawEvent = transmute(cookie.data);
								let button = match ev.detail {
									1 => MouseButton::Left,
									2 => MouseButton::Middle,
									3 => MouseButton::Right,
									o => MouseButton::Other(o as u8),
								};
								
								basalt.input_ref().send_event(Event::MousePress(button));
							},
							
							xi::XI_RawButtonRelease => {
								let ev: &mut xi::XIRawEvent = transmute(cookie.data);
								let button = match ev.detail {
									1 => MouseButton::Left,
									2 => MouseButton::Middle,
									3 => MouseButton::Right,
									o => MouseButton::Other(o as u8),
								};
								
								basalt.input_ref().send_event(Event::MouseRelease(button));
							},
							
							xi::XI_RawMotion => {
								let ev: &mut xi::XIRawEvent = transmute(cookie.data);
								let mask = slice::from_raw_parts(ev.valuators.mask, ev.valuators.mask_len as usize);
								let mut value = ev.raw_values;
								
								let mut x = 0.0;
								let mut y = 0.0;
								let mut scroll_y = 0.0;
								
								for i in 0..(ev.valuators.mask_len * 8) {
									if (mask[(i >> 3) as usize] & (1 << (i & 7))) != 0 {
										match i {
											0 => x += *value,
											1 => y += *value,
											3 => scroll_y += *value,
											_ => ()
										}
										
										value = value.offset(1);
									}
								}
								
								if x != 0.0 || y != 0.0 {
									basalt.input_ref().send_event(Event::MouseMotion(x as f32, y as f32));
								}
								
								if scroll_y != 0.0 {
									basalt.input_ref().send_event(Event::MouseScroll(scroll_y as f32));
								}
							},
							
							xi::XI_Motion => {
								let ev: &mut xi::XIDeviceEvent = transmute(cookie.data);
								basalt.input_ref().send_event(Event::MousePosition(ev.event_x as f32, ev.event_y as f32));
							},
							
							xi::XI_Enter => {
								basalt.input_ref().send_event(Event::MouseEnter);
							},
							
							xi::XI_Leave => {
								basalt.input_ref().send_event(Event::MouseLeave);
							},
							
							xi::XI_FocusIn => {
								basalt.input_ref().send_event(Event::WindowFocused);
							},
							
							xi::XI_FocusOut => {
								basalt.input_ref().send_event(Event::WindowLostFocus);
							},
								
							_ => ()
						}
						
						xi::XFreeEventData(display, cookie);
					},
					
					xi::ConfigureNotify => {
						let ev: &mut xi::XConfigureEvent = transmute(&mut event);
						
						if ev.width != window_w || ev.height != window_h {
							window_w = ev.width;
							window_h = ev.height;
							basalt.input_ref().send_event(Event::WindowResized);
						}
					},
					
					xi::ClientMessage => {
						let ev: &mut xi::XClientMessageEvent = transmute(&mut event);
						
						if ev.message_type == 307 {
							basalt.exit();
						}
					},
					
					//e => println!("{}", e)
					_ => ()
				}
			}
		});
		
		Ok(surface)
	}
}
