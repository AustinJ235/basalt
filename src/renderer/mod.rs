use std::sync::{Arc, Barrier};

use cosmic_text::{FontSystem, SwashCache};
use flume::Receiver;
use vulkano::buffer::Subbuffer;
use vulkano::image::Image;

use crate::image_cache::ImageCacheKey;
use crate::interface::{DefaultFont, ItfVertInfo};
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

enum RenderEvent {
    Redraw,
    Update {
        buffer: Subbuffer<[ItfVertInfo]>,
        images: Vec<Arc<Image>>,
        barrier: Arc<Barrier>,
    },
    Resize {
        width: u32,
        height: u32,
    },
    WindowClosed,
    WindowFullscreenEnabled,
    WindowFullscreenDisabled,
}

pub struct Renderer {
    window: Arc<Window>,
    render_event_recv: Receiver<RenderEvent>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let window_event_recv = window
            .window_manager_ref()
            .window_event_queue(window.id())
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

        let (render_event_send, render_event_recv) = flume::unbounded();
        worker::spawn(window.clone(), window_event_recv, render_event_send)?;

        Ok(Self {
            window,
            render_event_recv,
        })
    }

    pub fn run_interface_only(&mut self) -> Result<(), String> {
        'main_loop: loop {
            for render_event in self.render_event_recv.iter() {
                match render_event {
                    RenderEvent::WindowClosed => break 'main_loop Ok(()),
                    _ => (),
                }
            }
        }
    }

    pub fn run_with_user_renderer<R: UserRenderer>(
        &mut self,
        user_renderer: R,
    ) -> Result<(), String> {
        todo!()
    }
}

pub trait UserRenderer {
    fn surface_changed(&mut self, target_image: Arc<Image>);
    fn draw_requested(&mut self, command_buffer: u8);
}
