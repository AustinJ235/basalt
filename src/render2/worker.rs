use std::sync::Arc;

use flume::{Receiver, Sender};

use super::RenderEvent;
use crate::interface::ItfVertInfo;
use crate::window::{Window, WindowEvent};

mod vk {
    pub use vulkano::buffer::{BufferCreateInfo, BufferUsage};
    pub use vulkano::format::Format;
    pub use vulkano::memory::allocator::{AllocationCreateInfo, DeviceLayout, MemoryTypeFilter};
    pub use vulkano_taskgraph::resource::{Flight, HostAccessType, Resources};
    pub use vulkano_taskgraph::{execute, Id};
}

pub struct SpawnInfo {
    pub window: Arc<Window>,
    pub render_flt_id: vk::Id<vk::Flight>,
    pub worker_flt_id: vk::Id<vk::Flight>,
    pub window_event_recv: Receiver<WindowEvent>,
    pub render_event_send: Sender<RenderEvent>,
    pub image_format: vk::Format,
}

pub fn spawn(spawn_info: SpawnInfo) -> Result<(), String> {
    std::thread::spawn(move || {
        let SpawnInfo {
            window,
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,
        } = spawn_info;

        let buffer_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_buffer(
                vk::BufferCreateInfo {
                    usage: vk::BufferUsage::VERTEX_BUFFER,
                    ..Default::default()
                },
                vk::AllocationCreateInfo {
                    memory_type_filter: vk::MemoryTypeFilter::PREFER_DEVICE
                        | vk::MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                vk::DeviceLayout::new_unsized::<[ItfVertInfo]>(9).unwrap(),
            )
            .unwrap();

        let vertexes = [
            ([1.0, -1.0, 0.2], [1.0; 4]),
            ([-1.0, -1.0, 0.2], [1.0; 4]),
            ([-1.0, 1.0, 0.2], [1.0; 4]),
            ([1.0, -1.0, 0.2], [1.0; 4]),
            ([-1.0, 1.0, 0.2], [1.0; 4]),
            ([1.0, 1.0, 0.2], [1.0; 4]),
            ([0.0, -0.8, 0.1], [0.0, 0.0, 1.0, 1.0]),
            ([-0.8, 0.8, 0.1], [0.0, 0.0, 1.0, 1.0]),
            ([0.8, 0.8, 0.1], [0.0, 0.0, 1.0, 1.0]),
        ]
        .into_iter()
        .map(|(position, color)| {
            ItfVertInfo {
                position,
                coords: [0.0; 2],
                color,
                ty: 0,
                tex_i: 0,
            }
        })
        .collect::<Vec<_>>();

        unsafe {
            vk::execute(
                window.basalt_ref().transfer_queue_ref(),
                window.basalt_ref().device_resources_ref(),
                worker_flt_id,
                |_, task| {
                    task.write_buffer::<[ItfVertInfo]>(buffer_id, ..)?
                        .clone_from_slice(&vertexes);
                    Ok(())
                },
                [(buffer_id, vk::HostAccessType::Write)],
                [],
                [],
            )
            .unwrap();
        }

        if render_event_send
            .send(RenderEvent::Update {
                buffer_id,
                image_ids: Vec::new(),
                draw_range: 0..9,
            })
            .is_err()
        {
            return;
        }

        'main: loop {
            for window_event in window_event_recv.drain() {
                match window_event {
                    WindowEvent::Opened => (),
                    WindowEvent::Closed => break 'main,
                    WindowEvent::Resized {
                        width: _,
                        height: _,
                    } => {
                        if render_event_send.send(RenderEvent::CheckExtent).is_err() {
                            break 'main;
                        }
                    },
                    WindowEvent::ScaleChanged(_scale) => (),
                    WindowEvent::RedrawRequested => (),
                    WindowEvent::EnabledFullscreen => (),
                    WindowEvent::DisabledFullscreen => (),
                    WindowEvent::AssociateBin(_bin) => (),
                    WindowEvent::DissociateBin(_bin_id) => (),
                    WindowEvent::UpdateBin(_bin_id) => (),
                    WindowEvent::UpdateBinBatch(_bin_ids) => (),
                    WindowEvent::AddBinaryFont(_bytes) => (),
                    WindowEvent::SetDefaultFont(_default_font) => (),
                    WindowEvent::SetMSAA(msaa) => {
                        if render_event_send.send(RenderEvent::SetMSAA(msaa)).is_err() {
                            break 'main;
                        }
                    },
                    WindowEvent::SetVSync(vsync) => {
                        if render_event_send
                            .send(RenderEvent::SetVSync(vsync))
                            .is_err()
                        {
                            break 'main;
                        }
                    },
                    WindowEvent::SetMetrics(_metrics_level) => (),
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    Ok(())
}
