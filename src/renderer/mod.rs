use std::sync::Arc;

use cosmic_text::{FontSystem, SwashCache};
use vulkano::image::Image;

use crate::image_cache::ImageCacheKey;
use crate::interface::DefaultFont;
use crate::window::Window;

mod worker;

pub(crate) struct UpdateContext {
    pub extent: [f32; 2],
    pub scale: f32,
    pub font_system: FontSystem,
    pub glyph_cache: SwashCache,
    pub default_font: DefaultFont,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) enum ImageSource {
    #[default]
    None,
    Cache(ImageCacheKey),
    Vulkano(Arc<Image>),
}

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
