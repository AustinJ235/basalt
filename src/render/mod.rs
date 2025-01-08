mod context;
mod shaders;
mod worker;

mod vk {
    pub use vulkano::buffer::Buffer;
    pub use vulkano::format::{ClearColorValue, ClearValue, Format, NumericFormat};
    pub use vulkano::image::Image;
    pub use vulkano_taskgraph::Id;
}

use std::sync::{Arc, Barrier};

use context::Context;
use flume::Receiver;
pub(crate) use worker::ImageSource;
pub use worker::WorkerPerfMetrics;

use crate::window::Window;

/// Used to specify the MSAA sample count of the ui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MSAA {
    X1,
    X2,
    X4,
    X8,
}

/// Used to specify if VSync should be enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VSync {
    Enable,
    Disable,
}

/// Defines the level of metrics tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RendererMetricsLevel {
    /// No Metrics
    None,
    /// Renderer Metrics
    Basic,
    /// Renderer Metrics & Worker Metrics
    Extended,
    /// Renderer Metrics, Worker Metrics, & OVD Metrics
    ///
    /// ***Note:** This level may impact performance.*
    Full,
}

/// Performance metrics of a `Renderer`.
#[derive(Debug, Clone, Default)]
pub struct RendererPerfMetrics {
    pub total_frames: usize,
    pub total_updates: usize,
    pub avg_cpu_time: f32,
    pub avg_frame_rate: f32,
    pub avg_update_rate: f32,
    pub avg_worker_metrics: Option<WorkerPerfMetrics>,
}

enum RenderEvent {
    Redraw,
    Update {
        buffer_id: vk::Id<vk::Buffer>,
        image_ids: Vec<vk::Id<vk::Image>>,
        draw_count: u32,
        barrier: Arc<Barrier>,
    },
    CheckExtent,
    SetMSAA(MSAA),
    SetVSync(VSync),
}

// TODO: Define Here
use self::worker::Worker;

pub struct Renderer {
    context: Context,
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

        Worker::spawn(worker::SpawnInfo {
            window,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format: context.image_format(),
        });

        Ok(Self {
            context,
            render_event_recv,
        })
    }

    pub fn with_interface_only(mut self) -> Self {
        self.context.itf_only();
        self
    }

    pub fn run(mut self) -> Result<(), String> {
        loop {
            if self.render_event_recv.is_disconnected() {
                break;
            }

            for event in self.render_event_recv.drain() {
                match event {
                    RenderEvent::Redraw => (), // TODO:
                    RenderEvent::Update {
                        buffer_id,
                        image_ids,
                        draw_count,
                        barrier,
                    } => {
                        self.context
                            .set_buffer_and_images(buffer_id, image_ids, draw_count, barrier);
                    },
                    RenderEvent::CheckExtent => {
                        self.context.check_extent();
                    },
                    RenderEvent::SetMSAA(msaa) => {
                        self.context.set_msaa(msaa);
                    },
                    RenderEvent::SetVSync(vsync) => {
                        self.context.set_vsync(vsync);
                    },
                }
            }

            self.context.execute()?;
        }

        Ok(())
    }
}

fn clear_value_for_format(format: vk::Format) -> vk::ClearValue {
    match format.numeric_format_color().unwrap() {
        vk::NumericFormat::SFLOAT
        | vk::NumericFormat::UFLOAT
        | vk::NumericFormat::SNORM
        | vk::NumericFormat::UNORM
        | vk::NumericFormat::SRGB => vk::ClearValue::Float([0.0; 4]),
        vk::NumericFormat::SINT | vk::NumericFormat::SSCALED => vk::ClearValue::Int([0; 4]),
        vk::NumericFormat::UINT | vk::NumericFormat::USCALED => vk::ClearValue::Uint([0; 4]),
    }
}

fn clear_color_value_for_format(format: vk::Format) -> vk::ClearColorValue {
    match format.numeric_format_color().unwrap() {
        vk::NumericFormat::SFLOAT
        | vk::NumericFormat::UFLOAT
        | vk::NumericFormat::SNORM
        | vk::NumericFormat::UNORM
        | vk::NumericFormat::SRGB => vk::ClearColorValue::Float([0.0; 4]),
        vk::NumericFormat::SINT | vk::NumericFormat::SSCALED => vk::ClearColorValue::Int([0; 4]),
        vk::NumericFormat::UINT | vk::NumericFormat::USCALED => vk::ClearColorValue::Uint([0; 4]),
    }
}
