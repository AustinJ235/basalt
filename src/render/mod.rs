//! Window rendering

mod context;
mod error;
mod shaders;
mod worker;

use std::any::Any;
use std::fmt::Write;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use flume::Receiver;
use parking_lot::{Condvar, Mutex};
use smallvec::smallvec;

mod vko {
    pub use vulkano::buffer::Buffer;
    pub use vulkano::format::{ClearColorValue, ClearValue, Format, NumericFormat};
    pub use vulkano::image::Image;
    pub use vulkano::sync::Sharing;
    pub use vulkano_taskgraph::Id;
    pub use vulkano_taskgraph::graph::{NodeId, ResourceMap, TaskGraph};
    pub use vulkano_taskgraph::resource::Flight;
}

use crate::NonExhaustive;
pub use crate::render::context::RendererContext;
pub use crate::render::error::{
    ContextCreateError, ContextError, RendererCreateError, RendererError, VulkanoError,
    WorkerCreateError, WorkerError,
};
use crate::render::worker::{Worker, WorkerPerfMetrics};
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
    fn initialize(&mut self, flight_id: vko::Id<vko::Flight>);
    /// Called everytime the target image changes.
    fn target_changed(&mut self, target_image_id: vko::Id<vko::Image>);
    /// Called everytime before the `TaskGraph` is recreated.
    fn task_graph_info(&mut self) -> UserTaskGraphInfo;
    /// Called everytime during the creation of `TaskGraph`.
    fn task_graph_build(
        &mut self,
        task_graph: &mut vko::TaskGraph<RendererContext>,
        target_image_vid: vko::Id<vko::Image>,
    ) -> vko::NodeId;
    /// Called before the execution of the `TaskGraph`.
    fn task_graph_resources(&mut self, resource_map: &mut vko::ResourceMap);
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
        writeln!(&mut output, "Frames:               {:>6.1}/s", self.avg_frame_rate).unwrap();
        writeln!(&mut output, "Updates:              {:>6.1}/s", self.avg_update_rate).unwrap();
        writeln!(&mut output, "Cycles:               {:>6.1}/s", self.avg_worker_rate).unwrap();
        writeln!(&mut output, "CPU Time:             {:>5.2} ms", self.avg_cpu_time).unwrap();

        if let Some(worker) = self.avg_worker_metrics.as_ref() {
            writeln!(&mut output, "\nWorker (Average/Cycle)\n").unwrap();
            writeln!(&mut output, "  Bin Updates:        {:>8.2}", worker.bin_count).unwrap();
            writeln!(&mut output, "  Cycle Total:        {:>5.2} ms", worker.total).unwrap();
            writeln!(&mut output, "  Bin Remove:         {:>5.2} ms", worker.bin_remove).unwrap();
            writeln!(&mut output, "  Bin Obtain:         {:>5.2} ms", worker.bin_obtain).unwrap();
            writeln!(&mut output, "  Image Count:        {:>5.2} ms", worker.image_count).unwrap();
            writeln!(&mut output, "  Image Remove:       {:>5.2} ms", worker.image_remove).unwrap();
            writeln!(&mut output, "  Image Obtain:       {:>5.2} ms", worker.image_obtain).unwrap();
            writeln!(&mut output, "  Image Update Prep:  {:>5.2} ms", worker.image_update_prep).unwrap();
            writeln!(&mut output, "  Vertex Count:       {:>5.2} ms", worker.vertex_count).unwrap();
            writeln!(&mut output, "  Vertex Update Prep: {:>5.2} ms", worker.vertex_update_prep).unwrap();
            writeln!(&mut output, "  Swap Wait:          {:>5.2} ms", worker.swap_wait).unwrap();
            writeln!(&mut output, "  Execution:          {:>5.2} ms", worker.execution).unwrap();

            if let Some(ovd) = worker.ovd_metrics.as_ref() {
                writeln!(&mut output, "\nBin Obtain (Avg. Total/Cycle)").unwrap();
                writeln!(&mut output, "  Total:              {:>5.2} ms", ovd.total).unwrap();
                writeln!(&mut output, "  Style:              {:>5.2} ms", ovd.style).unwrap();
                writeln!(&mut output, "  Placment:           {:>5.2} ms", ovd.placement).unwrap();
                writeln!(&mut output, "  Visibility:         {:>5.2} ms", ovd.visibility).unwrap();
                writeln!(&mut output, "  Back Image:         {:>5.2} ms", ovd.back_image).unwrap();
                writeln!(&mut output, "  Back Vertex:        {:>5.2} ms", ovd.back_vertex).unwrap();
                writeln!(&mut output, "  Text Buffer:        {:>5.2} ms", ovd.text_buffer).unwrap();
                writeln!(&mut output, "  Text Layout:        {:>5.2} ms", ovd.text_layout).unwrap();
                writeln!(&mut output, "  Text Vertex:        {:>5.2} ms", ovd.text_vertex).unwrap();
                writeln!(&mut output, "  Overflow:           {:>5.2} ms", ovd.overflow).unwrap();
                writeln!(&mut output, "  Vertex Scale:       {:>5.2} ms", ovd.vertex_scale).unwrap();
                writeln!(&mut output, "  Post Update:        {:>5.2} ms", ovd.post_update).unwrap();
                writeln!(&mut output, "  Worker Process:     {:>5.2} ms", ovd.worker_process).unwrap();
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
        buffer_id: vko::Id<vko::Buffer>,
        image_ids: Vec<vko::Id<vko::Image>>,
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
    render_event_recv: Receiver<RenderEvent>,
    worker_join_handle: Option<JoinHandle<Result<(), WorkerError>>>,

    metrics_state_op: Option<MetricsState>,
    render_events: Vec<RenderEvent>,
    conservative_draw: Option<bool>,
    execute: bool,
    error_occurred: bool,
}

impl Renderer {
    /// Create a new `Renderer` given a window.
    pub fn new(window: Arc<Window>) -> Result<Self, RendererCreateError> {
        let window_event_recv = window
            .event_queue()
            .ok_or(RendererCreateError::WindowHasRenderer)?;

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
                vko::Sharing::Concurrent(smallvec![render_qfi, transfer_qfi])
            } else {
                vko::Sharing::Exclusive
            }
        };

        let render_flt_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_flight(2) // TODO: Configurable?
            .map_err(VulkanoError::CreateFlight)?;

        let (render_event_send, render_event_recv) = flume::unbounded();
        let context =
            RendererContext::new(window.clone(), render_flt_id, resource_sharing.clone())?;

        let worker_join_handle = Some(Worker::spawn(worker::SpawnInfo {
            window,
            window_event_recv,
            render_event_send,
            image_format: context.image_format(),
            render_flt_id,
            resource_sharing,
        })?);

        Ok(Self {
            context,
            render_event_recv,
            worker_join_handle,

            metrics_state_op: None,
            render_events: Vec::new(),
            conservative_draw: None,
            execute: false,
            error_occurred: false,
        })
    }

    /// This renderer will only render an interface.
    ///
    /// ***Note:** This method or `user_renderer` must be called before running.*
    pub fn interface_only(&mut self) -> &mut Self {
        self.context.with_interface_only();
        self
    }

    /// This renderer will render an interface on top of the user’s output.
    ///
    /// ***Note:** This method or `interface_only` must be called before running.*
    pub fn user_renderer<R>(&mut self, user_renderer: R) -> &mut Self
    where
        R: UserRenderer + Any,
    {
        self.context.with_user_renderer(user_renderer);
        self
    }

    /// Set the scale of the interface. This does not include dpi scaling.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_interface_scale`*
    pub fn interface_scale(&mut self, scale: f32) -> &mut Self {
        self.context.window_ref().set_interface_scale(scale);
        self
    }

    /// Set the scale of the interface. This includes dpi scaling.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_effective_interface_scale`*
    pub fn effective_interface_scale(&mut self, scale: f32) -> &mut Self {
        self.context
            .window_ref()
            .set_effective_interface_scale(scale);
        self
    }

    /// Set the current MSAA used for rendering.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_renderer_msaa`*
    pub fn msaa(&mut self, msaa: MSAA) -> &mut Self {
        self.context.set_msaa(msaa);
        self.context.window_ref().set_renderer_msaa_nev(msaa);
        self
    }

    /// Set the current VSync used for rendering.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_renderer_vsync`*
    pub fn vsync(&mut self, vsync: VSync) -> &mut Self {
        self.context.set_vsync(vsync);
        self.context.window_ref().set_renderer_vsync_nev(vsync);
        self
    }

    /// Set if conservative draw is enabled.
    ///
    /// ***Note:** User renderers will always default to disabled, so this method must be called \
    ///            called now if it is desired for the renderer to begin this way.*
    pub fn conservative_draw(&mut self, enabled: bool) -> &mut Self {
        self.conservative_draw = Some(enabled);
        self.context
            .window_ref()
            .set_renderer_consv_draw_nev(enabled);
        self
    }

    /// Set the current VSync used for rendering.
    ///
    /// ***Note:** This can be changed before or later via `Window::set_renderer_metrics_level`*
    pub fn metrics_level(&mut self, level: RendererMetricsLevel) -> &mut Self {
        self.context.window_ref().set_renderer_metrics_level(level);
        self
    }

    /// Obtain a reference to `RendererContext`.
    pub fn context_ref(&self) -> &RendererContext {
        &self.context
    }

    /// Obtain a mutable reference to `RendererContext`.
    pub fn context_mut(&mut self) -> &mut RendererContext {
        &mut self.context
    }

    fn consv_draw(&mut self) -> bool {
        if self.conservative_draw.is_none() {
            let current = self.context.window_ref().renderer_consv_draw();

            if self.context.is_user_renderer() && current {
                self.context.window_ref().set_renderer_consv_draw_nev(false);
                self.conservative_draw = Some(false);
                false
            } else {
                self.conservative_draw = Some(current);
                current
            }
        } else {
            self.conservative_draw.unwrap()
        }
    }

    fn poll_events(&mut self) -> Result<(), RendererError> {
        if self.render_event_recv.is_disconnected() {
            let ret = match self.worker_join_handle.take() {
                Some(join_handle) => {
                    match join_handle.join() {
                        Ok(worker_result) => {
                            match worker_result {
                                Ok(..) => Err(RendererError::Closed),
                                Err(e) => Err(RendererError::Worker(e)),
                            }
                        },
                        Err(..) => Err(RendererError::Worker(WorkerError::Panicked)),
                    }
                },
                None => Err(RendererError::ErrorNotHandled),
            };

            self.error_occurred = true;
            return ret;
        }

        self.render_events.extend(self.render_event_recv.drain());

        for render_event in self.render_events.drain(..) {
            match render_event {
                RenderEvent::Close => {
                    return Err(RendererError::Closed);
                },
                RenderEvent::Redraw => {
                    self.execute = true;
                },
                RenderEvent::Update {
                    buffer_id,
                    image_ids,
                    draw_count,
                    token,
                    metrics_op,
                } => {
                    self.context
                        .set_buffer_and_images(buffer_id, image_ids, draw_count, token)?;

                    if let Some(metrics_state) = self.metrics_state_op.as_mut() {
                        metrics_state.track_update(metrics_op);
                    }

                    self.execute = true;
                },
                RenderEvent::WorkerCycle(metrics_op) => {
                    if let Some(metrics_state) = self.metrics_state_op.as_mut() {
                        metrics_state.track_worker_cycle(metrics_op);
                    }
                },
                RenderEvent::CheckExtent => {
                    self.context.check_extent();
                    self.execute = true;
                },
                RenderEvent::SetMSAA(msaa) => {
                    self.context.set_msaa(msaa);
                    self.execute = true;
                },
                RenderEvent::SetVSync(vsync) => {
                    self.context.set_vsync(vsync);
                    self.execute = true;
                },
                RenderEvent::SetConsvDraw(enabled) => {
                    self.conservative_draw = Some(enabled);
                },
                RenderEvent::SetMetricsLevel(metrics_level) => {
                    if metrics_level >= RendererMetricsLevel::Basic {
                        self.metrics_state_op = Some(MetricsState::new());
                    } else {
                        self.metrics_state_op = None;
                    }
                },
            }
        }

        Ok(())
    }

    fn poll_event_wait(&mut self) -> Result<(), RendererError> {
        match self.render_event_recv.recv() {
            Ok(render_event) => {
                self.render_events.push(render_event);
                Ok(())
            },
            Err(_) => Err(RendererError::Closed),
        }
    }

    /// Start running the the renderer.
    pub fn run(mut self) -> Result<(), RendererError> {
        loop {
            loop {
                self.poll_events()?;

                if self.execute || !self.consv_draw() {
                    break;
                }

                self.poll_event_wait()?;
            }

            self.context.execute(&mut self.metrics_state_op)?;
            self.execute = false;
        }
    }

    /// Run the renderer with the provided callback.
    ///
    /// Unless there is a reason not to `Renderer::run` should be used instead.
    ///
    /// This method take two callbacks.
    /// - `execute_if` provides `&mut Renderer` and a `bool` which is set to `true` if a draw
    ///   is requested. This will be true when the ui updates, the window resizes or the window
    ///   received a redraw request. If the returned `bool` is `true` the renderer will draw. This
    ///   method may be called many times before `before_execute` method is called.
    /// - `before_execute` provides `&mut Renderer` and is called right before a draw.
    pub fn run_with<A, B>(
        &mut self,
        mut execute_if: A,
        mut before_execute: B,
    ) -> Result<(), RendererError>
    where
        A: FnMut(&mut Renderer, bool) -> bool,
        B: FnMut(&mut Renderer),
    {
        if self.error_occurred {
            return Err(RendererError::ErrorNotHandled);
        }

        loop {
            self.run_once(&mut execute_if, &mut before_execute)?;
        }
    }

    /// Run a single iteration of the renderer with the provided callback.
    ///
    /// Unless there is a reason not to `Renderer::run` should be used instead.
    ///
    /// This behaves similiarly to `Renderer::run_with` execept it only draws once.
    pub fn run_once<A, B>(
        &mut self,
        mut execute_if: A,
        before_execute: B,
    ) -> Result<(), RendererError>
    where
        A: FnMut(&mut Renderer, bool) -> bool,
        B: FnOnce(&mut Renderer),
    {
        if self.error_occurred {
            return Err(RendererError::ErrorNotHandled);
        }

        loop {
            if let Err(e) = self.poll_events() {
                self.error_occurred = true;
                return Err(e);
            }

            if execute_if(self, self.execute) {
                break;
            }

            if let Err(e) = self.poll_event_wait() {
                self.error_occurred = true;
                return Err(e);
            }
        }

        before_execute(self);

        if let Err(e) = self.context.execute(&mut self.metrics_state_op) {
            self.error_occurred = true;
            return Err(e.into());
        }

        self.execute = false;
        Ok(())
    }
}

fn clear_value_for_format(format: vko::Format) -> vko::ClearValue {
    match format.numeric_format_color().unwrap() {
        vko::NumericFormat::SFLOAT
        | vko::NumericFormat::UFLOAT
        | vko::NumericFormat::SNORM
        | vko::NumericFormat::UNORM
        | vko::NumericFormat::SRGB => vko::ClearValue::Float([0.0; 4]),
        vko::NumericFormat::SINT | vko::NumericFormat::SSCALED => vko::ClearValue::Int([0; 4]),
        vko::NumericFormat::UINT | vko::NumericFormat::USCALED => vko::ClearValue::Uint([0; 4]),
    }
}

fn clear_color_value_for_format(format: vko::Format) -> vko::ClearColorValue {
    match format.numeric_format_color().unwrap() {
        vko::NumericFormat::SFLOAT
        | vko::NumericFormat::UFLOAT
        | vko::NumericFormat::SNORM
        | vko::NumericFormat::UNORM
        | vko::NumericFormat::SRGB => vko::ClearColorValue::Float([0.0; 4]),
        vko::NumericFormat::SINT | vko::NumericFormat::SSCALED => vko::ClearColorValue::Int([0; 4]),
        vko::NumericFormat::UINT | vko::NumericFormat::USCALED => {
            vko::ClearColorValue::Uint([0; 4])
        },
    }
}
