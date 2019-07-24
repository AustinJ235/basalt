use std::sync::Arc;
use Basalt;
use std::thread;
use std::mem::transmute;
use std::ptr::null_mut;
use winit::os::unix::WindowExt;

use bindings::xinput2::XIGrabKeycode;
use bindings::xinput2::XIAllDevices;
use bindings::xinput2::XIAnyKeycode;
use bindings::xinput2::XDefaultRootWindow;
use bindings::xinput2::XIGrabModeAsync;
use bindings::xinput2::XIEventMask;
use bindings::xinput2::XNextEvent;
use bindings::xinput2::XGenericEvent;
use bindings::xinput2::XKeyEvent;
use bindings::xinput2::XButtonEvent;

use input::qwery::*;
use input::Event;

pub fn run(basalt: Arc<Basalt>) {
	thread::spawn(move || unsafe {
		let display = match basalt.surface_ref().window().get_xlib_display() {
			Some(some) => some,
			None => panic!("native input: unable to obtain xlib display from surface")
		};
		
		/*let window = match basalt.surface_ref().window().get_xlib_window() {
			Some(some) => some,
			None => panic!("native input: unable to obtain xlib window from surface")
		};*/
		
		let root_window = XDefaultRootWindow(transmute(display));
		let mut mask_bits = [0; 4];
		
		let mut mask = XIEventMask {
			deviceid: XIAllDevices as i32,
			mask_len: 4,
			mask: mask_bits.as_mut_ptr(),
		};
	
		match XIGrabKeycode(
			transmute(display),
			XIAllDevices as i32,
			XIAnyKeycode as i32,
			root_window,
			XIGrabModeAsync as i32,
			XIGrabModeAsync as i32,
			1, &mut mask, 0, null_mut()
		) {
			0 => (),
			e => panic!("native input: XIGrabKeycode failed with: {}", e)
		}
		
		let mut event = ::std::mem::uninitialized();
		
		loop {
			match XNextEvent(transmute(display), &mut event) {
				0 => (),
				e => panic!("native input: XNextEvent failed with: {}", e)
			}
			
			let (evtype, evdata) = match event.type_ {
				35 => (transmute::<_, &mut XGenericEvent>(&mut event).evtype, &mut event),
				_ => (event.type_, &mut event)
			};
			
			match evtype {
				2 => (), // Window Key Press, Handled by 13
				3 => (), // Window Key Release, Handled by 14
				
				4 => { // Window Mouse Press
					let x_button_ev = transmute::<_, &mut XButtonEvent>(evdata);
					dbg!(x_button_ev);
					// TODO
				},
				
				5 => { // Window Mouse Release
					// TODO
				},
				
				6 => { // Window Mouse Move
					// TODO
				},
				
				7 => { // Window Mouse Enter
					basalt.input_ref().send_event(Event::MouseEnter);
				},
				
				8 => { // Window Mouse Leave
					basalt.input_ref().send_event(Event::MouseLeave);
				},
				
				9 => { // Window Focused
					basalt.input_ref().send_event(Event::WindowFocused);
				},
				
				10 => { // Window Lost Focus
					basalt.input_ref().send_event(Event::WindowLostFocus);
				},
				
				12 => { // Window Resized
					basalt.input_ref().send_event(Event::WindowResized);
				},
				
				13 => { // Key Press
					let x_key_ev = transmute::<_, &mut XKeyEvent>(evdata);
					basalt.input_ref().send_event(Event::KeyPress(Qwery::from(x_key_ev.keycode)));
				},
				
				14 => { // Key Release
					let x_key_ev = transmute::<_, &mut XKeyEvent>(evdata);
					basalt.input_ref().send_event(Event::KeyRelease(Qwery::from(x_key_ev.keycode)));
				},
				
				15 => (), // Mouse Press, Handled by 4
				16 => (), // Mouse Release, Handled by 5
				17 => (), // Mouse Move, Handled by 6
				
				18 => { // Window Minimized
				
				},
				
				19 => { // Window Unminimized
				
				},
				
				22 => { // Window Moved
				
				},
				
				33 => { // Window Close Request
					basalt.exit();
				},
				
				35 => unreachable!(),
				t => println!("Unknown event type: {}", t)
			}
		}
	});
}

