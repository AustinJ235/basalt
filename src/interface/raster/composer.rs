use crate::atlas::AtlasImageID;
use crate::image_view::BstImageView;
use crate::interface::bin::Bin;
use crate::interface::interface::ItfVertInfo;
use crate::vulkano::buffer::BufferAccess;
use crate::Basalt;
use ordered_float::OrderedFloat;
use std::collections::{BTreeMap, HashMap};
use std::iter;
use std::sync::Arc;
use std::time::Instant;
use vulkano::buffer::cpu_pool::CpuBufferPool;
use vulkano::buffer::{BufferUsage, DeviceLocalBuffer};
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, CommandBufferUsage, PrimaryCommandBuffer,
};
use vulkano::sync::GpuFuture;

type BinID = u64;
type ZIndex = OrderedFloat<f32>;

const VERT_SIZE_BYTES: usize = std::mem::size_of::<ItfVertInfo>();

pub(super) struct Composer {
	bst: Arc<Basalt>,
	inst: Instant,
	bins: BTreeMap<BinID, BinData>,
	layers: BTreeMap<ZIndex, Layer>,
}

pub(super) struct ComposerView {
	pub inst: Instant,
	pub buffers_and_imgs: Vec<Vec<(Arc<DeviceLocalBuffer<[ItfVertInfo]>>, Arc<BstImageView>)>>,
}

struct BinData {
	inst: Instant,
	scale: f32,
	extent: [u32; 2],
}

struct Layer {
	vertex: BTreeMap<BinID, Vec<VertexData>>,
	composed: Option<LayerComposed>,
}

#[derive(Clone)]
struct VertexData {
	img: VertexImage,
	data: Vec<ItfVertInfo>,
}

#[derive(Clone, PartialEq)]
enum VertexImage {
	None,
	Atlas(AtlasImageID),
	Custom(Arc<BstImageView>),
}

#[derive(Clone)]
struct LayerComposed {
	buffers: Vec<(VertexImage, Arc<DeviceLocalBuffer<[ItfVertInfo]>>)>,
}

impl Composer {
	pub fn new(bst: Arc<Basalt>) -> Self {
		Self {
			bst,
			inst: Instant::now(),
			bins: BTreeMap::new(),
			layers: BTreeMap::new(),
		}
	}

	pub fn check_view(&mut self, view_op: Option<ComposerView>) -> ComposerView {
		if view_op.is_none() || view_op.as_ref().unwrap().inst < self.inst {
			let mut view = match view_op {
				Some(mut old_view) => {
					old_view.buffers_and_imgs.clear();
					old_view.inst = self.inst.clone();
					old_view
				},
				None =>
					ComposerView {
						inst: self.inst.clone(),
						buffers_and_imgs: Vec::new(),
					},
			};

			let atlas_views = self
				.bst
				.atlas_ref()
				.image_views()
				.map(|v| v.1)
				.unwrap_or_else(|| Arc::new(HashMap::new()));
			let empty_image = self.bst.atlas_ref().empty_image();

			for layer in self.layers.values().rev() {
				if let Some(composed) = layer.composed.clone() {
					assert!(!composed.buffers.is_empty());
					let mut composed_view = Vec::with_capacity(composed.buffers.len());

					for (i, (vertex_img, buffer)) in composed.buffers.iter().enumerate() {
						assert!(
							(i != 0 && buffer.size() > 0)
								|| buffer.size() >= VERT_SIZE_BYTES * 6
						);

						let img_view = match vertex_img {
							VertexImage::None => empty_image.clone(),
							VertexImage::Atlas(atlas_img) =>
								match atlas_views.get(&atlas_img) {
									Some(some) => some.clone(),
									None => empty_image.clone(),
								},
							VertexImage::Custom(view) => view.clone(),
						};

						composed_view.push((buffer.clone(), img_view));
					}

					view.buffers_and_imgs.push(composed_view);
				}
			}

			view
		} else {
			view_op.unwrap()
		}
	}

	pub fn update_and_compose(&mut self, scale: f32, extent: [u32; 2]) {
		#[derive(PartialEq, Eq)]
		enum BinStatus {
			Exists,
			Create,
			Remove,
		}

		#[derive(PartialEq, Eq)]
		enum UpdateStatus {
			Current,
			Required,
		}

		let mut state_changed = false;
		let contained_ids: Vec<BinID> = self.bins.keys().cloned().collect();
		let mut all_bins: BTreeMap<BinID, Arc<Bin>> = BTreeMap::new();

		for bin in self.bst.interface_ref().bins() {
			all_bins.insert(bin.id(), bin);
		}

		let mut bin_state: BTreeMap<BinID, (BinStatus, UpdateStatus)> = BTreeMap::new();

		for id in contained_ids {
			if all_bins.contains_key(&id) {
				let bin_data = self.bins.get(&id).unwrap();
				let bin = all_bins.get(&id).unwrap();

				let update_status = if bin.wants_update() {
					UpdateStatus::Required
				} else if bin_data.extent != extent || bin_data.scale != scale {
					UpdateStatus::Required
				} else if bin_data.inst < bin.last_update() {
					UpdateStatus::Required
				} else {
					UpdateStatus::Current
				};

				bin_state.insert(id, (BinStatus::Exists, update_status));
			} else {
				bin_state.insert(id, (BinStatus::Remove, UpdateStatus::Current));
			}
		}

		for (id, bin) in all_bins.iter() {
			if !bin_state.contains_key(id) {
				let post_up = bin.post_update();

				let update_status = if bin.wants_update() {
					UpdateStatus::Required
				} else if post_up.extent != extent || post_up.scale != scale {
					UpdateStatus::Required
				} else {
					UpdateStatus::Current
				};

				bin_state.insert(*id, (BinStatus::Create, update_status));
			}
		}

		for (id, (status, update)) in bin_state {
			let mut vertex_data_op = None;
			let bin_op = match &status {
				BinStatus::Exists | BinStatus::Create => Some(all_bins.get(&id).unwrap()),
				_ => None,
			};

			if update == UpdateStatus::Required {
				let bin = bin_op.as_ref().unwrap();
				bin.do_update([extent[0] as f32, extent[1] as f32], scale);
				vertex_data_op = Some(bin.verts_cp());
			}

			if (status == BinStatus::Exists && vertex_data_op.is_some())
				|| status == BinStatus::Remove
			{
				self.bins.remove(&id).unwrap();

				for layer in self.layers.values_mut() {
					if let Some(_) = layer.vertex.remove(&id) {
						layer.composed = None;
					}
				}
			}

			let vertex_data = match &status {
				BinStatus::Remove => continue,
				BinStatus::Exists => vertex_data_op.unwrap_or_else(|| Vec::new()),
				BinStatus::Create => bin_op.as_ref().unwrap().verts_cp(),
			};

			for (vertexes, img_op, atlas_id) in vertex_data {
				let vertex_img = match img_op {
					Some(some) => VertexImage::Custom(some),
					None =>
						if atlas_id == 0 {
							VertexImage::None
						} else {
							VertexImage::Atlas(atlas_id)
						},
				};

				for vertex in vertexes {
					let z_index = OrderedFloat::from(vertex.position.2);
					
					let layer_entry = self.layers.entry(z_index.clone()).or_insert_with(|| {
						Layer {
							vertex: BTreeMap::new(),
							composed: None,
						}
					});

					layer_entry.composed = None;

					let vertex_entry =
						layer_entry.vertex.entry(id).or_insert_with(|| Vec::new());
					let mut vertex_entry_i_op = None;

					for (i, entry_vertex_data) in vertex_entry.iter().enumerate() {
						if entry_vertex_data.img == vertex_img {
							vertex_entry_i_op = Some(i);
							break;
						}
					}

					let vertex_entry_i = match vertex_entry_i_op {
						Some(some) => some,
						None => {
							vertex_entry.push(VertexData {
								img: vertex_img.clone(),
								data: Vec::new(),
							});

							vertex_entry.len() - 1
						},
					};

					vertex_entry[vertex_entry_i].data.push(vertex);
				}
			}

			let bin = bin_op.as_ref().unwrap();

			self.bins.insert(id, BinData {
				inst: bin.last_update(),
				scale,
				extent,
			});
		}

		self.layers.retain(|_, layer| {
			let empty = layer.vertex.is_empty();

			if empty {
				state_changed = true;
			}

			!empty
		});

		let upload_buf_pool = CpuBufferPool::upload(self.bst.device());
		let mut cmd_buf = AutoCommandBufferBuilder::primary(
			self.bst.device(),
			self.bst.transfer_queue_ref().family(),
			CommandBufferUsage::OneTimeSubmit,
		)
		.unwrap();
		let mut exec_cmd_buf = false;

		for (zindex, layer) in self.layers.iter_mut() {
			if layer.composed.is_none() {
				let mut composed_vertex = Vec::new();

				for bin_vertex_datas in layer.vertex.values().cloned() {
					for VertexData {
						img,
						mut data,
					} in bin_vertex_datas
					{
						let mut composed_vertex_i_op = None;

						for (i, (composed_img, _)) in composed_vertex.iter().enumerate() {
							if img == *composed_img {
								composed_vertex_i_op = Some(i);
								break;
							}
						}

						let composed_vertex_i = match composed_vertex_i_op {
							Some(some) => some,
							None => {
								composed_vertex.push((img, Vec::new()));
								composed_vertex.len() - 1
							},
						};

						composed_vertex[composed_vertex_i].1.append(&mut data);
					}
				}

				const SQUARE_POSITIONS: [[f32; 2]; 6] =
					[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [1.0, 1.0], [-1.0, 1.0], [
						-1.0, -1.0,
					]];

				let mut clear_verts: Vec<_> = SQUARE_POSITIONS
					.iter()
					.map(|[x, y]| {
						ItfVertInfo {
							position: (*x, *y, **zindex),
							coords: (0.0, 0.0),
							color: (0.0, 0.0, 0.0, 0.0),
							ty: -1,
						}
					})
					.collect();

				clear_verts.append(&mut composed_vertex[0].1);
				composed_vertex[0].1 = clear_verts;

				let mut composed_buf = Vec::with_capacity(composed_vertex.len());

				for (img, vertexes) in composed_vertex {
					let len = vertexes.len();
					let src_buf = upload_buf_pool.chunk(vertexes).unwrap();

					let dst_buf = DeviceLocalBuffer::array(
						self.bst.device(),
						len,
						BufferUsage {
							transfer_destination: true,
							vertex_buffer: true,
							..BufferUsage::none()
						},
						iter::once(self.bst.graphics_queue().family()),
					)
					.unwrap();

					cmd_buf.copy_buffer(src_buf, dst_buf.clone()).unwrap();

					composed_buf.push((img, dst_buf));
					exec_cmd_buf = true;
				}

				layer.composed = Some(LayerComposed {
					buffers: composed_buf,
				});

				state_changed = true;
			}
		}

		if exec_cmd_buf {
			cmd_buf
				.build()
				.unwrap()
				.execute(self.bst.transfer_queue())
				.unwrap()
				.then_signal_fence_and_flush()
				.unwrap()
				.wait(None)
				.unwrap();
		}

		if state_changed {
			self.inst = Instant::now();
		}
	}
}
