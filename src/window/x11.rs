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
use std::slice;
use std::sync::atomic::{self,AtomicPtr};
use std::ffi::CString;

#[allow(dead_code)]
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
	
	fn cursor_captured(&self) -> bool {
		println!("cursor_captured() not implemented!");
		false
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
	
	fn inner_dimensions(&self) -> [u32; 2] {
		[0; 2]
	}
}

pub fn open_surface(ops: BasaltOptions, instance: Arc<Instance>) -> Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String> {
	unsafe {
		let display = match xi::XOpenDisplay(::std::ptr::null_mut()) {
			e if e.is_null() => panic!("unable to open x display"),
			d => d
		};
		
		let mut opcode = 0;
		let mut event = 0;
		let mut error = 0;
		let ext_name = CString::new("XInputExtension").unwrap();
		
		if xi::XQueryExtension(display, ext_name.as_ptr(), &mut opcode, &mut event, &mut error) == xi::False as i32 {
			return Err(format!("XQueryExtension failed: opcode {}, event {}, error {}", opcode, event, error));
		}
		
		drop(opcode);
		drop(event);
		drop(error);
		drop(ext_name);
		let mut major = xi::XI_2_Major as i32;
		let mut minor = xi::XI_2_Minor as i32;
		
		match xi::XIQueryVersion(display, &mut major, &mut minor) as u32 {
			xi::Success => (),
			xi::BadRequest => return Err(format!("XIQueryVersion BadRequest")),
			_ => return Err(format!("XIQueryVersion Internal Error"))
		}
		
		drop(major);
		drop(minor);
		
		let root = match xi::XDefaultRootWindow(display) {
			0 => panic!("unable to open root window"),
			r => r
		};
		
		let mut attrs: xi::XSetWindowAttributes = ::std::mem::zeroed();
		attrs.event_mask = (xi::XI_RawKeyPressMask
			| xi::XI_RawKeyReleaseMask
			| xi::XI_RawButtonPressMask
			| xi::XI_RawButtonReleaseMask
			| xi::XI_RawMotionMask
			| xi::XI_MotionMask
			| xi::XI_EnterMask
			| xi::XI_LeaveMask
			| xi::XI_FocusInMask
			| xi::XI_FocusOutMask) as i64;
		
		
		let screen = xi::XDefaultScreen(display);
		let window = xi::XCreateWindow(
			display,
			root,
			0,
			0,
			ops.window_size[0],
			ops.window_size[1],
			0,
			xi::CopyFromParent as i32,
			xi::InputOutput,
			::std::ptr::null_mut(),
			xi::CWBorderPixel as u64 | xi::CWColormap as u64 | xi::CWEventMask as u64,
			&mut attrs
		);
			
		
		/*let window = xi::XCreateSimpleWindow(
			display,
			root,
			0,
			0,
			ops.window_size[0],
			ops.window_size[1],
			0,
			xi::XWhitePixel(display, screen), 
			xi::XBlackPixel(display, screen)
		);*/
		
		drop(screen);
		drop(root);
		
		let mut mask = xi::XIEventMask {
			deviceid: xi::XIAllDevices as i32,
			mask_len: 4,
			mask: transmute(&mut (
				  xi::XI_RawKeyPressMask
				| xi::XI_RawKeyReleaseMask
				| xi::XI_RawButtonPressMask
				| xi::XI_RawButtonReleaseMask
				| xi::XI_RawMotionMask
				| xi::XI_MotionMask
				| xi::XI_EnterMask
				| xi::XI_LeaveMask
				| xi::XI_FocusInMask
				| xi::XI_FocusOutMask
			))
		};
		
		match xi::XISelectEvents(display, window, &mut mask, 1) as u32 {
			xi::Success => (),
			xi::BadValue => return Err(format!("XISelectEvents BadValue")),
			xi::BadWindow => return Err(format!("XISelectEvents BadWindow")),
			_ => return Err(format!("XISelectEvents Interal Error"))
		}
		
		::std::mem::forget(mask); // TODO: Don't do this
			
		match xi::XMapWindow(display, window) as u32 {
			xi::BadWindow => return Err(format!("XMapWindow BadWindow")),
			_ => ()
		}
			
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
		
		thread::spawn(move || {
			println!("1");
			let mut basalt_lk = x11window.basalt.lock();
			
			while basalt_lk.is_none() {
				x11window.basalt_ready.wait(&mut basalt_lk);
			}
			println!("2");
			
			let basalt = basalt_lk.take().unwrap();
			drop(basalt_lk);
			
			let display = x11window.display.load(atomic::Ordering::Relaxed);
			let event: *mut xi::_XEvent = ::std::ptr::null_mut();
			let mut window_w = 0;
			let mut window_h = 0;
			println!("3");
			
			loop {
				match xi::XNextEvent(transmute(display), event) {
					0 => (),
					e => panic!("native input: XNextEvent failed with: {}", e)
				}
				
				let mut event = *event;
				
				dbg!(event.xany);
				
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
						println!("Close Request");
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
