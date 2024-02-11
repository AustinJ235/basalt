use std::sync::Arc;

use crate::window::Window;

mod worker;

pub struct Renderer {}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let _window_event_recv = window
            .window_manager_ref()
            .window_event_queue(window.id())
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

        // worker::run(basalt.clone(), window_id, ...);
        todo!();
    }

    pub fn draw(&mut self) {
        todo!()
    }
}
