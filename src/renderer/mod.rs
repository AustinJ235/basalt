use std::sync::Arc;

use crate::window::BasaltWindow;

mod worker;

pub struct Renderer {}

impl Renderer {
    pub fn new(window: Arc<dyn BasaltWindow>) -> Self {
        // worker::run(basalt.clone(), window_id, ...);
        todo!();
    }

    pub fn draw(&mut self) {
        todo!()
    }
}
