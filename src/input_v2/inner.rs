use super::{Hook, InputEvent, InputHookID};
use crate::interface::Interface;
use crossbeam::channel::Receiver;
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

pub(super) fn begin_loop(_interface: Arc<Interface>, event_recv: Receiver<LoopEvent>) {
	thread::spawn(move || while let Ok(_event) = event_recv.recv() {});
}
