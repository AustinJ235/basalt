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
						InputEvent::Cursor {
							win,
							x,
							y,
						} => {
							let window_state =
								win_state.entry(win).or_insert_with(|| WindowState::new(win));

							if let Some([_x, _y]) = window_state.update_cursor_pos(x, y) {
								// TODO: Cursor position changed
							}
						},
						_ => (), // TODO
					},
			}
		}
	});
}
