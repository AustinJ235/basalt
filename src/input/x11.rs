use std::sync::Arc;
use Basalt;
use std::thread;
use std::mem::transmute;
use winit::os::unix::WindowExt;
use bindings::xinput2 as xi;
use input::qwery::*;
use input::{Event,MouseButton};
use std::os::raw::c_char;
use std::slice;

pub fn run(basalt: Arc<Basalt>) {
	thread::spawn(move || unsafe {
		let display: *mut xi::_XDisplay = match basalt.surface_ref().window().get_xlib_display() {
			Some(some) => transmute(some),
			None => panic!("native input: unable to obtain xlib display from surface")
		};
		
		let window = match basalt.surface_ref().window().get_xlib_window() {
			Some(some) => some,
			None => panic!("native input: unable to obtain xlib window from surface")
		};
		
		/*let display = match xi::XOpenDisplay(null_mut()) {
			e if e.is_null() => panic!("unable to open x display"),
			d => d
		};
		
		let window = match xi::XDefaultRootWindow(display) {
			w if w == 0 => panic!("unable to open root window"),
			w => w
		};*/
		
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
								0 => MouseButton::Left,
								1 => MouseButton::Middle,
								2 => MouseButton::Right,
								o => MouseButton::Other(o as u8),
							};
							
							basalt.input_ref().send_event(Event::MousePress(button));
						},
						
						xi::XI_RawButtonRelease => {
							let ev: &mut xi::XIRawEvent = transmute(cookie.data);
							let button = match ev.detail {
								0 => MouseButton::Left,
								1 => MouseButton::Middle,
								2 => MouseButton::Right,
								o => MouseButton::Other(o as u8),
							};
							
							basalt.input_ref().send_event(Event::MousePress(button));
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
				}
				
				, _ => ()
			}
		}
	});
}

