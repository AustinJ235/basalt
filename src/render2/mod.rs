#![allow(warnings)]

use std::sync::Arc;

mod vk {
    pub use vulkano::format::{Format, FormatFeatures, NumericFormat};
    pub use vulkano::swapchain::{ColorSpace, FullScreenExclusive};
    pub use vulkano_taskgraph::resource::{Flight, Resources};
    pub use vulkano_taskgraph::Id;
}

use flume::Receiver;

use crate::window::Window;

mod context;
mod shaders;
mod worker;

use context::Context;

enum RenderEvent {
    Redraw,
    Update { _dummy: u8 },
    Resize,
}

// TODO: Define Here
pub use crate::render::{VSync, MSAA};

pub struct Renderer {
    window: Arc<Window>,
    context: Context,
    render_flt_id: vk::Id<vk::Flight>,
    worker_flt_id: vk::Id<vk::Flight>,
    render_event_recv: Receiver<RenderEvent>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let window_event_recv = window
            .window_manager_ref()
            .window_event_queue(window.id())
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

        let (render_event_send, render_event_recv) = flume::unbounded();

        let render_flt_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_flight(2)
            .unwrap();

        let worker_flt_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_flight(1)
            .unwrap();

        let context = Context::new(window.clone(), render_flt_id)?;

        worker::spawn(worker::SpawnInfo {
            window: window.clone(),
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format: context.image_format(),
        })?;

        Ok(Self {
            window,
            context,
            render_flt_id,
            worker_flt_id,
            render_event_recv,
        })
    }

    pub fn with_minimal(mut self) -> Result<Self, String> {
        self.context.minimal()?;
        Ok(self)
    }

    pub fn with_interface_only(mut self) -> Self {
        self.context.itf_only();
        self
    }

    pub fn run(mut self) -> Result<(), String> {
        loop {
            self.context.execute()?;
        }
    }
}
