use Engine;
use std::sync::Arc;
use winit;
use winit::WindowEvent;
use winit::DeviceEvent;
use super::*;

pub const ENABLED: bool = true;

pub fn run(engine: Arc<Engine>, events_loop: &mut winit::EventsLoop) {
	if !ENABLED {
		return;
	}

	let mut mouse_inside = true;
	
	events_loop.run_forever(|ev| {
		match ev {
			winit::Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
				engine.exit();
				return winit::ControlFlow::Break;
			},
			
			winit::Event::WindowEvent { event: WindowEvent::CursorMoved { position, .. }, .. } => {
				let winit::dpi::PhysicalPosition { x, y }
					= position.to_physical(engine.surface.window().get_hidpi_factor());
				engine.input_ref().send_event(Event::MousePosition(x as f32, y as f32));
			},
			
			winit::Event::WindowEvent { event: WindowEvent::KeyboardInput { input, .. }, .. } => {
				engine.input_ref().send_event(match input.state {
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
			
				engine.input_ref().send_event(match state {
					winit::ElementState::Pressed => Event::MousePress(button),
					winit::ElementState::Released => Event::MouseRelease(button),
				});
			},
			
			#[cfg(target_os = "windows")]
			winit::Event::WindowEvent { event: WindowEvent::MouseWheel { delta, .. }, .. } => {
				if mouse_inside {
					engine.input_ref().send_event(match delta {
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
				engine.input_ref().send_event(Event::MouseEnter);
			},
			
			winit::Event::WindowEvent { event: WindowEvent::CursorLeft { .. }, .. } => {
				mouse_inside = false;
				engine.input_ref().send_event(Event::MouseLeave);
			},
			
			winit::Event::WindowEvent { event: WindowEvent::Resized { .. }, .. } => {
				engine.input_ref().send_event(Event::WindowResized);
			},
			
			winit::Event::WindowEvent { event: WindowEvent::HiDpiFactorChanged(dpi), .. } => {
				engine.input_ref().send_event(Event::WindowDPIChange(dpi as f32));
			},
			
			winit::Event::WindowEvent { event: WindowEvent::Focused(focused), .. } => {
				engine.input_ref().send_event(match focused {
					true => Event::WindowFocused,
					false => Event::WindowLostFocus
				});
			},
			
			winit::Event::DeviceEvent { event: DeviceEvent::Motion { axis, value }, .. } => {
				engine.input_ref().send_event(match axis {
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
}

