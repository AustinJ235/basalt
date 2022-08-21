use super::{Hook, InputEvent, InputHookID};
use crate::interface::bin::BinID;
use crate::interface::Interface;
use crate::window::BstWindowID;
use crossbeam::channel::Receiver;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

pub(super) enum LoopEvent {
	Normal(InputEvent),
	Add {
		id: InputHookID,
		hook: Hook,
	},
	Remove(InputHookID),
}

#[derive(Default)]
struct WindowState {
	focused_bin: Option<BinID>,
}

pub(super) fn begin_loop(_interface: Arc<Interface>, event_recv: Receiver<LoopEvent>) {
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
						} => {},
						_ => (), // TODO
					},
			}
		}
	});
}
