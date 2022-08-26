use crate::input_v2::state::WindowState;
use crate::input_v2::{proc, Hook, InputEvent, InputHookID};
use crate::interface::Interface;
use crate::window::BstWindowID;
use crossbeam::channel::Receiver;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

pub(in crate::input_v2) enum LoopEvent {
	Normal(InputEvent),
	Add {
		id: InputHookID,
		hook: Hook,
	},
	Remove(InputHookID),
}

pub(in crate::input_v2) fn begin_loop(
	interface: Arc<Interface>,
	event_recv: Receiver<LoopEvent>,
) {
	thread::spawn(move || {
		let mut hooks: HashMap<InputHookID, Hook> = HashMap::new();
		let mut win_state: HashMap<BstWindowID, WindowState> = HashMap::new();

		while let Ok(event) = event_recv.recv() {
			match event {
				LoopEvent::Add {
					id,
					hook,
				} => {
					hooks.insert(id, hook);
				},
				LoopEvent::Remove(id) => {
					hooks.remove(&id);
				},
				LoopEvent::Normal(event) =>
					match event {
						InputEvent::Press {
							win,
							key,
						} => {
							proc::press(&interface, &mut hooks, &mut win_state, win, key);
						},
						InputEvent::Release {
							win,
							key,
						} => {
							proc::release(&mut hooks, &mut win_state, win, key);
						},
						InputEvent::Focus {
							win,
						} => {
							proc::window_focus(&mut hooks, &mut win_state, win, true);
						},
						InputEvent::FocusLost {
							win,
						} => {
							proc::window_focus(&mut hooks, &mut win_state, win, false);
						},
						InputEvent::Cursor {
							win,
							x,
							y,
						} => {
							proc::cursor(&interface, &mut hooks, &mut win_state, win, x, y);
						},
						InputEvent::Enter {
							win,
						} => {
							proc::window_cursor_inside(&mut hooks, &mut win_state, win, true);
						},
						InputEvent::Leave {
							win,
						} => {
							proc::window_cursor_inside(&mut hooks, &mut win_state, win, false);
						},
						_ => (), // TODO
					},
			}
		}
	});
}
