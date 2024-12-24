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

        let image_format = if context.surface_format().components()[0] > 8 {
            vec![
                vk::Format::R16G16B16A16_UINT,
                vk::Format::R16G16B16A16_UNORM,
                vk::Format::R8G8B8A8_UINT,
                vk::Format::R8G8B8A8_UNORM,
                vk::Format::B8G8R8A8_UINT,
                vk::Format::B8G8R8A8_UNORM,
                vk::Format::A8B8G8R8_UINT_PACK32,
                vk::Format::A8B8G8R8_UNORM_PACK32,
                vk::Format::R8G8B8A8_SRGB,
                vk::Format::B8G8R8A8_SRGB,
                vk::Format::A8B8G8R8_SRGB_PACK32,
            ]
        } else {
            vec![
                vk::Format::R8G8B8A8_UINT,
                vk::Format::R8G8B8A8_UNORM,
                vk::Format::B8G8R8A8_UINT,
                vk::Format::B8G8R8A8_UNORM,
                vk::Format::A8B8G8R8_UINT_PACK32,
                vk::Format::A8B8G8R8_UNORM_PACK32,
                vk::Format::R8G8B8A8_SRGB,
                vk::Format::B8G8R8A8_SRGB,
                vk::Format::A8B8G8R8_SRGB_PACK32,
            ]
        }
        .into_iter()
        .find(|format| {
            let properties = match window
                .basalt_ref()
                .physical_device_ref()
                .format_properties(*format)
            {
                Ok(ok) => ok,
                Err(_) => return false,
            };

            properties.optimal_tiling_features.contains(
                vk::FormatFeatures::TRANSFER_DST
                    | vk::FormatFeatures::TRANSFER_SRC
                    | vk::FormatFeatures::SAMPLED_IMAGE
                    | vk::FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
            )
        })
        .ok_or(String::from("Failed to find suitable image format."))?;

        worker::spawn(worker::SpawnInfo {
            window: window.clone(),
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,
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
        todo!()
    }

    pub fn run(mut self) -> Result<(), String> {
        loop {
            self.context.execute()?;
        }
    }
}
