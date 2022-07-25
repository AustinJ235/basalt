use crate::image_view::BstImageView;
use crate::{misc, Basalt};
use crossbeam::deque::{Injector, Steal};
use crossbeam::sync::{Parker, Unparker};
use guillotiere::{
	AllocId as GuillotiereID, AllocatorOptions as GuillotiereOptions,
	AtlasAllocator as Guillotiere,
};
use ilmenite::{ImtImageView, ImtWeight};
use image;
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use smallvec::{smallvec, SmallVec};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fmt, thread};
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage as VkBufferUsage;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, BlitImageInfo, BufferImageCopy, ClearColorImageInfo,
	CommandBufferUsage, CopyBufferToImageInfo, CopyImageInfo, ImageBlit, ImageCopy,
	PrimaryAutoCommandBuffer, PrimaryCommandBuffer,
};
use vulkano::format::{ClearColorValue, Format as VkFormat, NumericType as VkFormatType};
use vulkano::image::immutable::ImmutableImage;
use vulkano::image::{
	ImageAccess, ImageCreateFlags, ImageDimensions as VkImgDimensions,
	ImageUsage as VkImageUsage, MipmapsCount, SampleCount, StorageImage,
};
use vulkano::sampler::{Sampler, SamplerCreateInfo};
use vulkano::sync::GpuFuture;

const PRINT_UPDATE_TIME: bool = false;

#[inline(always)]
fn convert_8b_to_f32(v: u8) -> f32 {
	v as f32 / u8::max_value() as f32
}

#[inline(always)]
fn convert_f32_to_8b(v: f32) -> u8 {
	(v * u8::max_value() as f32).clamp(0.0, u8::max_value() as f32).trunc() as u8
}

#[inline(always)]
fn convert_16b_to_f32(v: u16) -> f32 {
	v as f32 / u16::max_value() as f32
}

#[inline(always)]
fn convert_f32_to_16b(v: f32) -> u16 {
	(v * u16::max_value() as f32).clamp(0.0, u16::max_value() as f32).trunc() as u16
}

#[inline(always)]
fn convert_lin_to_std(v: f32) -> f32 {
	(v.powf(1.0 / 2.4) * 1.005) - 0.055
}

#[inline(always)]
fn convert_std_to_lin(v: f32) -> f32 {
	if v < 0.04045 {
		v / 12.92
	} else {
		((v + 0.055) / 1.055).powf(2.4)
	}
}

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

		atlas.cmd_queue.push(Command::Dropped(*img_id, *sub_img_id));

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageDims {
	pub w: u32,
	pub h: u32,
}

#[derive(Clone)]
pub enum ImageData {
	D8(Vec<u8>),
	D16(Vec<u16>),
	Imt(Arc<ImtImageView>),
	Bst(Arc<BstImageView>),
}

impl ImageData {
	fn as_bytes(&self) -> &[u8] {
		match self {
			ImageData::D8(data) => data.as_slice(),
			ImageData::D16(data) => unsafe {
				std::slice::from_raw_parts(data.as_ptr() as _, data.len() * 2)
			},
			_ => unreachable!(),
		}
	}
}

impl fmt::Debug for ImageData {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ImageData::D8(_) => write!(f, "ImageData::D8"),
			ImageData::D16(_) => write!(f, "ImageData::D16"),
			ImageData::Imt(_) => write!(f, "ImageData::Imt"),
			ImageData::Bst(_) => write!(f, "ImageData::Bst"),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageType {
	LRGBA,
	LRGB,
	LMono,
	SRGBA,
	SRGB,
	SMono,
	YUV444,
	Raw,
}

impl ImageType {
	pub fn components(&self) -> usize {
		match self {
			&ImageType::LRGBA => 4,
			&ImageType::LRGB => 3,
			&ImageType::LMono => 1,
			&ImageType::SRGBA => 4,
			&ImageType::SRGB => 3,
			&ImageType::SMono => 1,
			&ImageType::YUV444 => 3,
			&ImageType::Raw => 0,
		}
	}
}

#[derive(Debug, Clone)]
pub struct Image {
	ty: ImageType,
	dims: ImageDims,
	data: ImageData,
	atlas_ready: bool,
}

fn image_atlas_compatible(img: &dyn ImageAccess) -> Result<(), String> {
	if img.samples() != SampleCount::Sample1 {
		return Err(String::from("Source image must not be multisampled. "));
	}

	match img.format().type_color() {
		Some(color_type) =>
			match color_type {
				VkFormatType::UNORM => (),
				_ => return Err(format!("Source image must be an unorm numeric type.")),
			},
		None => return Err(format!("Source image must be of a color format.")),
	}

	Ok(())
}

impl Image {
	pub fn new(ty: ImageType, dims: ImageDims, data: ImageData) -> Result<Image, String> {
		if ty == ImageType::Raw {
			return Err(format!(
				"This method can not create raw images. Use `from_imt` or `from_bst`."
			));
		}

		let expected_len = dims.w as usize * dims.h as usize * ty.components();

		if expected_len == 0 {
			return Err(format!("Image can't be empty."));
		}

		let actual_len = match &data {
			ImageData::D8(d) => d.len(),
			ImageData::D16(d) => d.len(),
			_ => return Err(format!("`Image::new()` can only create D8 & D16 images.")),
		};

		if actual_len != expected_len {
			return Err(format!(
				"Data len doesn't match the provided dimensions! {} != {}",
				actual_len, expected_len
			));
		}

		Ok(Image {
			ty,
			dims,
			data,
			atlas_ready: false,
		})
	}

	pub fn from_imt(imt: Arc<ImtImageView>) -> Result<Image, String> {
		let dims = match imt.dimensions() {
			VkImgDimensions::Dim2d {
				width,
				height,
				array_layers,
			} => {
				if array_layers != 1 {
					return Err(format!("array_layers != 1"));
				}

				ImageDims {
					w: width,
					h: height,
				}
			},
			_ => {
				return Err(format!("Only 2d images are supported."));
			},
		};

		image_atlas_compatible(&imt)?;

		Ok(Image {
			ty: ImageType::Raw,
			dims,
			data: ImageData::Imt(imt),
			atlas_ready: true,
		})
	}

	pub fn from_bst(bst: Arc<BstImageView>) -> Result<Image, String> {
		let dims = match bst.dimensions() {
			VkImgDimensions::Dim2d {
				width,
				height,
				array_layers,
			} => {
				if array_layers != 1 {
					return Err(format!("array_layers != 1"));
				}

				ImageDims {
					w: width,
					h: height,
				}
			},
			_ => {
				return Err(format!("Only 2d images are supported."));
			},
		};

		image_atlas_compatible(&bst)?;

		Ok(Image {
			ty: ImageType::Raw,
			dims,
			data: ImageData::Bst(bst),
			atlas_ready: true,
		})
	}

	pub fn into_data(self) -> ImageData {
		self.data
	}

	fn to_rgba(mut self, to_16bit: bool, to_linear: bool) -> Self {
		let from_16bit = match &self.data {
			ImageData::D8(_) => false,
			ImageData::D16(_) => true,
			_ => return self,
		};

		let from_linear = match &self.ty {
			ImageType::LRGBA => true,
			ImageType::LRGB => true,
			ImageType::LMono => true,
			ImageType::SRGBA => false,
			ImageType::SRGB => false,
			ImageType::SMono => false,
			ImageType::YUV444 => false,
			_ => return self,
		};

		// Check if image is already the desired type and depth
		if from_16bit == to_16bit {
			if to_linear {
				if self.ty == ImageType::LRGBA {
					return self;
				}
			} else if self.ty == ImageType::SRGBA {
				return self;
			}
		}

		// Check if just remap is required
		// TODO: This is lossless and should be preferred.
		// if self.ty != ImageType::YUV444 && from_16bit == to_16bit && from_linear == to_linear

		let mut data: Vec<f32> = match self.data {
			ImageData::D8(data) => data.into_iter().map(|v| convert_8b_to_f32(v)).collect(),
			ImageData::D16(data) => data.into_iter().map(|v| convert_16b_to_f32(v)).collect(),
			_ => unreachable!(),
		};

		if from_linear != to_linear && self.ty != ImageType::YUV444 {
			if from_linear {
				for val in data.iter_mut() {
					*val = convert_lin_to_std(*val);
				}
			} else {
				for val in data.iter_mut() {
					*val = convert_std_to_lin(*val);
				}
			}
		}

		let data: Vec<f32> = match self.ty {
			ImageType::LRGBA | ImageType::SRGBA => data,
			ImageType::LRGB | ImageType::SRGB => {
				let mut mapped = Vec::with_capacity((data.len() / 3) * 4);

				for val in data.into_iter() {
					mapped.push(val);

					if mapped.len() % 4 == 2 {
						mapped.push(1.0);
					}
				}

				mapped
			},
			ImageType::LMono | ImageType::SMono => {
				let mut mapped = Vec::with_capacity(data.len() * 4);

				for val in data.into_iter() {
					for _ in 0..4 {
						mapped.push(val);
					}
				}

				mapped
			},
			ImageType::YUV444 => {
				let mut mapped: Vec<f32> = Vec::with_capacity((data.len() / 3) * 4);

				for chunk in data.chunks_exact(3) {
					if let [y, u, v] = chunk {
						let mut srgb = [
							y + (1.402 * (v - 0.5)),
							y + (0.344 * (u - 0.5)) - (0.714 * (v - 0.5)),
							y + (1.772 * (u - 0.5)),
						];

						if to_linear {
							for val in srgb.iter_mut() {
								*val = convert_std_to_lin(*val);
							}
						}

						mapped.extend_from_slice(&srgb);
						mapped.push(1.0);
					} else {
						unreachable!()
					}
				}

				mapped
			},
			_ => unreachable!(),
		};

		self.data = if to_16bit {
			ImageData::D16(data.into_iter().map(|v| convert_f32_to_16b(v)).collect())
		} else {
			ImageData::D8(data.into_iter().map(|v| convert_f32_to_8b(v)).collect())
		};

		self.ty = if to_linear {
			ImageType::LRGBA
		} else {
			ImageType::SRGBA
		};

		self
	}

	#[inline(always)]
	pub fn to_16b_srgba(self) -> Self {
		self.to_rgba(true, false)
	}

	#[inline(always)]
	pub fn to_16b_lrgba(self) -> Self {
		self.to_rgba(true, true)
	}

	#[inline(always)]
	pub fn to_8b_srgba(self) -> Self {
		self.to_rgba(false, false)
	}

	#[inline(always)]
	pub fn to_8b_lrgba(self) -> Self {
		self.to_rgba(false, true)
	}

	fn atlas_ready(self, format: VkFormat) -> Self {
		if self.atlas_ready {
			return self;
		}

		let mut image = match format {
			VkFormat::R16G16B16A16_UNORM => self.to_rgba(true, true),
			VkFormat::R8G8B8A8_UNORM => self.to_rgba(false, true),
			VkFormat::B8G8R8A8_UNORM => {
				let mut image = self.to_rgba(false, true);

				match &mut image.data {
					ImageData::D8(data) =>
						for chunk in data.chunks_exact_mut(4) {
							if let [r, _, b, _] = chunk {
								std::mem::swap(r, b);
							} else {
								unreachable!()
							}
						},
					ImageData::D16(_) => unreachable!(),
					_ => (),
				}

				image
			},
			VkFormat::A8B8G8R8_UNORM_PACK32 => {
				let mut image = self.to_rgba(false, true);

				match &mut image.data {
					ImageData::D8(data) =>
						for chunk in data.chunks_exact_mut(4) {
							if let [r, g, b, a] = chunk {
								std::mem::swap(r, a);
								std::mem::swap(g, b);
							}
						},
					ImageData::D16(_) => unreachable!(),
					_ => (),
				}

				image
			},
			_ => panic!("Unexpected Atlas Format: {:?}", format),
		};

		image.atlas_ready = true;
		image
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
	cmd_queue: Injector<Command>,
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

		let atlas_ret = Arc::new(Atlas {
			basalt,
			unparker,
			linear_sampler,
			nearest_sampler,
			empty_image,
			cmd_queue: Injector::new(),
			image_views: Mutex::new(None),
		});

		let atlas = atlas_ret.clone();

		thread::spawn(move || {
			let mut iter_start;
			let mut atlas_images: Vec<AtlasImage> = Vec::new();
			let mut sub_img_id_count = 1;
			let mut cached_map: HashMap<SubImageCacheID, (AtlasImageID, SubImageID)> =
				HashMap::new();
			// TODO: Check before new allocations
			let mut pending_removal: HashMap<(AtlasImageID, SubImageID), Instant> =
				HashMap::new();
			let mut execute = false;

			loop {
				iter_start = Instant::now();
				let mut cmds = Vec::new();
				let mut got_cmd = false;
				let mut ready_on_execute: Vec<Arc<dyn CommandResponseAbstract>> = Vec::new();

				loop {
					let cmd = match atlas.cmd_queue.steal() {
						Steal::Empty => break,
						Steal::Retry => continue,
						Steal::Success(cmd) => cmd,
					};

					got_cmd = true;

					match cmd {
						// TODO: Should proccess all drops before allocation
						Command::Dropped(img_id, sub_img_id) => {
							if img_id > 0 {
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

									for (cm_id, (cm_img_id, cm_sub_img_id)) in cached_map.iter()
									{
										if *cm_sub_img_id == sub_img_id && *cm_img_id == img_id
										{
											cache_id = cm_id.clone();
											break;
										}
									}

									if cache_id != SubImageCacheID::None {
										// TODO: Remove debug message
										println!("[Atlas]: Removing {:?}", cache_id);
										cached_map.remove(&cache_id).unwrap();
									}

									drop(sub_img);

									let SubImage {
										alloc_id,
										..
									} = atlas_img.sub_imgs.remove(&sub_img_id).unwrap();

									// TODO: Zero this region or all new regions padding?
									atlas_img.con_sub_img.iter_mut().for_each(|contains| {
										contains.retain(|id| *id != sub_img_id)
									});
									atlas_img.allocator.deallocate(alloc_id);
								},
							}
						},
						Command::Upload(response, cache_id, cache_ctrl, image) => {
							let mut coords_op = None;
							let mut image_op = Some(image);

							for (i, atlas_image) in atlas_images.iter_mut().enumerate() {
								match atlas_image.try_allocate(
									image_op.take().unwrap(),
									cache_ctrl,
									(i + 1) as u64,
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
								let mut atlas_image = AtlasImage::new(atlas.basalt.clone());

								match atlas_image.try_allocate(
									image_op.take().unwrap(),
									cache_ctrl,
									(atlas_images.len() + 1) as u64,
									sub_img_id_count,
								) {
									Ok(ok) => {
										coords_op = Some(ok);
									},
									Err(_) => {
										response.respond(Err(format!(
											"Image to big to fit in the atlas."
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
							ready_on_execute.push(response);
						},
						c => cmds.push(c),
					}
				}

				if !got_cmd && !execute {
					parker.park();
					continue;
				}

				for cmd in cmds {
					match cmd {
						Command::Upload(..) | Command::Dropped(..) => unreachable!(),
						Command::CacheIDLookup(response, cache_id) => {
							response.respond(match cached_map.get(&cache_id) {
								Some((img_id, sub_img_id)) =>
									match atlas_images.get_mut(*img_id as usize - 1) {
										Some(atlas_image) =>
											match atlas_image.sub_imgs.get_mut(sub_img_id) {
												Some(sub_image) =>
													Some(sub_image.coords(
														atlas.clone(),
														*img_id,
														*sub_img_id,
													)),
												None => None,
											},
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
														match atlas_image
															.sub_imgs
															.get_mut(sub_img_id)
														{
															Some(sub_image) =>
																Some(sub_image.coords(
																	atlas.clone(),
																	*img_id,
																	*sub_img_id,
																)),
															None => None,
														},
													None => None,
												},
											None => None,
										}
									})
									.collect(),
							);
						},
					}
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

				execute = false;
				let mut sizes = Vec::new();

				for atlas_image in &mut atlas_images {
					let res = atlas_image.update(cmd_buf);
					cmd_buf = res.0;
					sizes.push((res.2, res.3));

					if res.1 {
						execute = res.1;
					}
				}

				if execute {
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

				for response in ready_on_execute {
					response.ready_response();
				}

				if PRINT_UPDATE_TIME && execute {
					let mut out = format!(
						"Atlas Updated in {:.1} ms. ",
						iter_start.elapsed().as_micros() as f64 / 1000.0
					);

					for (i, (w, h)) in sizes.into_iter().enumerate() {
						out.push_str(format!("{}:{}x{} ", i + 1, w, h).as_str());
					}

					out.pop();
					println!("{}", out);
				}
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
						let image_buffer: image::ImageBuffer<image::Rgba<u8>, _> =
							image::ImageBuffer::from_raw(width, height, data).unwrap();
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

						let image_buffer: image::ImageBuffer<image::Rgba<u16>, _> =
							image::ImageBuffer::from_raw(width, height, data).unwrap();
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
		self.cmd_queue.push(Command::CacheIDLookup(response.clone(), cache_id));
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
		self.cmd_queue.push(Command::BatchCacheIDLookup(response.clone(), cache_ids));
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

		self.cmd_queue.push(Command::Upload(
			response.clone(),
			cache_id,
			cache_ctrl,
			image.atlas_ready(self.basalt.formats_in_use().atlas),
		));

		self.unparker.unpark();
		response.wait_for_response()
	}

	pub fn load_image_from_bytes(
		&self,
		cache_id: SubImageCacheID,
		cache_ctrl: AtlasCacheCtrl,
		bytes: Vec<u8>,
	) -> Result<AtlasCoords, String> {
		let format = match image::guess_format(bytes.as_slice()) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to guess image type for data: {}", e)),
		};

		let (w, h, data) = match image::load_from_memory(bytes.as_slice()) {
			Ok(image) => (image.width(), image.height(), image.to_rgba16().into_vec()),
			Err(e) => return Err(format!("Failed to read image: {}", e)),
		};

		let image_type = match format {
			image::ImageFormat::Jpeg => ImageType::SRGBA,
			_ => ImageType::LRGBA,
		};

		let image = Image::new(
			image_type,
			ImageDims {
				w,
				h,
			},
			ImageData::D16(data),
		)
		.map_err(|e| format!("Invalid Image: {}", e))?;

		self.load_image(
			cache_id,
			cache_ctrl,
			image.atlas_ready(self.basalt.formats_in_use().atlas),
		)
	}

	pub fn load_image_from_path<P: Into<PathBuf>>(
		&self,
		cache_ctrl: AtlasCacheCtrl,
		path: P,
	) -> Result<AtlasCoords, String> {
		let path_buf = path.into();
		let cache_id = SubImageCacheID::Path(path_buf.clone());

		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}

		let mut handle = match File::open(path_buf) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to open file: {}", e)),
		};

		let mut bytes = Vec::new();

		if let Err(e) = handle.read_to_end(&mut bytes) {
			return Err(format!("Failed to read file: {}", e));
		}

		self.load_image_from_bytes(cache_id, cache_ctrl, bytes)
	}

	pub fn load_image_from_url<U: AsRef<str>>(
		self: &Arc<Self>,
		cache_ctrl: AtlasCacheCtrl,
		url: U,
	) -> Result<AtlasCoords, String> {
		let cache_id = SubImageCacheID::Url(url.as_ref().to_string());

		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}

		let bytes = match misc::http::get_bytes(&url) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to retreive url data: {}", e)),
		};

		self.load_image_from_bytes(cache_id, cache_ctrl, bytes)
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
	basalt: Arc<Basalt>,
	active: Option<usize>,
	update: Option<usize>,
	sto_imgs: Vec<Arc<BstImageView>>,
	sub_imgs: HashMap<SubImageID, SubImage>,
	con_sub_img: Vec<Vec<SubImageID>>,
	allocator: Guillotiere,
	max_alloc_size: i32,
}

impl AtlasImage {
	fn new(basalt: Arc<Basalt>) -> Self {
		AtlasImage {
			active: None,
			update: None,
			sto_imgs: Vec::new(),
			sub_imgs: HashMap::new(),
			con_sub_img: Vec::new(),
			max_alloc_size: basalt.limits().max_image_dimension_2d as _,
			allocator: Guillotiere::with_options([512; 2].into(), &GuillotiereOptions {
				small_size_threshold: ALLOC_MIN,
				large_size_threshold: ALLOC_MAX,
				..GuillotiereOptions::default()
			}),
			basalt,
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

		Some(self.sto_imgs[img_i].create_tmp())
	}

	// TODO: Borrow cmd_buf
	fn update(
		&mut self,
		mut cmd_buf: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, bool, u32, u32) {
		self.update = None;
		let mut found_op = None;
		let [min_img_w, min_img_h] = self.minium_size();
		let mut cur_img_w = 0;
		let mut cur_img_h = 0;
		let mut resize = false;

		for (i, sto_img) in self.sto_imgs.iter().enumerate() {
			if found_op.is_none() && sto_img.temporary_views() == 0 {
				if let VkImgDimensions::Dim2d {
					width,
					height,
					..
				} = sto_img.dimensions()
				{
					self.update = Some(i);
					found_op = Some((i, sto_img.clone()));
					cur_img_w = width;
					cur_img_h = height;
					resize = width < min_img_w || height < min_img_h;
				} else {
					unreachable!()
				}
			}
		}

		if found_op.is_none() && self.sto_imgs.len() > 3 {
			return (cmd_buf, false, cur_img_w, cur_img_h);
		}

		if found_op.is_none() || resize {
			let img_i = match found_op.as_ref() {
				Some((img_i, _)) => *img_i,
				None => self.sto_imgs.len(),
			};

			let image: Arc<BstImageView> = BstImageView::from_storage(
				StorageImage::with_usage(
					self.basalt.device(),
					VkImgDimensions::Dim2d {
						width: min_img_w,
						height: min_img_h,
						array_layers: 1,
					},
					self.basalt.formats_in_use().atlas,
					VkImageUsage {
						transfer_src: true,
						transfer_dst: true,
						sampled: true,
						..VkImageUsage::none()
					},
					ImageCreateFlags::none(),
					vec![self
						.basalt
						.secondary_graphics_queue_ref()
						.unwrap_or_else(|| self.basalt.graphics_queue_ref())
						.family()],
				)
				.unwrap(),
			)
			.unwrap();

			if img_i < self.sto_imgs.len() {
				// TODO: Is clear the whole image faster than clearing only the new parts?
				// cmd_buf
				//	.clear_color_image(ClearColorImageInfo {
				//		clear_value: ClearColorValue::Uint([0; 4]),
				//		..ClearColorImageInfo::image(image.clone())
				//	})
				//	.unwrap();

				let r_w = min_img_w - cur_img_w;
				let r_h = cur_img_h;
				let b_w = min_img_w;
				let b_h = min_img_h - cur_img_h;
				let mut zero_buf_len = std::cmp::max(r_w * r_h * 4, b_w * b_h * 4);

				if self.basalt.formats_in_use().atlas.components()[0] == 16 {
					zero_buf_len *= 2;
				}

				if zero_buf_len > 0 {
					let zero_buf: Arc<CpuAccessibleBuffer<[u8]>> =
						CpuAccessibleBuffer::from_iter(
							self.basalt.device(),
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
							image_subresource: image.subresource_layers(),
							image_offset: [cur_img_w, 0, 0],
							image_extent: [r_w, r_h, 1],
							..BufferImageCopy::default()
						});
					}

					if b_w * b_h > 0 {
						regions.push(BufferImageCopy {
							buffer_offset: 0,
							buffer_row_length: b_w,
							buffer_image_height: b_h,
							image_subresource: image.subresource_layers(),
							image_offset: [0, cur_img_h, 0],
							image_extent: [b_w, b_h, 1],
							..BufferImageCopy::default()
						});
					}

					cmd_buf
						.copy_buffer_to_image(CopyBufferToImageInfo {
							regions: SmallVec::from_vec(regions),
							..CopyBufferToImageInfo::buffer_image(zero_buf, image.clone())
						})
						.unwrap();
				}

				cmd_buf
					.copy_image(CopyImageInfo {
						regions: smallvec![ImageCopy {
							src_subresource: self.sto_imgs[img_i].subresource_layers(),
							dst_subresource: image.subresource_layers(),
							extent: [cur_img_w, cur_img_h, 1],
							..ImageCopy::default()
						}],
						..CopyImageInfo::images(self.sto_imgs[img_i].clone(), image.clone())
					})
					.unwrap();

				self.sto_imgs[img_i].mark_stale();
				self.sto_imgs[img_i] = image.clone();
				found_op = Some((img_i, image));
				cur_img_w = min_img_w;
				cur_img_h = min_img_h;
			} else {
				cmd_buf
					.clear_color_image(ClearColorImageInfo {
						clear_value: ClearColorValue::Uint([0; 4]),
						..ClearColorImageInfo::image(image.clone())
					})
					.unwrap();

				self.sto_imgs.push(image.clone());
				self.con_sub_img.push(Vec::new());
				found_op = Some((img_i, image));
				self.update = Some(img_i);
				cur_img_w = min_img_w;
				cur_img_h = min_img_h;
			}
		}

		let (img_i, sto_img) = found_op.unwrap();
		let mut upload_data: Vec<u8> = Vec::new();
		let mut copy_cmds = Vec::new();
		let mut copy_cmds_imt = Vec::new();
		let mut copy_cmds_bst = Vec::new();

		for (sub_img_id, sub_img) in &self.sub_imgs {
			if !self.con_sub_img[img_i].contains(sub_img_id) {
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

						self.con_sub_img[img_i].push(*sub_img_id);
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

						self.con_sub_img[img_i].push(*sub_img_id);
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

						self.con_sub_img[img_i].push(*sub_img_id);
					},
				}
			}
		}

		if copy_cmds.is_empty() && copy_cmds_imt.is_empty() && copy_cmds_bst.is_empty() {
			self.update = None;
			return (cmd_buf, false, cur_img_w, cur_img_h);
		}

		if !upload_data.is_empty() {
			let upload_buf: Arc<CpuAccessibleBuffer<[u8]>> = CpuAccessibleBuffer::from_iter(
				self.basalt.device(),
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
					..CopyBufferToImageInfo::buffer_image(upload_buf.clone(), sto_img.clone())
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

		(cmd_buf, true, cur_img_w, cur_img_h)
	}

	fn minium_size(&self) -> [u32; 2] {
		let [w, h]: [i32; 2] = self.allocator.size().into();
		[w as _, h as _]
	}

	fn try_allocate(
		&mut self,
		image: Image,
		cache_ctrl: AtlasCacheCtrl,
		atlas_img_id: AtlasImageID,
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

		let coords = sub_img.coords(self.basalt.atlas(), atlas_img_id, sub_img_id);
		self.sub_imgs.insert(sub_img_id, sub_img);
		Ok(coords)
	}
}
