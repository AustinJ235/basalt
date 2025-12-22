use std::sync::Arc;
use std::time::Duration;

use flume::Sender;

use crate::Basalt;
use crate::input::InputEvent;
use crate::interval::{IntvlHookCtrl, IntvlHookID};
use crate::window::WindowID;

pub struct KeyRepeatState {
    basalt: Arc<Basalt>,
    active_code_op: Option<u32>,
    repeat_send: Sender<RepeatEvent>,
    intvl_hook_id: IntvlHookID,
}

enum RepeatEvent {
    Start(RepeatInfo),
    Stop,
}

struct RepeatInfo {
    window_id: WindowID,
    utf8: String,
}

impl KeyRepeatState {
    pub fn new(basalt: Arc<Basalt>) -> Self {
        let (repeat_send, repeat_recv) = flume::unbounded();
        let cb_basalt = basalt.clone();
        let mut repeat_info_op = None;

        let intvl_hook_id = basalt.interval_ref().do_every(
            Duration::from_millis(40),
            Some(Duration::from_millis(600)),
            move |_| {
                for repeat_event in repeat_recv.drain() {
                    match repeat_event {
                        RepeatEvent::Start(repeat_info) => {
                            repeat_info_op = Some(repeat_info);
                        },
                        RepeatEvent::Stop => {
                            repeat_info_op.take();
                        },
                    }
                }

                let repeat_info = match repeat_info_op.as_ref() {
                    Some(some) => some,
                    None => return IntvlHookCtrl::Pause,
                };

                for c in repeat_info.utf8.chars() {
                    cb_basalt.input_ref().send_event(InputEvent::Character {
                        win: repeat_info.window_id,
                        c,
                    });
                }

                Default::default()
            },
        );

        Self {
            basalt,
            active_code_op: None,
            repeat_send,
            intvl_hook_id,
        }
    }

    pub fn begin_repeat(&mut self, window_id: WindowID, code: u32, utf8: String) {
        self.repeat_send
            .send(RepeatEvent::Start(RepeatInfo {
                window_id,
                utf8,
            }))
            .unwrap();

        self.active_code_op = Some(code);
        self.basalt.interval_ref().pause(self.intvl_hook_id);
        self.basalt.interval_ref().start(self.intvl_hook_id);
    }

    pub fn release_key(&mut self, code: u32) {
        if Some(code) == self.active_code_op {
            self.active_code_op = None;
            let _ = self.repeat_send.send(RepeatEvent::Stop);
        }
    }
}

impl Drop for KeyRepeatState {
    fn drop(&mut self) {
        self.basalt.interval_ref().remove(self.intvl_hook_id);
    }
}
