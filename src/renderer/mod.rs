use std::sync::Arc;

use crate::window::Window;

mod worker;

pub struct Renderer {}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, ()> {
        // worker::run(basalt.clone(), window_id, ...);
        todo!();
    }

    pub fn draw(&mut self) {
        todo!()
    }
}
