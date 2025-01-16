mod context;
mod shaders;
mod worker;

mod vk {
    pub use vulkano::buffer::Buffer;
    pub use vulkano::format::{ClearColorValue, ClearValue, Format, NumericFormat};
    pub use vulkano::image::Image;
    pub use vulkano_taskgraph::graph::{NodeId, ResourceMap, TaskGraph};
    pub use vulkano_taskgraph::Id;
}

use std::any::Any;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use flume::Receiver;
pub(crate) use worker::ImageSource;

pub use crate::render::context::RendererContext;
use crate::render::worker::{Worker, WorkerPerfMetrics};
use crate::window::Window;
use crate::NonExhaustive;

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

#[derive(Debug, Clone)]
pub struct UserTaskGraphInfo {
    pub max_nodes: u32,
    pub max_resources: u32,
    pub _ne: NonExhaustive,
}

impl Default for UserTaskGraphInfo {
    fn default() -> Self {
        Self {
            max_nodes: 0,
            max_resources: 0,
            _ne: NonExhaustive(()),
        }
    }
}

pub trait UserRenderer: Any {
    fn target_changed(&mut self, target_image_id: vk::Id<vk::Image>);
    fn task_graph_info(&mut self) -> UserTaskGraphInfo;
    fn task_graph_build(&mut self, task_graph: &mut vk::TaskGraph<RendererContext>) -> vk::NodeId;
    fn task_graph_resources(&mut self, resource_map: &mut vk::ResourceMap);
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

struct MetricsState {
    start: Instant,
    last_acquire: Instant,
    last_update: Instant,
    cpu_times: Vec<f32>,
    gpu_times: Vec<f32>,
    update_times: Vec<f32>,
    worker_metrics: Vec<WorkerPerfMetrics>,
}

impl MetricsState {
    fn new() -> Self {
        let inst = Instant::now();

        Self {
            start: inst,
            last_acquire: inst,
            last_update: inst,
            cpu_times: Vec::new(),
            gpu_times: Vec::new(),
            update_times: Vec::new(),
            worker_metrics: Vec::new(),
        }
    }

    fn track_acquire(&mut self) {
        self.gpu_times
            .push(self.last_acquire.elapsed().as_micros() as f32 / 1000.0);
        self.last_acquire = Instant::now();
    }

    fn track_present(&mut self) {
        self.cpu_times
            .push(self.last_acquire.elapsed().as_micros() as f32 / 1000.0);
    }

    fn track_update(&mut self, worker_metrics_op: Option<WorkerPerfMetrics>) {
        self.update_times
            .push(self.last_update.elapsed().as_micros() as f32 / 1000.0);
        self.last_update = Instant::now();

        if let Some(worker_metrics) = worker_metrics_op {
            self.worker_metrics.push(worker_metrics);
        }
    }

    fn tracked_time(&self) -> Duration {
        self.start.elapsed()
    }

    fn complete(&mut self) -> RendererPerfMetrics {
        let (total_updates, avg_update_rate, avg_worker_metrics) = if !self.update_times.is_empty()
        {
            let avg_worker_metrics = if !self.worker_metrics.is_empty() {
                let mut total_worker_metrics = WorkerPerfMetrics::default();
                let count = self.worker_metrics.len();

                for worker_metrics in self.worker_metrics.drain(..) {
                    total_worker_metrics += worker_metrics;
                }

                total_worker_metrics /= count as f32;
                Some(total_worker_metrics)
            } else {
                None
            };

            (
                self.update_times.len(),
                1000.0 / (self.update_times.iter().sum::<f32>() / self.update_times.len() as f32),
                avg_worker_metrics,
            )
        } else {
            (0, 0.0, None)
        };

        let (total_frames, avg_cpu_time, avg_frame_rate) = if !self.gpu_times.is_empty() {
            (
                self.gpu_times.len(),
                self.cpu_times.iter().sum::<f32>() / self.cpu_times.len() as f32,
                1000.0 / (self.gpu_times.iter().sum::<f32>() / self.gpu_times.len() as f32),
            )
        } else {
            (0, 0.0, 0.0)
        };

        *self = Self::new();

        RendererPerfMetrics {
            total_updates,
            avg_update_rate,
            avg_worker_metrics,
            total_frames,
            avg_cpu_time,
            avg_frame_rate,
        }
    }
}

enum RenderEvent {
    Close,
    Redraw,
    Update {
        buffer_id: vk::Id<vk::Buffer>,
        image_ids: Vec<vk::Id<vk::Image>>,
        draw_count: u32,
        metrics_op: Option<WorkerPerfMetrics>,
        barrier: Arc<Barrier>,
    },
    CheckExtent,
    SetMSAA(MSAA),
    SetVSync(VSync),
    SetMetricsLevel(RendererMetricsLevel),
}

pub struct Renderer {
    context: RendererContext,
    conservative_draw: bool,
    render_event_recv: Receiver<RenderEvent>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let window_event_recv = window
            .window_manager_ref()
            .window_event_queue(window.id())
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

        let (render_event_send, render_event_recv) = flume::unbounded();
        let context = RendererContext::new(window.clone())?;
        let conservative_draw = window.basalt_ref().config.render_default_consv_draw;

        Worker::spawn(worker::SpawnInfo {
            window,
            window_event_recv,
            render_event_send,
            image_format: context.image_format(),
        });

        Ok(Self {
            context,
            conservative_draw,
            render_event_recv,
        })
    }

    pub fn with_interface_only(mut self) -> Self {
        self.context.with_interface_only();
        self
    }

    pub fn with_user_renderer<R: UserRenderer + Any>(mut self, user_renderer: R) -> Self {
        self.context.with_user_renderer(user_renderer);
        self
    }

    pub fn run(mut self) -> Result<(), String> {
        let mut metrics_state_op: Option<MetricsState> = None;
        let mut render_events = Vec::new();
        let mut execute = false;

        'main: loop {
            loop {
                render_events.extend(self.render_event_recv.drain());

                for render_event in render_events.drain(..) {
                    match render_event {
                        RenderEvent::Close => break 'main,
                        RenderEvent::Redraw => {
                            execute = true;
                        },
                        RenderEvent::Update {
                            buffer_id,
                            image_ids,
                            draw_count,
                            barrier,
                            metrics_op,
                        } => {
                            self.context
                                .set_buffer_and_images(buffer_id, image_ids, draw_count, barrier);

                            if let Some(metrics_state) = metrics_state_op.as_mut() {
                                metrics_state.track_update(metrics_op);
                            }

                            execute = true;
                        },
                        RenderEvent::CheckExtent => {
                            self.context.check_extent();
                            execute = true;
                        },
                        RenderEvent::SetMSAA(msaa) => {
                            self.context.set_msaa(msaa);
                            execute = true;
                        },
                        RenderEvent::SetVSync(vsync) => {
                            self.context.set_vsync(vsync);
                            execute = true;
                        },
                        RenderEvent::SetMetricsLevel(metrics_level) => {
                            if metrics_level >= RendererMetricsLevel::Basic {
                                metrics_state_op = Some(MetricsState::new());
                            } else {
                                metrics_state_op = None;
                            }
                        },
                    }
                }

                if !self.conservative_draw {
                    if self.render_event_recv.is_disconnected() {
                        break 'main;
                    } else {
                        break;
                    }
                } else if execute {
                    break;
                }

                match self.render_event_recv.recv() {
                    Ok(render_event) => render_events.push(render_event),
                    Err(_) => break 'main,
                }
            }

            self.context.execute(&mut metrics_state_op)?;
            execute = false;
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
