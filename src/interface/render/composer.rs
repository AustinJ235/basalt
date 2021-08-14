use crate::atlas::AtlasImageID;
use crate::image_view::BstImageView;
use crate::interface::bin::Bin;
use crate::interface::ItfVertInfo;
use crate::vulkano::buffer::BufferAccess;
use crate::Basalt;
use crossbeam::channel::unbounded;
use crossbeam::queue::SegQueue;
use crossbeam::sync::{Parker, Unparker};
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{iter, thread};
use vulkano::buffer::cpu_pool::CpuBufferPool;
use vulkano::buffer::{BufferUsage, DeviceLocalBuffer};
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, CommandBufferUsage, PrimaryCommandBuffer,
};
use vulkano::sync::GpuFuture;

type BinID = u64;
type ZIndex = OrderedFloat<f32>;
type BinVertexData = Vec<(Vec<ItfVertInfo>, Option<Arc<BstImageView>>, AtlasImageID)>;

const VERT_SIZE_BYTES: u64 = std::mem::size_of::<ItfVertInfo>() as u64;

pub(crate) struct Composer {
	bst: Arc<Basalt>,
	view: Mutex<Option<Arc<ComposerView>>>,
	view_cond: Condvar,
	ev_queue: SegQueue<ComposerEv>,
	unparker: Unparker,
}

pub(crate) enum ComposerEv {
	Scale(f32),
	Extent([u32; 2]),
}

pub(crate) struct ComposerView {
	pub inst: Instant,
	pub buffers_and_imgs: Vec<Vec<(Arc<DeviceLocalBuffer<[ItfVertInfo]>>, Arc<BstImageView>)>>,
}

impl ComposerView {
	fn atlas_views_stale(&self) -> bool {
		for layer in self.buffers_and_imgs.iter() {
			for (_, img) in layer {
				if img.is_stale() {
					return true;
				}
			}
		}

		false
	}
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

struct BinUpdateIn {
	bin: Arc<Bin>,
	scale: f32,
	extent: [u32; 2],
}

struct BinUpdateOut {
	bin: Arc<Bin>,
	inst: Instant,
	scale: f32,
	extent: [u32; 2],
	data: BinVertexData,
}

impl Composer {
	pub fn send_event(&self, ev: ComposerEv) {
		self.ev_queue.push(ev);
		self.unparker.unpark();
	}

	pub fn unpark(&self) {
		self.unparker.unpark();
	}

	pub fn new(bst: Arc<Basalt>) -> Arc<Self> {
		let parker = Parker::new();

		let composer_ret = Arc::new(Self {
			bst,
			view: Mutex::new(None),
			view_cond: Condvar::new(),
			ev_queue: SegQueue::new(),
			unparker: parker.unparker().clone(),
		});

		let (up_in_s, up_in_r) = unbounded();
		let (up_out_s, up_out_r) = unbounded();
		let update_workers = (num_cpus::get() as f32 / 3.0).ceil() as usize;

		for _ in 0..update_workers {
			let up_in_r = up_in_r.clone();
			let up_out_s = up_out_s.clone();

			thread::spawn(move || {
				loop {
					match up_in_r.recv() {
						Ok(BinUpdateIn {
							bin,
							scale,
							extent,
						}) => {
							bin.do_update([extent[0] as f32, extent[1] as f32], scale);
							let inst = bin.last_update();
							let data = bin.verts_cp();

							if let Err(_) = up_out_s.send(BinUpdateOut {
								bin,
								inst,
								scale,
								extent,
								data,
							}) {
								return;
							}
						},
						Err(_) => return,
					}
				}
			});
		}

		let composer = composer_ret.clone();

		thread::spawn(move || {
			let mut init_complete = crate::BASALT_INIT_COMPLETE.lock();

			while !*init_complete {
				crate::BASALT_INIT_COMPLETE_COND.wait(&mut init_complete);
			}

			let mut bins: BTreeMap<BinID, BinData> = BTreeMap::new();
			let mut layers: BTreeMap<ZIndex, Layer> = BTreeMap::new();
			let mut scale = composer.bst.options_ref().scale;
			let mut extent = composer.bst.options_ref().window_size;

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

			let process_vertex_data = |layers: &mut BTreeMap<ZIndex, Layer>,
			                           bins: &mut BTreeMap<BinID, BinData>,
			                           up_out: BinUpdateOut| {
				let BinUpdateOut {
					bin,
					inst,
					scale,
					extent,
					data,
				} = up_out;
				let id = bin.id();

				for (vertexes, img_op, atlas_id) in data {
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

						let layer_entry = layers.entry(z_index.clone()).or_insert_with(|| {
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

				bins.insert(id, BinData {
					inst,
					scale,
					extent,
				});
			};

			loop {
				while let Some(ev) = composer.ev_queue.pop() {
					match ev {
						ComposerEv::Scale(new_scale) => scale = new_scale,
						ComposerEv::Extent(new_extent) => extent = new_extent,
					}
				}

				let mut state_changed = false;
				let contained_ids: Vec<BinID> = bins.keys().cloned().collect();
				let mut all_bins: BTreeMap<BinID, Arc<Bin>> = BTreeMap::new();

				for bin in composer.bst.interface_ref().bins() {
					all_bins.insert(bin.id(), bin);
				}

				let mut bin_state: BTreeMap<BinID, (BinStatus, UpdateStatus)> = BTreeMap::new();

				for id in contained_ids {
					if all_bins.contains_key(&id) {
						let bin_data = bins.get(&id).unwrap();
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

				let mut updates_in_prog = 0;

				for (id, (status, update)) in bin_state {
					let bin_op = match &status {
						BinStatus::Exists | BinStatus::Create =>
							Some(all_bins.get(&id).unwrap()),
						_ => None,
					};

					if update == UpdateStatus::Required {
						let bin = (**bin_op.as_ref().unwrap()).clone();

						up_in_s
							.send(BinUpdateIn {
								bin,
								extent,
								scale,
							})
							.expect("All bin update threads have panicked!");

						updates_in_prog += 1;
					}

					if (status == BinStatus::Exists && update == UpdateStatus::Required)
						|| status == BinStatus::Remove
					{
						bins.remove(&id).unwrap();

						for layer in layers.values_mut() {
							if let Some(_) = layer.vertex.remove(&id) {
								layer.composed = None;
							}
						}
					}

					if status == BinStatus::Create && update == UpdateStatus::Current {
						let bin = bin_op.unwrap().clone();
						let inst = bin.last_update();
						let data = bin.verts_cp();

						process_vertex_data(&mut layers, &mut bins, BinUpdateOut {
							bin,
							inst,
							scale,
							extent,
							data,
						});
					}
				}

				let mut update_received = 0;

				while update_received < updates_in_prog {
					let out = up_out_r.recv().expect("All bin update threads have panicked!");
					process_vertex_data(&mut layers, &mut bins, out);
					update_received += 1;
				}

				layers.retain(|_, layer| {
					let empty = layer.vertex.is_empty();

					if empty {
						state_changed = true;
					}

					!empty
				});

				let upload_buf_pool = CpuBufferPool::upload(composer.bst.device());
				let mut cmd_buf = AutoCommandBufferBuilder::primary(
					composer.bst.device(),
					composer.bst.transfer_queue_ref().family(),
					CommandBufferUsage::OneTimeSubmit,
				)
				.unwrap();
				let mut exec_cmd_buf = false;

				for (zindex, layer) in layers.iter_mut() {
					if layer.composed.is_none() {
						let mut composed_vertex = Vec::new();

						for bin_vertex_datas in layer.vertex.values().cloned() {
							for VertexData {
								img,
								mut data,
							} in bin_vertex_datas
							{
								let mut composed_vertex_i_op = None;

								for (i, (composed_img, _)) in composed_vertex.iter().enumerate()
								{
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
							let len = vertexes.len() as u64;
							let src_buf = upload_buf_pool.chunk(vertexes).unwrap();

							let dst_buf = DeviceLocalBuffer::array(
								composer.bst.device(),
								len,
								BufferUsage {
									transfer_destination: true,
									vertex_buffer: true,
									..BufferUsage::none()
								},
								iter::once(composer.bst.graphics_queue().family()),
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
						.execute(composer.bst.transfer_queue())
						.unwrap()
						.then_signal_fence_and_flush()
						.unwrap()
						.wait(None)
						.unwrap();
				}

				if state_changed
					|| composer
						.view
						.lock()
						.as_ref()
						.map(|v| v.atlas_views_stale())
						.unwrap_or(true)
				{
					let mut view = ComposerView {
						inst: Instant::now(),
						buffers_and_imgs: Vec::new(),
					};

					let atlas_views = composer
						.bst
						.atlas_ref()
						.image_views()
						.map(|v| v.1)
						.unwrap_or_else(|| Arc::new(HashMap::new()));
					let empty_image = composer.bst.atlas_ref().empty_image();

					for layer in layers.values().rev() {
						if let Some(composed) = layer.composed.clone() {
							assert!(!composed.buffers.is_empty());
							let mut composed_view = Vec::with_capacity(composed.buffers.len());

							for (i, (vertex_img, buffer)) in composed.buffers.iter().enumerate()
							{
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

					*composer.view.lock() = Some(Arc::new(view));
					composer.view_cond.notify_all();
				}

				parker.park_timeout(Duration::from_millis(1000));
			}
		});

		composer_ret
	}

	pub fn check_view(&self, view_op: Option<Arc<ComposerView>>) -> Arc<ComposerView> {
		let mut composer_view_op = self.view.lock();

		while composer_view_op.is_none() {
			self.view_cond.wait(&mut composer_view_op);
		}

		let composer_view = composer_view_op.as_ref().cloned().unwrap();

		if view_op.is_none() {
			composer_view
		} else if view_op.as_ref().unwrap().inst < composer_view.inst {
			composer_view
		} else {
			view_op.unwrap()
		}
	}
}
