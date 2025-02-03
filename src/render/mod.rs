//! Window rendering

mod context;
mod shaders;
mod worker;

mod vk {
    pub use vulkano::buffer::Buffer;
    pub use vulkano::format::{ClearColorValue, ClearValue, Format, NumericFormat};
    pub use vulkano::image::Image;
    pub use vulkano::sync::Sharing;
    pub use vulkano_taskgraph::graph::{NodeId, ResourceMap, TaskGraph};
    pub use vulkano_taskgraph::resource::Flight;
    pub use vulkano_taskgraph::Id;
}

use std::any::Any;
use std::fmt::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flume::Receiver;
use parking_lot::{Condvar, Mutex};
use smallvec::smallvec;
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

/// Information about the user's created task graph node(s).
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

/// Trait used for user provided renderers.
pub trait UserRenderer: Any {
    /// Called once when `Renderer` is initialized.
    fn initialize(&mut self, flight_id: vk::Id<vk::Flight>);
    /// Called everytime the target image changes.
    fn target_changed(&mut self, target_image_id: vk::Id<vk::Image>);
    /// Called everytime before the `TaskGraph` is recreated.
    fn task_graph_info(&mut self) -> UserTaskGraphInfo;
    /// Called everytime during the creation of `TaskGraph`.
    fn task_graph_build(
        &mut self,
        task_graph: &mut vk::TaskGraph<RendererContext>,
        target_image_vid: vk::Id<vk::Image>,
    ) -> vk::NodeId;
    /// Called before the execution of the `TaskGraph`.
    fn task_graph_resources(&mut self, resource_map: &mut vk::ResourceMap);
}

/// Performance metrics of a `Renderer`.
#[derive(Debug, Clone, Default)]
pub struct RendererPerfMetrics {
    pub total_frames: usize,
    pub total_updates: usize,
    pub worker_cycles: usize,
    pub avg_cpu_time: f32,
    pub avg_frame_rate: f32,
    pub avg_update_rate: f32,
    pub avg_worker_rate: f32,
    pub avg_worker_metrics: Option<WorkerPerfMetrics>,
}

impl RendererPerfMetrics {
    /// Display metrics in a pretty way suitable for display within the interface.
    ///
    /// For best results, use a monospace font.
    #[rustfmt::skip]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        write!(&mut output, "Frames:               {:>6.1}/s\n", self.avg_frame_rate).unwrap();
        write!(&mut output, "Updates:              {:>6.1}/s\n", self.avg_update_rate).unwrap();
        write!(&mut output, "Cycles:               {:>6.1}/s\n", self.avg_worker_rate).unwrap();
        write!(&mut output, "CPU Time:             {:>5.2} ms\n", self.avg_cpu_time).unwrap();

        if let Some(worker) = self.avg_worker_metrics.as_ref() {
            write!(&mut output, "\nWorker (Average/Cycle)\n").unwrap();
            write!(&mut output, "  Bin Updates:        {:>8.2}\n", worker.bin_count).unwrap();
            write!(&mut output, "  Cycle Total:        {:>5.2} ms\n", worker.total).unwrap();
            write!(&mut output, "  Bin Remove:         {:>5.2} ms\n", worker.bin_remove).unwrap();
            write!(&mut output, "  Bin Obtain:         {:>5.2} ms\n", worker.bin_obtain).unwrap();
            write!(&mut output, "  Image Count:        {:>5.2} ms\n", worker.image_count).unwrap();
            write!(&mut output, "  Image Remove:       {:>5.2} ms\n", worker.image_remove).unwrap();
            write!(&mut output, "  Image Obtain:       {:>5.2} ms\n", worker.image_obtain).unwrap();
            write!(&mut output, "  Image Update Prep:  {:>5.2} ms\n", worker.image_update_prep).unwrap();
            write!(&mut output, "  Vertex Count:       {:>5.2} ms\n", worker.vertex_count).unwrap();
            write!(&mut output, "  Vertex Update Prep: {:>5.2} ms\n", worker.vertex_update_prep).unwrap();
            write!(&mut output, "  Swap Wait:          {:>5.2} ms\n", worker.swap_wait).unwrap();
            write!(&mut output, "  Execution:          {:>5.2} ms\n", worker.execution).unwrap();

            if let Some(ovd) = worker.ovd_metrics.as_ref() {
                write!(&mut output, "\nBin Obtain (Avg. Total/Cycle)\n").unwrap();
                write!(&mut output, "  Total:              {:>5.2} ms\n", ovd.total).unwrap();
                write!(&mut output, "  Style:              {:>5.2} ms\n", ovd.style).unwrap();
                write!(&mut output, "  Placment:           {:>5.2} ms\n", ovd.placement).unwrap();
                write!(&mut output, "  Visibility:         {:>5.2} ms\n", ovd.visibility).unwrap();
                write!(&mut output, "  Back Image:         {:>5.2} ms\n", ovd.back_image).unwrap();
                write!(&mut output, "  Back Vertex:        {:>5.2} ms\n", ovd.back_vertex).unwrap();
                write!(&mut output, "  Text Buffer:        {:>5.2} ms\n", ovd.text_buffer).unwrap();
                write!(&mut output, "  Text Layout:        {:>5.2} ms\n", ovd.text_layout).unwrap();
                write!(&mut output, "  Text Vertex:        {:>5.2} ms\n", ovd.text_vertex).unwrap();
                write!(&mut output, "  Overflow:           {:>5.2} ms\n", ovd.overflow).unwrap();
                write!(&mut output, "  Vertex Scale:       {:>5.2} ms\n", ovd.vertex_scale).unwrap();
                write!(&mut output, "  Post Update:        {:>5.2} ms\n", ovd.post_update).unwrap();
            }
        }

        output
    }
}

struct MetricsState {
    start: Instant,
    last_acquire: Instant,
    last_update: Instant,
    cpu_times: Vec<f32>,
    gpu_times: Vec<f32>,
    update_times: Vec<f32>,
    worker_cycles: usize,
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
            worker_cycles: 0,
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
        self.worker_cycles += 1;

        if let Some(worker_metrics) = worker_metrics_op {
            self.worker_metrics.push(worker_metrics);
        }
    }

    fn track_worker_cycle(&mut self, worker_metrics_op: Option<WorkerPerfMetrics>) {
        self.worker_cycles += 1;

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

        let worker_cycles = self.worker_cycles;
        let avg_worker_rate =
            worker_cycles as f32 / (self.start.elapsed().as_micros() as f32 / 1000000.0);

        *self = Self::new();

        RendererPerfMetrics {
            total_updates,
            worker_cycles,
            avg_update_rate,
            avg_worker_metrics,
            total_frames,
            avg_cpu_time,
            avg_frame_rate,
            avg_worker_rate,
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
        token: Arc<(Mutex<Option<u64>>, Condvar)>,
    },
    WorkerCycle(Option<WorkerPerfMetrics>),
    CheckExtent,
    SetMSAA(MSAA),
    SetVSync(VSync),
    SetConsvDraw(bool),
    SetMetricsLevel(RendererMetricsLevel),
}

/// Provides rendering for a window.
pub struct Renderer {
    context: RendererContext,
    set_consv_draw: Option<bool>,
    render_event_recv: Receiver<RenderEvent>,
}

impl Renderer {
    /// Create a new `Renderer` given a window.
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let window_event_recv = window
            .event_queue()
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

        let resource_sharing = {
            let render_qfi = window
                .basalt_ref()
                .graphics_queue_ref()
                .queue_family_index();
            let transfer_qfi = window
                .basalt_ref()
                .transfer_queue_ref()
                .queue_family_index();

            if render_qfi != transfer_qfi {
                vk::Sharing::Concurrent(smallvec![render_qfi, transfer_qfi])
            } else {
                vk::Sharing::Exclusive
            }
        };

        let render_flt_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_flight(2) // TODO: Configurable?
            .unwrap();

        let (render_event_send, render_event_recv) = flume::unbounded();
        let context =
            RendererContext::new(window.clone(), render_flt_id, resource_sharing.clone())?;

        Worker::spawn(worker::SpawnInfo {
            window,
            window_event_recv,
            render_event_send,
            image_format: context.image_format(),
            render_flt_id,
            resource_sharing,
        });

        Ok(Self {
            context,
            set_consv_draw: None,
            render_event_recv,
        })
    }

    /// This renderer will only render an interface.
    ///
    /// ***Note:** This method or `user_renderer` must be called before running.*
    pub fn interface_only(mut self) -> Self {
        self.context.with_interface_only();
        self
    }

    /// This renderer will render an interface on top of the user’s output.
    ///
    /// ***Note:** This method or `interface_only` must be called before running.*
    pub fn user_renderer<R>(mut self, user_renderer: R) -> Self
    where
        R: UserRenderer + Any,
    {
        self.context.with_user_renderer(user_renderer);
        self
    }

    /// Set the scale of the interface. This does not include dpi scaling.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_interface_scale`*
    pub fn interface_scale(self, scale: f32) -> Self {
        self.context.window_ref().set_interface_scale(scale);
        self
    }

    /// Set the scale of the interface. This includes dpi scaling.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_effective_interface_scale`*
    pub fn effective_interface_scale(self, scale: f32) -> Self {
        self.context
            .window_ref()
            .set_effective_interface_scale(scale);
        self
    }

    /// Set the current MSAA used for rendering.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_renderer_msaa`*
    pub fn msaa(mut self, msaa: MSAA) -> Self {
        self.context.set_msaa(msaa);
        self.context.window_ref().set_renderer_msaa_nev(msaa);
        self
    }

    /// Set the current VSync used for rendering.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_renderer_vsync`*
    pub fn vsync(mut self, vsync: VSync) -> Self {
        self.context.set_vsync(vsync);
        self.context.window_ref().set_renderer_vsync_nev(vsync);
        self
    }

    /// Set if conservative draw is enabled.
    ///
    /// ***Note:** User renderers will always default to disabled, so this method must be called \
    ///            called now if it is desired for the renderer to begin this way.*
    pub fn conservative_draw(mut self, enabled: bool) -> Self {
        self.set_consv_draw = Some(enabled);
        self.context
            .window_ref()
            .set_renderer_consv_draw_nev(enabled);
        self
    }

    /// Set the current VSync used for rendering.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_renderer_metrics_level`*
    pub fn metrics_level(self, level: RendererMetricsLevel) -> Self {
        self.context.window_ref().set_renderer_metrics_level(level);
        self
    }

    /// Start running the the renderer.
    pub fn run(mut self) -> Result<(), String> {
        let mut metrics_state_op: Option<MetricsState> = None;
        let mut render_events = Vec::new();
        let mut execute = false;

        let mut conservative_draw = match self.set_consv_draw.take() {
            Some(enabled) => enabled,
            None => {
                let current = self.context.window_ref().renderer_consv_draw();

                if self.context.is_user_renderer() && current {
                    self.context.window_ref().set_renderer_consv_draw_nev(false);
                    false
                } else {
                    current
                }
            },
        };

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
                            token,
                            metrics_op,
                        } => {
                            self.context
                                .set_buffer_and_images(buffer_id, image_ids, draw_count, token);

                            if let Some(metrics_state) = metrics_state_op.as_mut() {
                                metrics_state.track_update(metrics_op);
                            }

                            execute = true;
                        },
                        RenderEvent::WorkerCycle(metrics_op) => {
                            if let Some(metrics_state) = metrics_state_op.as_mut() {
                                metrics_state.track_worker_cycle(metrics_op);
                            }
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
                        RenderEvent::SetConsvDraw(enabled) => {
                            conservative_draw = enabled;
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

                if !conservative_draw {
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
