use std::sync::Arc;

use flume::{Receiver, Sender};

use super::RenderEvent;
use crate::window::{Window, WindowEvent};

mod vk {
    pub use vulkano::format::Format;
    pub use vulkano_taskgraph::resource::{Flight, Resources};
    pub use vulkano_taskgraph::Id;
}

pub struct SpawnInfo {
    pub window: Arc<Window>,
    pub render_flt_id: vk::Id<vk::Flight>,
    pub worker_flt_id: vk::Id<vk::Flight>,
    pub window_event_recv: Receiver<WindowEvent>,
    pub render_event_send: Sender<RenderEvent>,
    pub image_format: vk::Format,
}

pub fn spawn(_info: SpawnInfo) -> Result<(), String> {
    Ok(())
}
