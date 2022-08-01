pub mod image;

pub use self::image::{Image, ImageData, ImageDims, ImageType};

use crate::image_view::BstImageView;
use crate::Basalt;
use crossbeam::channel::{self, Sender, TryRecvError};
use crossbeam::sync::{Parker, Unparker};
use guillotiere::{
	AllocId as GuillotiereID, AllocatorOptions as GuillotiereOptions,
	AtlasAllocator as Guillotiere,
};
use ilmenite::ImtWeight;
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use smallvec::{smallvec, SmallVec};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage as VkBufferUsage;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, BlitImageInfo, BufferImageCopy, ClearColorImageInfo,
	CommandBufferUsage, CopyBufferToImageInfo, CopyImageInfo, ImageBlit, ImageCopy,
	PrimaryAutoCommandBuffer, PrimaryCommandBuffer,
};
use vulkano::format::ClearColorValue;
use vulkano::image::immutable::ImmutableImage;
use vulkano::image::{
	ImageAccess, ImageCreateFlags, ImageDimensions as VkImgDimensions,
	ImageUsage as VkImageUsage, MipmapsCount, StorageImage,
};
use vulkano::sampler::{Sampler, SamplerCreateInfo};
use vulkano::sync::GpuFuture;

const ATLAS_IMAGE_COUNT: usize = 4;
const ALLOC_MIN: i32 = 16;
const ALLOC_MAX: i32 = 1024;
const ALLOC_PAD: i32 = 2;
const ALLOC_PAD_2X: i32 = ALLOC_PAD * 2;

pub type AtlasImageID = u64;
pub type SubImageID = u64;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub enum SubImageCacheID {
	Path(PathBuf),
	Url(String),
	Glyph(String, ImtWeight, u16, OrderedFloat<f32>),
	#[default]
	None,
}

impl SubImageCacheID {
	pub fn path<P: Into<PathBuf>>(p: P) -> Self {
		SubImageCacheID::Path(p.into())
	}

	pub fn url<U: Into<String>>(u: U) -> Self {
		SubImageCacheID::Url(u.into())
	}
}

/// Defines how long images are retain within the `Atlas` after all `AtlasCoords` referencing them
/// have been dropped.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AtlasCacheCtrl {
	/// Immediately remove the image.
	#[default]
	Immediate,
	/// Always keep the images stored.
	Indefinite,
	/// Keep the images stored for a specficed time.
	Seconds(u64),
}

#[derive(Clone)]
pub struct AtlasCoords {
	img_id: AtlasImageID,
	tlwh: [f32; 4],
	inner: Option<Arc<CoordsInner>>,
}

impl AtlasCoords {
	pub fn none() -> Self {
		Self {
			img_id: 0,
			tlwh: [0.0; 4],
			inner: None,
		}
	}

	pub fn external(x: f32, y: f32, w: f32, h: f32) -> Self {
		Self {
			img_id: u64::max_value(),
			tlwh: [x, y, w, h],
			inner: None,
		}
	}

	/// Returns true if `AtlasCoords` was constructed via `external()`.
	pub fn is_external(&self) -> bool {
		self.img_id == u64::max_value()
	}

	/// Returns true if `AtlasCoords` was constructed via `none()`.
	pub fn is_none(&self) -> bool {
		self.img_id == 0
	}

	pub fn image_id(&self) -> AtlasImageID {
		self.img_id
	}

	pub fn tlwh(&self) -> [f32; 4] {
		self.tlwh
	}

	pub fn top_left(&self) -> [f32; 2] {
		[self.tlwh[0], self.tlwh[1]]
	}

	pub fn top_right(&self) -> [f32; 2] {
		[self.tlwh[0] + self.tlwh[2], self.tlwh[1]]
	}

	pub fn bottom_left(&self) -> [f32; 2] {
		[self.tlwh[0], self.tlwh[1] + self.tlwh[3]]
	}

	pub fn bottom_right(&self) -> [f32; 2] {
		[self.tlwh[0] + self.tlwh[2], self.tlwh[1] + self.tlwh[3]]
	}

	pub fn width_height(&self) -> [f32; 2] {
		[self.tlwh[2], self.tlwh[3]]
	}
}

struct CoordsInner {
	atlas: Arc<Atlas>,
	img_id: AtlasImageID,
	sub_img_id: SubImageID,
}

impl Drop for CoordsInner {
	fn drop(&mut self) {
		let CoordsInner {
			atlas,
			img_id,
			sub_img_id,
		} = self;

		atlas.cmd_send.send(Command::Dropped(*img_id, *sub_img_id)).unwrap();

		// NOTE: atlas.unparker.unpark() is not called. This shouldn't be an issue though
		//       as it isn't a high priority to remove images. Just need them to be removed
		//       before another allocation and since the queue is FIFO this should be OK.
	}
}

impl std::fmt::Debug for AtlasCoords {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.inner.as_ref() {
			Some(inner) =>
				f.debug_struct("AtlasCoords")
					.field("img_id", &self.img_id)
					.field("sub_img_id", &inner.sub_img_id)
					.field("tlwh", &self.tlwh)
					.finish(),
			None if self.img_id == 0 =>
				f.debug_struct("AtlasCoords").field("img_id", &"None").finish(),
			_ =>
				f.debug_struct("AtlasCoords")
					.field("img_id", &"External")
					.field("tlwh", &self.tlwh)
					.finish(),
		}
	}
}

impl PartialEq for AtlasCoords {
	fn eq(&self, other: &Self) -> bool {
		if self.inner.is_some() != other.inner.is_some() {
			return false;
		}

		if let Some(inner) = self.inner.as_ref() {
			let other_inner = other.inner.as_ref().unwrap();

			if !Arc::ptr_eq(&inner.atlas, &other_inner.atlas)
				|| inner.sub_img_id != other_inner.sub_img_id
			{
				return false;
			}
		}

		self.img_id == other.img_id && self.tlwh == other.tlwh
	}
}

impl Eq for AtlasCoords {}

impl std::hash::Hash for AtlasCoords {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.img_id.hash(state);

		for v in self.tlwh.iter() {
			OrderedFloat::from(*v).hash(state);
		}

		if let Some(inner) = self.inner.as_ref() {
			Arc::as_ptr(&inner.atlas).hash(state);
			inner.sub_img_id.hash(state);
		}
	}
}

enum Command {
	Upload(
		Arc<CommandResponse<Result<AtlasCoords, String>>>,
		SubImageCacheID,
		AtlasCacheCtrl,
		Image,
	),
	CacheIDLookup(Arc<CommandResponse<Option<AtlasCoords>>>, SubImageCacheID),
	BatchCacheIDLookup(Arc<CommandResponse<Vec<Option<AtlasCoords>>>>, Vec<SubImageCacheID>),
	Dropped(AtlasImageID, SubImageID),
	TemporaryViewDropped(usize, usize),
}

struct CommandResponse<T> {
	tmp_response: Mutex<Option<T>>,
	response: Mutex<Option<T>>,
	condvar: Condvar,
}

impl<T> CommandResponse<T> {
	fn new() -> Arc<Self> {
		Arc::new(CommandResponse {
			tmp_response: Mutex::new(None),
			response: Mutex::new(None),
			condvar: Condvar::new(),
		})
	}

	fn respond(&self, val: T) {
		*self.response.lock() = Some(val);
		self.condvar.notify_one();
	}

	fn set_response(&self, val: T) {
		*self.tmp_response.lock() = Some(val);
	}

	fn ready_response(&self) {
		let mut tmp = self.tmp_response.lock();
		let mut res = self.response.lock();
		*res = tmp.take();
		self.condvar.notify_one();
	}

	fn wait_for_response(&self) -> T {
		let mut response = self.response.lock();

		while response.is_none() {
			self.condvar.wait(&mut response);
		}

		response.take().unwrap()
	}
}

pub trait CommandResponseAbstract {
	fn ready_response(&self);
}

impl<T> CommandResponseAbstract for CommandResponse<T> {
	fn ready_response(&self) {
		self.ready_response();
	}
}

pub struct Atlas {
	basalt: Arc<Basalt>,
	cmd_send: Sender<Command>,
	empty_image: Arc<BstImageView>,
	linear_sampler: Arc<Sampler>,
	nearest_sampler: Arc<Sampler>,
	unparker: Unparker,
	image_views: Mutex<Option<(Instant, Arc<HashMap<AtlasImageID, Arc<BstImageView>>>)>>,
}

impl Atlas {
	pub fn new(basalt: Arc<Basalt>) -> Arc<Self> {
		let linear_sampler = Sampler::new(basalt.device(), SamplerCreateInfo {
			mag_filter: vulkano::sampler::Filter::Linear,
			min_filter: vulkano::sampler::Filter::Linear,
			address_mode: [vulkano::sampler::SamplerAddressMode::ClampToBorder; 3],
			border_color: vulkano::sampler::BorderColor::FloatTransparentBlack,
			unnormalized_coordinates: true,
			..SamplerCreateInfo::default()
		})
		.unwrap();

		let nearest_sampler = Sampler::new(basalt.device(), SamplerCreateInfo {
			mag_filter: vulkano::sampler::Filter::Nearest,
			min_filter: vulkano::sampler::Filter::Nearest,
			address_mode: [vulkano::sampler::SamplerAddressMode::ClampToBorder; 3],
			border_color: vulkano::sampler::BorderColor::FloatTransparentBlack,
			unnormalized_coordinates: true,
			..SamplerCreateInfo::default()
		})
		.unwrap();

		let empty_image = BstImageView::from_immutable(
			ImmutableImage::from_iter(
				vec![1.0, 1.0, 1.0, 1.0].into_iter(),
				VkImgDimensions::Dim2d {
					width: 1,
					height: 1,
					array_layers: 1,
				},
				MipmapsCount::One,
				basalt.formats_in_use().atlas,
				basalt.secondary_graphics_queue().unwrap_or_else(|| basalt.graphics_queue()),
			)
			.unwrap()
			.0,
		)
		.unwrap();

		let parker = Parker::new();
		let unparker = parker.unparker().clone();
		let (cmd_send, cmd_recv) = channel::unbounded();

		let atlas_ret = Arc::new(Atlas {
			basalt,
			unparker,
			linear_sampler,
			nearest_sampler,
			empty_image,
			cmd_send,
			image_views: Mutex::new(None),
		});

		let atlas = atlas_ret.clone();

		thread::spawn(move || {
			let mut atlas_images: Vec<AtlasImage> = Vec::new();
			let mut sub_img_id_count = 1;
			let mut cached_map: HashMap<SubImageCacheID, (AtlasImageID, SubImageID)> =
				HashMap::new();
			let mut pending_removal: HashMap<(AtlasImageID, SubImageID), Instant> =
				HashMap::new();
			let mut pending_updates = false;
			let mut responses_pending: Vec<Arc<dyn CommandResponseAbstract>> = Vec::new();

			loop {
				let mut dropped_cmds = Vec::new();
				let mut upload_cmds = Vec::new();
				let mut lookup_cmds = Vec::new();

				loop {
					match cmd_recv.try_recv() {
						Ok(cmd) =>
							match cmd {
								Command::Upload(response, cache_id, cache_ctrl, image) =>
									upload_cmds.push((response, cache_id, cache_ctrl, image)),
								Command::Dropped(img_id, sub_img_id) =>
									dropped_cmds.push((img_id, sub_img_id)),
								cmd @ Command::CacheIDLookup(..)
								| cmd @ Command::BatchCacheIDLookup(..) => lookup_cmds.push(cmd),
								Command::TemporaryViewDropped(img_id, index) => {
									atlas_images[img_id].views[index].updatable = true;
								},
							},
						Err(TryRecvError::Empty) => break,
						Err(TryRecvError::Disconnected) => return,
					}
				}

				pending_updates |= !upload_cmds.is_empty();

				for (img_id, sub_img_id) in dropped_cmds {
					if img_id < 1 {
						continue;
					}

					let atlas_img = match atlas_images.get_mut(img_id as usize - 1) {
						Some(some) => some,
						None => continue,
					};

					let sub_img = match atlas_img.sub_imgs.get_mut(&sub_img_id) {
						Some(some) => some,
						None => continue,
					};

					if sub_img.alive > 0 {
						sub_img.alive -= 1;
					}

					if sub_img.alive > 0 {
						continue;
					}

					match sub_img.cache_ctrl {
						AtlasCacheCtrl::Indefinite => continue,
						AtlasCacheCtrl::Seconds(secs) => {
							pending_removal.insert(
								(img_id, sub_img_id),
								Instant::now() + Duration::from_secs(secs),
							);
							continue;
						},
						AtlasCacheCtrl::Immediate => {
							let mut cache_id = SubImageCacheID::None;

							for (cm_id, (cm_img_id, cm_sub_img_id)) in cached_map.iter() {
								if *cm_sub_img_id == sub_img_id && *cm_img_id == img_id {
									cache_id = cm_id.clone();
									break;
								}
							}

							if cache_id != SubImageCacheID::None {
								cached_map.remove(&cache_id).unwrap();
							}

							let SubImage {
								alloc_id,
								xywh,
								..
							} = atlas_img.sub_imgs.remove(&sub_img_id).unwrap();

							atlas_img.views.iter_mut().for_each(|view| {
								view.contains.retain(|id| *id != sub_img_id);
								view.clear_regions.push(xywh);
							});

							atlas_img.allocator.deallocate(alloc_id);
						},
					}
				}

				if !upload_cmds.is_empty() {
					let now = Instant::now();

					pending_removal.retain(|(img_id, sub_img_id), expires| {
						if *expires <= now {
							let atlas_img = match atlas_images.get_mut(*img_id as usize - 1) {
								Some(some) => some,
								None => return false,
							};

							let sub_img = match atlas_img.sub_imgs.get_mut(sub_img_id) {
								Some(some) => some,
								None => return false,
							};

							if sub_img.alive > 0 {
								return false;
							}

							let mut cache_id = SubImageCacheID::None;

							for (cm_id, (cm_img_id, cm_sub_img_id)) in cached_map.iter() {
								if *cm_sub_img_id == *sub_img_id && *cm_img_id == *img_id {
									cache_id = cm_id.clone();
									break;
								}
							}

							if cache_id != SubImageCacheID::None {
								cached_map.remove(&cache_id).unwrap();
							}

							let SubImage {
								alloc_id,
								xywh,
								..
							} = atlas_img.sub_imgs.remove(sub_img_id).unwrap();

							atlas_img.views.iter_mut().for_each(|view| {
								view.contains.retain(|id| *id != *sub_img_id);
								view.clear_regions.push(xywh);
							});

							atlas_img.allocator.deallocate(alloc_id);
							false
						} else {
							true
						}
					});
				}

				for (response, cache_id, cache_ctrl, image) in upload_cmds {
					let mut coords_op = None;
					let mut image_op = Some(image);

					for atlas_image in atlas_images.iter_mut() {
						match atlas_image.try_allocate(
							image_op.take().unwrap(),
							cache_ctrl,
							sub_img_id_count,
						) {
							Ok(ok) => {
								coords_op = Some(ok);
								break;
							},
							Err(e) => {
								image_op = Some(e);
							},
						}
					}

					if coords_op.is_none() {
						let mut atlas_image =
							AtlasImage::new(atlas.clone(), atlas_images.len());

						match atlas_image.try_allocate(
							image_op.take().unwrap(),
							cache_ctrl,
							sub_img_id_count,
						) {
							Ok(ok) => {
								coords_op = Some(ok);
							},
							Err(_) => {
								response.respond(Err(String::from(
									"Image to big to fit in the atlas.",
								)));
								continue;
							},
						}

						atlas_images.push(atlas_image);
					}

					let coords = coords_op.unwrap();
					sub_img_id_count += 1;

					if cache_id != SubImageCacheID::None {
						let coords_inner = coords.inner.as_ref().unwrap();

						cached_map.insert(
							cache_id.clone(),
							(coords_inner.img_id, coords_inner.sub_img_id),
						);
					}

					response.set_response(Ok(coords));
					responses_pending.push(response);
				}

				for cmd in lookup_cmds {
					match cmd {
						Command::CacheIDLookup(response, cache_id) => {
							response.respond(match cached_map.get(&cache_id) {
								Some((img_id, sub_img_id)) =>
									match atlas_images.get_mut(*img_id as usize - 1) {
										Some(atlas_image) =>
											atlas_image.sub_imgs.get_mut(sub_img_id).map(
												|sub_image| {
													sub_image.coords(
														atlas.clone(),
														*img_id,
														*sub_img_id,
													)
												},
											),
										None => None,
									},
								None => None,
							});
						},
						Command::BatchCacheIDLookup(response, cache_ids) => {
							response.respond(
								cache_ids
									.into_iter()
									.map(|cache_id| {
										match cached_map.get(&cache_id) {
											Some((img_id, sub_img_id)) =>
												match atlas_images.get_mut(*img_id as usize - 1)
												{
													Some(atlas_image) =>
														atlas_image
															.sub_imgs
															.get_mut(sub_img_id)
															.map(|sub_image| {
																sub_image.coords(
																	atlas.clone(),
																	*img_id,
																	*sub_img_id,
																)
															}),
													None => None,
												},
											None => None,
										}
									})
									.collect(),
							);
						},
						_ => unreachable!(),
					}
				}

				if !pending_updates {
					for response in responses_pending.drain(..) {
						response.ready_response();
					}

					parker.park();
					continue;
				}

				let mut cmd_buf = AutoCommandBufferBuilder::primary(
					atlas.basalt.device(),
					atlas
						.basalt
						.secondary_graphics_queue_ref()
						.unwrap_or_else(|| atlas.basalt.graphics_queue_ref())
						.family(),
					CommandBufferUsage::OneTimeSubmit,
				)
				.unwrap();

				pending_updates = false;
				let mut execute_cmd_buf = false;
				let mut ready_responses = true;

				for atlas_image in &mut atlas_images {
					let exec_res = atlas_image.update(&mut cmd_buf);
					execute_cmd_buf |= exec_res.updated;
					pending_updates |= exec_res.pending_update;

					if exec_res.pending_update && !exec_res.updated {
						ready_responses = false;
					}
				}

				if execute_cmd_buf {
					cmd_buf
						.build()
						.unwrap()
						.execute(
							atlas
								.basalt
								.secondary_graphics_queue()
								.unwrap_or_else(|| atlas.basalt.graphics_queue()),
						)
						.unwrap()
						.then_signal_fence_and_flush()
						.unwrap()
						.wait(None)
						.unwrap();

					let mut draw_map = HashMap::new();

					for (i, atlas_image) in atlas_images.iter_mut().enumerate() {
						if let Some(tmp_img) = atlas_image.complete_update() {
							draw_map.insert((i + 1) as u64, tmp_img);
						}
					}

					*atlas.image_views.lock() = Some((Instant::now(), Arc::new(draw_map)));
				}

				// TODO: If not ready, should all responses be withheld?
				if ready_responses {
					for response in responses_pending.drain(..) {
						response.ready_response();
					}
				}

				parker.park();
			}
		});

		atlas_ret
	}

	/// Obtain the current image views for the Atlas images. These should be used direclty when
	/// drawing and dropped afterwards. They should proably shouldn't be stored. Keeping these
	/// views alive will result in the Atlas consuming more resources or even preventing it from
	/// updating at all in a bad case.
	pub fn image_views(
		&self,
	) -> Option<(Instant, Arc<HashMap<AtlasImageID, Arc<BstImageView>>>)> {
		self.image_views.lock().clone()
	}

	pub(crate) fn dump(&self) {
		use vulkano::command_buffer::CopyImageToBufferInfo;

		if let Some((_, image_map)) = self.image_views() {
			if image_map.is_empty() {
				println!("[Basalt]: Unable to dump atlas images: no images present.");
				return;
			}

			let total_texels: u32 =
				image_map.values().map(|img| img.dimensions().num_texels()).sum();
			let texel_bytes = image_map.values().next().unwrap().format().block_size().unwrap();
			let total_bytes = total_texels as u64 * texel_bytes;

			let target_buf: Arc<CpuAccessibleBuffer<[u8]>> = unsafe {
				CpuAccessibleBuffer::uninitialized_array(
					self.basalt.device(),
					total_bytes,
					VkBufferUsage::transfer_dst(),
					false,
				)
				.unwrap()
			};

			let mut cmd_buf = AutoCommandBufferBuilder::primary(
				self.basalt.device(),
				self.basalt.graphics_queue_ref().family(),
				CommandBufferUsage::OneTimeSubmit,
			)
			.unwrap();

			let mut buffer_offset = 0;
			let mut buffer_locations = Vec::new();

			for (id, image) in image_map.iter() {
				let img_start = buffer_offset;

				cmd_buf
					.copy_image_to_buffer(CopyImageToBufferInfo {
						regions: smallvec![BufferImageCopy {
							buffer_offset,
							image_subresource: image.subresource_layers(),
							image_extent: image.dimensions().width_height_depth(),
							..BufferImageCopy::default()
						}],
						..CopyImageToBufferInfo::image_buffer(image.clone(), target_buf.clone())
					})
					.unwrap();

				buffer_offset += image.dimensions().num_texels() as u64 * texel_bytes;

				buffer_locations.push((
					id,
					(img_start as usize)..(buffer_offset as usize),
					image.dimensions().width_height(),
				));
			}

			cmd_buf
				.build()
				.unwrap()
				.execute(self.basalt.graphics_queue())
				.unwrap()
				.then_signal_fence_and_flush()
				.unwrap()
				.wait(None)
				.unwrap();

			let buffer_bytes: &[u8] = &target_buf.read().unwrap();

			for (id, range, [width, height]) in buffer_locations {
				assert!(
					(width * height) as u64 * texel_bytes == (range.end - range.start) as u64
				);
				let start = Instant::now();

				match texel_bytes {
					4 => {
						let data = &buffer_bytes[range.start..range.end];
						let image_buffer: ::image::ImageBuffer<::image::Rgba<u8>, _> =
							::image::ImageBuffer::from_raw(width, height, data).unwrap();
						image_buffer
							.save(format!("./target/atlas-{}.png", id).as_str())
							.unwrap();
					},
					8 => {
						let data = unsafe {
							std::slice::from_raw_parts(
								buffer_bytes[range.start..range.end].as_ptr() as *const u16,
								(width * height * 4) as usize,
							)
						};

						let image_buffer: ::image::ImageBuffer<::image::Rgba<u16>, _> =
							::image::ImageBuffer::from_raw(width, height, data).unwrap();
						image_buffer
							.save(format!("./target/atlas-{}.png", id).as_str())
							.unwrap();
					},
					_ => unreachable!(),
				}

				println!("[Basalt]: Atlas Image #{} in {} ms", id, start.elapsed().as_millis());
			}
		} else {
			println!("[Basalt]: Unable to dump atlas images: no images present.");
		}
	}

	/// General purpose empty image that can be used in descritors where an image is required,
	/// but where it won't be used.
	pub fn empty_image(&self) -> Arc<BstImageView> {
		self.empty_image.clone()
	}

	/// An unnormalized, linear filter, clamp to transparent black border `vulkano::Sampler`
	/// primary used for sampling atlas images. May be useful outside of Basalt.
	pub fn linear_sampler(&self) -> Arc<Sampler> {
		self.linear_sampler.clone()
	}

	/// An unnormalized, nearest filter, clamp to transparent black border `vulkano::Sampler`
	/// primary used for sampling atlas images. May be useful outside of Basalt.
	pub fn nearest_sampler(&self) -> Arc<Sampler> {
		self.nearest_sampler.clone()
	}

	/// Obtain coords given a cache id. If doing this in bulk there will be a considerable
	/// performance improvement when using `batch_cache_coords()`.
	pub fn cache_coords(&self, cache_id: SubImageCacheID) -> Option<AtlasCoords> {
		let response = CommandResponse::new();
		self.cmd_send.send(Command::CacheIDLookup(response.clone(), cache_id)).unwrap();
		self.unparker.unpark();
		response.wait_for_response()
	}

	/// Obtain coords for a set of cache ids. This method will be a considerable
	/// improvment for obtaining coords over `cache_coords` where this is done in bulk.
	pub fn batch_cache_coords(
		&self,
		cache_ids: Vec<SubImageCacheID>,
	) -> Vec<Option<AtlasCoords>> {
		let response = CommandResponse::new();
		self.cmd_send.send(Command::BatchCacheIDLookup(response.clone(), cache_ids)).unwrap();
		self.unparker.unpark();
		response.wait_for_response()
	}

	pub fn load_image(
		&self,
		cache_id: SubImageCacheID,
		cache_ctrl: AtlasCacheCtrl,
		image: Image,
	) -> Result<AtlasCoords, String> {
		let response = CommandResponse::new();

		self.cmd_send
			.send(Command::Upload(
				response.clone(),
				cache_id,
				cache_ctrl,
				image.atlas_ready(self.basalt.formats_in_use().atlas),
			))
			.unwrap();

		self.unparker.unpark();
		response.wait_for_response()
	}

	pub fn load_image_from_bytes(
		&self,
		cache_id: SubImageCacheID,
		cache_ctrl: AtlasCacheCtrl,
		bytes: Vec<u8>,
	) -> Result<AtlasCoords, String> {
		self.load_image(cache_id, cache_ctrl, Image::load_from_bytes(&bytes)?)
	}

	pub fn load_image_from_path<P: AsRef<Path>>(
		&self,
		cache_ctrl: AtlasCacheCtrl,
		path: P,
	) -> Result<AtlasCoords, String> {
		let path = path.as_ref();
		let cache_id = SubImageCacheID::Path(path.to_path_buf());

		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}

		self.load_image(cache_id, cache_ctrl, Image::load_from_path(path)?)
	}

	pub fn load_image_from_url<U: AsRef<str>>(
		self: &Arc<Self>,
		cache_ctrl: AtlasCacheCtrl,
		url: U,
	) -> Result<AtlasCoords, String> {
		let url = url.as_ref();
		let cache_id = SubImageCacheID::Url(url.to_string());

		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}

		self.load_image(cache_id, cache_ctrl, Image::load_from_url(url)?)
	}
}

struct SubImage {
	alloc_id: GuillotiereID,
	xywh: [u32; 4],
	img: Image,
	alive: usize,
	cache_ctrl: AtlasCacheCtrl,
}

impl SubImage {
	fn coords(
		&mut self,
		atlas: Arc<Atlas>,
		img_id: AtlasImageID,
		sub_img_id: SubImageID,
	) -> AtlasCoords {
		self.alive += 1;

		AtlasCoords {
			img_id,
			tlwh: [
				self.xywh[0] as f32,
				self.xywh[1] as f32,
				self.xywh[2] as f32,
				self.xywh[3] as f32,
			],
			inner: Some(Arc::new(CoordsInner {
				atlas,
				img_id,
				sub_img_id,
			})),
		}
	}
}

struct AtlasImage {
	atlas: Arc<Atlas>,
	index: usize,
	active: Option<usize>,
	update: Option<usize>,
	views: Vec<AtlasImageView>,
	sub_imgs: HashMap<SubImageID, SubImage>,
	allocator: Guillotiere,
	max_alloc_size: i32,
}

struct AtlasImageView {
	image: Arc<BstImageView>,
	contains: Vec<SubImageID>,
	updatable: bool,
	stale: bool,
	pending_update: bool,
	clear_regions: Vec<[u32; 4]>,
}

struct UpdateExecResult {
	updated: bool,
	pending_update: bool,
}

impl AtlasImage {
	fn new(atlas: Arc<Atlas>, index: usize) -> Self {
		AtlasImage {
			index,
			active: None,
			update: None,
			views: Vec::new(),
			sub_imgs: HashMap::new(),
			max_alloc_size: atlas.basalt.limits().max_image_dimension_2d as _,
			allocator: Guillotiere::with_options([512; 2].into(), &GuillotiereOptions {
				small_size_threshold: ALLOC_MIN,
				large_size_threshold: ALLOC_MAX,
				..GuillotiereOptions::default()
			}),
			atlas,
		}
	}

	fn complete_update(&mut self) -> Option<Arc<BstImageView>> {
		let img_i = match self.update.take() {
			Some(img_i) => {
				self.active = Some(img_i);
				img_i
			},
			None => *self.active.as_ref()?,
		};

		for view in self.views.iter_mut() {
			if view.pending_update {
				view.stale = false;
			} else if view.stale {
				view.image.mark_stale();
			}
		}

		let image = self.views[img_i].image.create_tmp();
		self.views[img_i].updatable = false;
		Some(image)
	}

	fn update(
		&mut self,
		cmd_buf: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
	) -> UpdateExecResult {
		struct ViewUpdate {
			index: usize,
			create: bool,
			resize: bool,
			cur_dim: [u32; 2],
			set_dim: [u32; 2],
		}

		let mut view_updates: Vec<ViewUpdate> = Vec::new();
		let mut pending_update = false;
		let min_dim = self.minium_size();

		if self.views.is_empty() {
			view_updates.push(ViewUpdate {
				index: 0,
				create: true,
				resize: false,
				cur_dim: [0; 2],
				set_dim: min_dim,
			});

			self.update = Some(0);
		} else {
			for (i, view) in self.views.iter_mut().enumerate() {
				let mut require_update = false;

				for sub_img_id in self.sub_imgs.keys() {
					if !view.contains.contains(sub_img_id) {
						require_update = true;
						break;
					}
				}

				if require_update {
					if view.updatable
						&& (self.active.is_none() || *self.active.as_ref().unwrap() != i)
					{
						let cur_dim = view.image.dimensions().width_height();

						view_updates.push(ViewUpdate {
							index: i,
							create: false,
							resize: cur_dim != min_dim,
							cur_dim,
							set_dim: min_dim,
						});

						view.pending_update = true;
					} else {
						view.stale = true;
						pending_update = true;
					}
				}
			}

			if view_updates.is_empty() {
				if pending_update
					&& (self.active.is_none()
						|| self.views[*self.active.as_ref().unwrap()].stale)
				{
					if self.views.len() < ATLAS_IMAGE_COUNT {
						let index = self.views.len();
						self.update = Some(index);

						view_updates.push(ViewUpdate {
							index,
							create: true,
							resize: false,
							cur_dim: [0; 2],
							set_dim: min_dim,
						});
					} else {
						return UpdateExecResult {
							updated: false,
							pending_update: true,
						};
					}
				} else {
					return UpdateExecResult {
						updated: false,
						pending_update: false,
					};
				}
			}
		}

		assert!(!view_updates.is_empty());
		self.update = Some(view_updates[0].index);

		for ViewUpdate {
			index,
			create,
			resize,
			cur_dim,
			set_dim,
		} in view_updates
		{
			let new_image = if create || resize {
				let image = BstImageView::from_storage(
					StorageImage::with_usage(
						self.atlas.basalt.device(),
						VkImgDimensions::Dim2d {
							width: set_dim[0],
							height: set_dim[1],
							array_layers: 1,
						},
						self.atlas.basalt.formats_in_use().atlas,
						VkImageUsage {
							transfer_src: true,
							transfer_dst: true,
							sampled: true,
							..VkImageUsage::none()
						},
						ImageCreateFlags::none(),
						vec![self
							.atlas
							.basalt
							.secondary_graphics_queue_ref()
							.unwrap_or_else(|| self.atlas.basalt.graphics_queue_ref())
							.family()],
					)
					.unwrap(),
				)
				.unwrap();

				let atlas = self.atlas.clone();
				let atlas_image_i = self.index;

				image.set_drop_fn(Some(Arc::new(move || {
					atlas
						.cmd_send
						.send(Command::TemporaryViewDropped(atlas_image_i, index))
						.unwrap();

					atlas.unparker.unpark();
				})));

				Some(image)
			} else {
				None
			};

			let sto_img = if create {
				let image = new_image.unwrap();

				cmd_buf
					.clear_color_image(ClearColorImageInfo {
						clear_value: ClearColorValue::Uint([0; 4]),
						..ClearColorImageInfo::image(image.clone())
					})
					.unwrap();

				self.views.push(AtlasImageView {
					image: image.clone(),
					contains: Vec::new(),
					updatable: true,
					stale: true,
					pending_update: true,
					clear_regions: Vec::new(),
				});

				image
			} else if resize {
				let new_image = new_image.unwrap();
				let old_image = self.views.get(index).unwrap().image.clone();

				cmd_buf
					.copy_image(CopyImageInfo {
						regions: smallvec![ImageCopy {
							src_subresource: old_image.subresource_layers(),
							dst_subresource: new_image.subresource_layers(),
							extent: [cur_dim[0], cur_dim[1], 1],
							..ImageCopy::default()
						}],
						..CopyImageInfo::images(old_image.clone(), new_image.clone())
					})
					.unwrap();

				let r_w = set_dim[0] - cur_dim[0];
				let r_h = cur_dim[1];
				let b_w = set_dim[0];
				let b_h = set_dim[1] - cur_dim[1];
				let mut zero_buf_len = std::cmp::max(r_w * r_h * 4, b_w * b_h * 4);

				for [_, _, w, h] in self.views[index].clear_regions.iter() {
					let size_needed = w * h;

					if size_needed > zero_buf_len {
						zero_buf_len = size_needed;
					}
				}

				if self.atlas.basalt.formats_in_use().atlas.components()[0] == 16 {
					zero_buf_len *= 2;
				}

				if zero_buf_len > 0 {
					let zero_buf: Arc<CpuAccessibleBuffer<[u8]>> =
						CpuAccessibleBuffer::from_iter(
							self.atlas.basalt.device(),
							VkBufferUsage {
								transfer_src: true,
								..VkBufferUsage::none()
							},
							false,
							(0..zero_buf_len).into_iter().map(|_| 0),
						)
						.unwrap();

					let mut regions = Vec::new();

					if r_w * r_h > 0 {
						regions.push(BufferImageCopy {
							buffer_offset: 0,
							buffer_row_length: r_w,
							buffer_image_height: r_h,
							image_subresource: new_image.subresource_layers(),
							image_offset: [cur_dim[0], 0, 0],
							image_extent: [r_w, r_h, 1],
							..BufferImageCopy::default()
						});
					}

					if b_w * b_h > 0 {
						regions.push(BufferImageCopy {
							buffer_offset: 0,
							buffer_row_length: b_w,
							buffer_image_height: b_h,
							image_subresource: new_image.subresource_layers(),
							image_offset: [0, cur_dim[1], 0],
							image_extent: [b_w, b_h, 1],
							..BufferImageCopy::default()
						});
					}

					for [x, y, w, h] in self.views[index].clear_regions.drain(..) {
						regions.push(BufferImageCopy {
							buffer_offset: 0,
							buffer_row_length: w,
							buffer_image_height: h,
							image_subresource: new_image.subresource_layers(),
							image_offset: [x, y, 0],
							image_extent: [w, h, 1],
							..BufferImageCopy::default()
						});
					}

					cmd_buf
						.copy_buffer_to_image(CopyBufferToImageInfo {
							regions: SmallVec::from_vec(regions),
							..CopyBufferToImageInfo::buffer_image(zero_buf, new_image.clone())
						})
						.unwrap();
				}

				self.views[index].image = new_image.clone();
				new_image
			} else {
				self.views[index].image.clone()
			};

			let mut upload_data: Vec<u8> = Vec::new();
			let mut copy_cmds = Vec::new();
			let mut copy_cmds_imt = Vec::new();
			let mut copy_cmds_bst = Vec::new();

			for (sub_img_id, sub_img) in &self.sub_imgs {
				if !self.views[index].contains.contains(sub_img_id) {
					assert!(sub_img.img.atlas_ready);

					match &sub_img.img.data {
						sid @ ImageData::D8(_) | sid @ ImageData::D16(_) => {
							let sid_bytes = sid.as_bytes();
							assert!(!sid_bytes.is_empty());
							let s = upload_data.len() as u64;
							upload_data.extend_from_slice(sid_bytes);

							copy_cmds.push((
								s,
								upload_data.len() as u64,
								sub_img.xywh[0],
								sub_img.xywh[1],
								sub_img.xywh[2],
								sub_img.xywh[3],
							));

							self.views[index].contains.push(*sub_img_id);
						},
						ImageData::Imt(view) => {
							assert!(ImageType::Raw == sub_img.img.ty);

							copy_cmds_imt.push((
								view.clone(),
								sub_img.xywh[0],
								sub_img.xywh[1],
								sub_img.xywh[2],
								sub_img.xywh[3],
							));

							self.views[index].contains.push(*sub_img_id);
						},
						ImageData::Bst(view) => {
							assert!(ImageType::Raw == sub_img.img.ty);

							copy_cmds_bst.push((
								view.clone(),
								sub_img.xywh[0],
								sub_img.xywh[1],
								sub_img.xywh[2],
								sub_img.xywh[3],
							));

							self.views[index].contains.push(*sub_img_id);
						},
					}
				}
			}

			if !upload_data.is_empty() {
				let upload_buf: Arc<CpuAccessibleBuffer<[u8]>> =
					CpuAccessibleBuffer::from_iter(
						self.atlas.basalt.device(),
						VkBufferUsage {
							transfer_src: true,
							..VkBufferUsage::none()
						},
						false,
						upload_data.into_iter(),
					)
					.unwrap();

				let mut regions = Vec::with_capacity(copy_cmds.len());

				for (s, _e, x, y, w, h) in copy_cmds {
					regions.push(BufferImageCopy {
						buffer_offset: s,
						buffer_row_length: w,
						buffer_image_height: h,
						image_subresource: sto_img.subresource_layers(),
						image_offset: [x, y, 0],
						image_extent: [w, h, 1],
						..BufferImageCopy::default()
					});
				}

				cmd_buf
					.copy_buffer_to_image(CopyBufferToImageInfo {
						regions: SmallVec::from_vec(regions),
						..CopyBufferToImageInfo::buffer_image(
							upload_buf.clone(),
							sto_img.clone(),
						)
					})
					.unwrap();
			}

			for (v, x, y, w, h) in copy_cmds_imt {
				if v.format() == sto_img.format() {
					cmd_buf
						.copy_image(CopyImageInfo {
							regions: smallvec![ImageCopy {
								src_subresource: v.subresource_layers(),
								dst_subresource: sto_img.subresource_layers(),
								dst_offset: [x, y, 0],
								extent: [w, h, 1],
								..ImageCopy::default()
							},],
							..CopyImageInfo::images(v, sto_img.clone())
						})
						.unwrap();
				} else {
					cmd_buf
						.blit_image(BlitImageInfo {
							regions: smallvec![ImageBlit {
								src_subresource: v.subresource_layers(),
								dst_subresource: sto_img.subresource_layers(),
								src_offsets: [[0; 3], [w, h, 1],],
								dst_offsets: [[x, y, 0], [x + w, y + h, 1],],
								..ImageBlit::default()
							},],
							..BlitImageInfo::images(v, sto_img.clone())
						})
						.unwrap();
				}
			}

			for (v, x, y, w, h) in copy_cmds_bst {
				if v.format() == sto_img.format() {
					cmd_buf
						.copy_image(CopyImageInfo {
							regions: smallvec![ImageCopy {
								src_subresource: v.subresource_layers(),
								dst_subresource: sto_img.subresource_layers(),
								dst_offset: [x, y, 0],
								extent: [w, h, 1],
								..ImageCopy::default()
							},],
							..CopyImageInfo::images(v, sto_img.clone())
						})
						.unwrap();
				} else {
					cmd_buf
						.blit_image(BlitImageInfo {
							regions: smallvec![ImageBlit {
								src_subresource: v.subresource_layers(),
								dst_subresource: sto_img.subresource_layers(),
								src_offsets: [[0; 3], [w, h, 1],],
								dst_offsets: [[x, y, 0], [x + w, y + h, 1],],
								..ImageBlit::default()
							},],
							..BlitImageInfo::images(v, sto_img.clone())
						})
						.unwrap();
				}
			}
		}

		UpdateExecResult {
			updated: true,
			pending_update,
		}
	}

	fn minium_size(&self) -> [u32; 2] {
		let [w, h]: [i32; 2] = self.allocator.size().into();
		[w as _, h as _]
	}

	fn try_allocate(
		&mut self,
		image: Image,
		cache_ctrl: AtlasCacheCtrl,
		sub_img_id: SubImageID,
	) -> Result<AtlasCoords, Image> {
		let alloc_size =
			[image.dims.w as i32 + ALLOC_PAD_2X, image.dims.h as i32 + ALLOC_PAD_2X].into();

		let alloc = match self.allocator.allocate(alloc_size) {
			Some(alloc) => alloc,
			None => {
				if alloc_size.width.max(alloc_size.height) > ALLOC_MAX {
					return Err(image);
				}

				let [cur_w, cur_h]: [i32; 2] = self.allocator.size().into();
				let try_w = (alloc_size.width + cur_w).min(self.max_alloc_size);
				let try_h = (alloc_size.height + cur_h).min(self.max_alloc_size);
				self.allocator.grow([try_w, try_h].into());

				match self.allocator.allocate(alloc_size) {
					Some(alloc) => alloc,
					None => return Err(image),
				}
			},
		};

		let mut sub_img = SubImage {
			alloc_id: alloc.id,
			xywh: [
				(alloc.rectangle.min.x + ALLOC_PAD) as u32,
				(alloc.rectangle.min.y + ALLOC_PAD) as u32,
				image.dims.w,
				image.dims.h,
			],
			img: image,
			alive: 0,
			cache_ctrl,
		};

		let coords =
			sub_img.coords(self.atlas.clone(), (self.index + 1) as AtlasImageID, sub_img_id);
		self.sub_imgs.insert(sub_img_id, sub_img);
		Ok(coords)
	}
}
