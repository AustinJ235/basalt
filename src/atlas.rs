use crate::image_view::BstImageView;
use crate::{misc, Basalt};
use crossbeam::deque::{Injector, Steal};
use crossbeam::sync::{Parker, Unparker};
use ilmenite::{ImtImageView, ImtWeight};
use image::{self, GenericImageView};
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use std::{fmt, thread};
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::{BufferAccess, BufferUsage as VkBufferUsage};
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, CommandBufferUsage, PrimaryAutoCommandBuffer,
	PrimaryCommandBuffer,
};
use vulkano::format::NumericType as VkFormatType;
use vulkano::image::immutable::ImmutableImage;
use vulkano::image::{
	ImageAccess, ImageCreateFlags, ImageDimensions as VkImgDimensions,
	ImageUsage as VkImageUsage, MipmapsCount, SampleCount, StorageImage,
};
use vulkano::sampler::Sampler;
use vulkano::sync::GpuFuture;

const PRINT_UPDATE_TIME: bool = false;

#[inline]
fn srgb_to_linear(mut f: f32) -> u16 {
	if f < 0.04045 {
		f /= 12.92;
	} else {
		f = ((f + 0.055) / 1.055).powf(2.4)
	}

	f = (f * u16::max_value() as f32).round();

	if f > u16::max_value() as f32 {
		f = u16::max_value() as f32;
	} else if f < 0.0 {
		f = 0.0;
	}

	f as u16
}

#[inline]
fn linear_to_srgb(v: f32) -> u16 {
	let mut v = (((v.powf(1.0 / 2.4) * 1.005) - 0.055) * u16::max_value() as f32).round();

	if v > u16::max_value() as f32 {
		v = u16::max_value() as f32;
	} else if v < 0.0 {
		v = 0.0;
	}

	v as u16
}

#[inline]
fn f32_to_u16(mut v: f32) -> u16 {
	v *= u16::max_value() as f32;

	if v > u16::max_value() as f32 {
		v = u16::max_value() as f32;
	} else if v < 0.0 {
		v = 0.0;
	}

	v.trunc() as u16
}

const CELL_WIDTH: u32 = 32;
const CELL_PAD: u32 = 5;

pub type AtlasImageID = u64;
pub type SubImageID = u64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubImageCacheID {
	Path(PathBuf),
	Url(String),
	Glyph(String, ImtWeight, u16, OrderedFloat<f32>),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coords {
	pub img_id: AtlasImageID,
	pub sub_img_id: SubImageID,
	pub x: u32,
	pub y: u32,
	pub w: u32,
	pub h: u32,
}

impl Coords {
	pub fn none() -> Self {
		Coords {
			img_id: 0,
			sub_img_id: 0,
			x: 0,
			y: 0,
			w: 0,
			h: 0,
		}
	}

	pub fn top_left(&self) -> (f32, f32) {
		(self.x as f32, self.y as f32)
	}

	pub fn top_right(&self) -> (f32, f32) {
		((self.x + self.w) as f32, self.y as f32)
	}

	pub fn bottom_left(&self) -> (f32, f32) {
		(self.x as f32, (self.y + self.h) as f32)
	}

	pub fn bottom_right(&self) -> (f32, f32) {
		((self.x + self.w) as f32, (self.y + self.h) as f32)
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
		})
	}

	pub fn into_data(self) -> ImageData {
		self.data
	}

	pub fn to_16b_srgba(self) -> Self {
		let (len, mut data) = match self.data {
			ImageData::D8(data) =>
				(
					data.len() / self.ty.components(),
					Box::new(data.into_iter().map(|v| v as f32 / u8::max_value() as f32))
						as Box<dyn Iterator<Item = f32>>,
				),
			ImageData::D16(data) =>
				(
					data.len() / self.ty.components(),
					Box::new(data.into_iter().map(|v| v as f32 / u16::max_value() as f32))
						as Box<dyn Iterator<Item = f32>>,
				),
			_ => return self,
		};

		let srgba: Vec<u16> = match self.ty {
			ImageType::LRGBA => data.map(|v| linear_to_srgb(v)).collect(),
			ImageType::LRGB => {
				let mut srgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					srgba.push(linear_to_srgb(v));

					if srgba.len() % 4 == 2 {
						srgba.push(u16::max_value());
					}
				}

				srgba
			},
			ImageType::LMono => {
				let mut srgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					let v = linear_to_srgb(v);
					srgba.push(v);
					srgba.push(v);
					srgba.push(v);
					srgba.push(u16::max_value());
				}

				srgba
			},
			ImageType::SMono => {
				let mut srgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					let v = f32_to_u16(v);
					srgba.push(v);
					srgba.push(v);
					srgba.push(v);
					srgba.push(u16::max_value());
				}

				srgba
			},
			ImageType::SRGBA => data.map(|v| f32_to_u16(v)).collect(),
			ImageType::SRGB => {
				let mut srgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					srgba.push(f32_to_u16(v));

					if srgba.len() % 4 == 2 {
						srgba.push(u16::max_value());
					}
				}

				srgba
			},
			ImageType::YUV444 => {
				let mut srgba: Vec<u16> = Vec::with_capacity(len * 4);

				loop {
					let y = match data.next() {
						Some(some) => some,
						None => break,
					};

					let u = data.next().unwrap();
					let v = data.next().unwrap();

					let components = [
						y + (1.402 * (v - 0.5)),
						y + (0.344 * (u - 0.5)) - (0.714 * (v - 0.5)),
						y + (1.772 * (u - 0.5)),
					];

					for v in &components {
						srgba.push(f32_to_u16(*v));
					}

					srgba.push(u16::max_value());
				}

				srgba
			},
			_ => panic!("Unexpected ImageType"),
		};

		Image {
			ty: ImageType::SRGBA,
			dims: self.dims,
			data: ImageData::D16(srgba),
		}
	}

	pub fn to_16b_lrgba(self) -> Self {
		let (len, mut data) = match self.data {
			ImageData::D8(data) =>
				(
					data.len() / self.ty.components(),
					Box::new(data.into_iter().map(|v| v as f32 / u8::max_value() as f32))
						as Box<dyn Iterator<Item = f32>>,
				),
			ImageData::D16(data) =>
				(
					data.len() / self.ty.components(),
					Box::new(data.into_iter().map(|v| v as f32 / u16::max_value() as f32))
						as Box<dyn Iterator<Item = f32>>,
				),
			_ => return self,
		};

		let lrgba: Vec<u16> = match self.ty {
			ImageType::LRGBA => data.map(|v| f32_to_u16(v)).collect(),
			ImageType::LRGB => {
				let mut lrgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					lrgba.push(f32_to_u16(v));

					if lrgba.len() % 4 == 2 {
						lrgba.push(u16::max_value());
					}
				}

				lrgba
			},
			ImageType::LMono => {
				let mut lrgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					lrgba.push(f32_to_u16(v));
					lrgba.push(f32_to_u16(v));
					lrgba.push(f32_to_u16(v));
					lrgba.push(u16::max_value());
				}

				lrgba
			},
			ImageType::SMono => {
				let mut lrgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					let v = srgb_to_linear(v);
					lrgba.push(v);
					lrgba.push(v);
					lrgba.push(v);
					lrgba.push(u16::max_value());
				}

				lrgba
			},
			ImageType::SRGBA => data.map(|v| srgb_to_linear(v)).collect(),
			ImageType::SRGB => {
				let mut lrgba: Vec<u16> = Vec::with_capacity(len * 4);

				for v in data {
					lrgba.push(srgb_to_linear(v));

					if lrgba.len() % 4 == 2 {
						lrgba.push(u16::max_value());
					}
				}

				lrgba
			},
			ImageType::YUV444 => {
				let mut lrgba: Vec<u16> = Vec::with_capacity(len * 4);

				loop {
					let y = match data.next() {
						Some(some) => some,
						None => break,
					};

					let u = data.next().unwrap();
					let v = data.next().unwrap();

					let mut components = [
						y + (1.402 * (v - 0.5)),
						y + (0.344 * (u - 0.5)) - (0.714 * (v - 0.5)),
						y + (1.772 * (u - 0.5)),
					];

					for v in &mut components {
						lrgba.push(srgb_to_linear(*v));
					}

					lrgba.push(u16::max_value());
				}

				lrgba
			},
			_ => panic!("Unexpected ImageType"),
		};

		Image {
			ty: ImageType::LRGBA,
			dims: self.dims,
			data: ImageData::D16(lrgba),
		}
	}
}

enum Command {
	Upload(Arc<CommandResponse<Result<Coords, String>>>, SubImageCacheID, Image),
	CacheIDLookup(Arc<CommandResponse<Option<Coords>>>, SubImageCacheID),
	BatchCacheIDLookup(Arc<CommandResponse<Vec<Option<Coords>>>>, Vec<SubImageCacheID>),
	Delete(SubImageID),
	DeleteCache(SubImageCacheID),
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
		let linear_sampler = Sampler::unnormalized(
			basalt.device(),
			vulkano::sampler::Filter::Linear,
			vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack,
			),
			vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack,
			),
		)
		.unwrap();

		let nearest_sampler = Sampler::unnormalized(
			basalt.device(),
			vulkano::sampler::Filter::Nearest,
			vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack,
			),
			vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack,
			),
		)
		.unwrap();

		let empty_image = BstImageView::from_immutable(
			ImmutableImage::from_iter(
				vec![255, 255, 255, 255].into_iter(),
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
			let mut cached_map = HashMap::new();
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
						Command::Upload(response, cache_id, up_image) => {
							let mut space_op = None;

							for (i, atlas_image) in atlas_images.iter().enumerate() {
								if let Some(region) = atlas_image.find_space_for(&up_image.dims)
								{
									space_op = Some((i + 1, region));
									break;
								}
							}

							if space_op.is_none() {
								let atlas_image = AtlasImage::new(atlas.basalt.clone());

								match atlas_image.find_space_for(&up_image.dims) {
									Some(region) => {
										space_op = Some((atlas_images.len() + 1, region));
									},
									None => {
										response.respond(Err(format!(
											"Image to big to fit in atlas."
										)));
										continue;
									},
								}

								atlas_images.push(atlas_image);
							}

							let (atlas_image_i, region) = space_op.unwrap();
							let sub_img_id = sub_img_id_count;
							sub_img_id_count += 1;

							let coords =
								region.coords(atlas_image_i as u64, sub_img_id, &up_image.dims);

							if cache_id != SubImageCacheID::None {
								cached_map.insert(cache_id.clone(), coords);
							}

							response.set_response(Ok(coords));
							ready_on_execute.push(response);

							atlas_images[atlas_image_i - 1]
								.insert(&region, sub_img_id, coords, up_image);
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
						Command::Upload(..) => unreachable!(),
						Command::Delete(_sub_img_id) => (), // TODO: Implement Deletes
						Command::DeleteCache(_sub_img_cache_id) => (),
						Command::CacheIDLookup(response, cache_id) => {
							response.respond(cached_map.get(&cache_id).cloned());
						},
						Command::BatchCacheIDLookup(response, cache_ids) => {
							response.respond(
								cache_ids
									.into_iter()
									.map(|cache_id| cached_map.get(&cache_id).cloned())
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

	/// Remove a sub image. Currently not implemented.
	pub fn delete_sub_image(&self, sub_img_id: SubImageID) {
		self.cmd_queue.push(Command::Delete(sub_img_id));
	}

	/// Remove a sub cache image. Currently not implemented.
	pub fn delete_sub_cache_image(&self, sub_img_cache_id: SubImageCacheID) {
		self.cmd_queue.push(Command::DeleteCache(sub_img_cache_id));
	}

	/// Obtain coords given a cache id. If doing this in bulk there will be a considerable
	/// performance improvement when using `batch_cache_coords()`.
	pub fn cache_coords(&self, cache_id: SubImageCacheID) -> Option<Coords> {
		let response = CommandResponse::new();
		self.cmd_queue.push(Command::CacheIDLookup(response.clone(), cache_id));
		self.unparker.unpark();
		response.wait_for_response()
	}

	/// Obtain coords for a set of cache ids. This method will be a considerable
	/// improvment for obtaining coords over `cache_coords` where this is done in bulk.
	pub fn batch_cache_coords(&self, cache_ids: Vec<SubImageCacheID>) -> Vec<Option<Coords>> {
		let response = CommandResponse::new();
		self.cmd_queue.push(Command::BatchCacheIDLookup(response.clone(), cache_ids));
		self.unparker.unpark();
		response.wait_for_response()
	}

	pub fn load_image(
		&self,
		cache_id: SubImageCacheID,
		image: Image,
	) -> Result<Coords, String> {
		let response = CommandResponse::new();
		self.cmd_queue.push(Command::Upload(response.clone(), cache_id, image.to_16b_lrgba()));
		self.unparker.unpark();
		response.wait_for_response()
	}

	pub fn load_image_from_bytes(
		&self,
		cache_id: SubImageCacheID,
		bytes: Vec<u8>,
	) -> Result<Coords, String> {
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
		self.load_image(cache_id, image.to_16b_lrgba())
	}

	pub fn load_image_from_path<P: Into<PathBuf>>(&self, path: P) -> Result<Coords, String> {
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

		self.load_image_from_bytes(cache_id, bytes)
	}

	pub fn load_image_from_url<U: AsRef<str>>(
		self: &Arc<Self>,
		url: U,
	) -> Result<Coords, String> {
		let cache_id = SubImageCacheID::Url(url.as_ref().to_string());

		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}

		let bytes = match misc::http::get_bytes(&url) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to retreive url data: {}", e)),
		};

		self.load_image_from_bytes(cache_id, bytes)
	}
}

struct Region {
	x: usize,
	y: usize,
	w: usize,
	h: usize,
}

impl Region {
	fn coords(&self, img_id: AtlasImageID, sub_img_id: SubImageID, dims: &ImageDims) -> Coords {
		Coords {
			img_id,
			sub_img_id,
			x: (self.x as u32 * CELL_WIDTH)
				+ (self.x.checked_sub(1).unwrap_or(0) as u32 * CELL_PAD)
				+ CELL_PAD,
			y: (self.y as u32 * CELL_WIDTH)
				+ (self.y.checked_sub(1).unwrap_or(0) as u32 * CELL_PAD)
				+ CELL_PAD,
			w: dims.w,
			h: dims.h,
		}
	}
}

struct SubImage {
	coords: Coords,
	img: Image,
}

struct AtlasImage {
	basalt: Arc<Basalt>,
	active: Option<usize>,
	update: Option<usize>,
	sto_imgs: Vec<Arc<BstImageView>>,
	sub_imgs: HashMap<SubImageID, SubImage>,
	con_sub_img: Vec<Vec<SubImageID>>,
	alloc_cell_w: usize,
	alloc: Vec<Vec<Option<SubImageID>>>,
}

impl AtlasImage {
	fn new(basalt: Arc<Basalt>) -> Self {
		let max_img_w = basalt.limits().max_image_dimension_2d as f32 + CELL_PAD as f32;
		let alloc_cell_w = (max_img_w / (CELL_WIDTH + CELL_PAD) as f32).floor() as usize;
		let mut alloc = Vec::with_capacity(alloc_cell_w);
		alloc.resize_with(alloc_cell_w, || {
			let mut out = Vec::with_capacity(alloc_cell_w);
			out.resize(alloc_cell_w, None);
			out
		});

		AtlasImage {
			basalt,
			alloc,
			alloc_cell_w,
			active: None,
			update: None,
			sto_imgs: Vec::new(),
			sub_imgs: HashMap::new(),
			con_sub_img: Vec::new(),
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
		let (min_img_w, min_img_h) = self.minium_size();
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
						transfer_source: true,
						transfer_destination: true,
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
				// TODO:	This is a workaround for https://github.com/AustinJ235/basalt/issues/6
				// 			Clearing the whole image is slighly slower than only clearing the parts?
				// 			Although upload a bunch of zeros the gpu and the copying that onto the
				// 			newer portions may be just as slow and zero'ing the whole image.

				// cmd_buf = cmd_buf.clear_color_image(image.clone(), [0_32;
				// 4].into()).unwrap();

				let r_w = min_img_w - cur_img_w;
				let r_h = cur_img_h;
				let mut r_zeros = Vec::new();
				r_zeros.resize((r_w * r_h * 4) as usize, 0);

				let r_buf = CpuAccessibleBuffer::from_iter(
					self.basalt.device(),
					VkBufferUsage {
						transfer_source: true,
						..VkBufferUsage::none()
					},
					false,
					r_zeros.into_iter(),
				)
				.unwrap();

				if r_w > 0 && r_h > 0 {
					cmd_buf
						.copy_buffer_to_image_dimensions(
							r_buf,
							image.clone(),
							[cur_img_w, 0, 0],
							[r_w, r_h, 1],
							0,
							1,
							0,
						)
						.unwrap();
				}

				let b_w = min_img_w;
				let b_h = min_img_h - cur_img_h;
				let mut b_zeros = Vec::new();
				b_zeros.resize((b_w * b_h * 4) as usize, 0);

				let b_buf = CpuAccessibleBuffer::from_iter(
					self.basalt.device(),
					VkBufferUsage {
						transfer_source: true,
						..VkBufferUsage::none()
					},
					false,
					b_zeros.into_iter(),
				)
				.unwrap();

				if b_w > 0 && b_h > 0 {
					cmd_buf
						.copy_buffer_to_image_dimensions(
							b_buf,
							image.clone(),
							[0, cur_img_h, 0],
							[b_w, b_h, 1],
							0,
							1,
							0,
						)
						.unwrap();
				}

				cmd_buf
					.copy_image(
						self.sto_imgs[img_i].clone(),
						[0, 0, 0],
						0,
						0,
						image.clone(),
						[0, 0, 0],
						0,
						0,
						[cur_img_w, cur_img_h, 1],
						1,
					)
					.unwrap();

				self.sto_imgs[img_i].mark_stale();
				self.sto_imgs[img_i] = image.clone();
				found_op = Some((img_i, image));
				cur_img_w = min_img_w;
				cur_img_h = min_img_h;
			} else {
				cmd_buf.clear_color_image(image.clone(), [0_32; 4].into()).unwrap();
				self.sto_imgs.push(image.clone());
				self.con_sub_img.push(Vec::new());
				found_op = Some((img_i, image));
				self.update = Some(img_i);
				cur_img_w = min_img_w;
				cur_img_h = min_img_h;
			}
		}

		let (img_i, sto_img) = found_op.unwrap();
		let mut upload_data = Vec::new();
		let mut copy_cmds = Vec::new();
		let mut copy_cmds_imt = Vec::new();
		let mut copy_cmds_bst = Vec::new();

		for (sub_img_id, sub_img) in &self.sub_imgs {
			if !self.con_sub_img[img_i].contains(sub_img_id) {
				match &sub_img.img.data {
					ImageData::D16(sub_img_data) => {
						assert!(ImageType::LRGBA == sub_img.img.ty);
						assert!(!sub_img_data.is_empty());

						let s = upload_data.len() as u64;
						upload_data.extend_from_slice(&sub_img_data);

						copy_cmds.push((
							s,
							upload_data.len() as u64,
							sub_img.coords.x,
							sub_img.coords.y,
							sub_img.coords.w,
							sub_img.coords.h,
						));

						self.con_sub_img[img_i].push(*sub_img_id);
					},
					ImageData::Imt(view) => {
						assert!(ImageType::Raw == sub_img.img.ty);

						copy_cmds_imt.push((
							view.clone(),
							sub_img.coords.x,
							sub_img.coords.y,
							sub_img.coords.w,
							sub_img.coords.h,
						));

						self.con_sub_img[img_i].push(*sub_img_id);
					},
					ImageData::Bst(view) => {
						assert!(ImageType::Raw == sub_img.img.ty);

						copy_cmds_bst.push((
							view.clone(),
							sub_img.coords.x,
							sub_img.coords.y,
							sub_img.coords.w,
							sub_img.coords.h,
						));

						self.con_sub_img[img_i].push(*sub_img_id);
					},
					_ => unreachable!(),
				}
			}
		}

		if copy_cmds.is_empty() && copy_cmds_imt.is_empty() && copy_cmds_bst.is_empty() {
			self.update = None;
			return (cmd_buf, false, cur_img_w, cur_img_h);
		}

		let upload_buf = CpuAccessibleBuffer::from_iter(
			self.basalt.device(),
			VkBufferUsage {
				transfer_source: true,
				..VkBufferUsage::none()
			},
			false,
			upload_data.into_iter(),
		)
		.unwrap();

		for (s, e, x, y, w, h) in copy_cmds {
			cmd_buf
				.copy_buffer_to_image_dimensions(
					upload_buf.clone().into_buffer_slice().slice(s..e).unwrap(),
					sto_img.clone(),
					[x, y, 0],
					[w, h, 1],
					0,
					1,
					0,
				)
				.unwrap();
		}

		for (v, x, y, w, h) in copy_cmds_imt {
			if v.format() == sto_img.format() {
				cmd_buf
					.copy_image(
						v,
						[0, 0, 0],
						0,
						0,
						sto_img.clone(),
						[x as i32, y as i32, 0],
						0,
						0,
						[w, h, 1],
						1,
					)
					.unwrap();
			} else {
				cmd_buf
					.blit_image(
						v,
						[0, 0, 0],
						[w as i32, h as i32, 1],
						0,
						0,
						sto_img.clone(),
						[x as i32, y as i32, 0],
						[x as i32 + w as i32, y as i32 + h as i32, 1],
						0,
						0,
						1,
						vulkano::sampler::Filter::Nearest,
					)
					.unwrap();
			}
		}

		for (v, x, y, w, h) in copy_cmds_bst {
			if v.format() == sto_img.format() {
				cmd_buf
					.copy_image(
						v,
						[0, 0, 0],
						0,
						0,
						sto_img.clone(),
						[x as i32, y as i32, 0],
						0,
						0,
						[w, h, 1],
						1,
					)
					.unwrap();
			} else {
				cmd_buf
					.blit_image(
						v,
						[0, 0, 0],
						[w as i32, h as i32, 1],
						0,
						0,
						sto_img.clone(),
						[x as i32, y as i32, 0],
						[x as i32 + w as i32, y as i32 + h as i32, 1],
						0,
						0,
						1,
						vulkano::sampler::Filter::Nearest,
					)
					.unwrap();
			}
		}

		(cmd_buf, true, cur_img_w, cur_img_h)
	}

	fn minium_size(&self) -> (u32, u32) {
		let mut min_x = 1;
		let mut min_y = 1;

		for sub_img in self.sub_imgs.values() {
			let x = sub_img.coords.x + sub_img.coords.w;
			let y = sub_img.coords.y + sub_img.coords.h;

			if x > min_x {
				min_x = x;
			}

			if y > min_y {
				min_y = y;
			}
		}

		min_x += CELL_PAD;
		min_y += CELL_PAD;

		(min_x, min_y)
	}

	fn find_space_for(&self, dims: &ImageDims) -> Option<Region> {
		// TODO: Include padding in available space
		let w = (dims.w as f32 / CELL_WIDTH as f32).ceil() as usize;
		let h = (dims.h as f32 / CELL_WIDTH as f32).ceil() as usize;
		let mut cell_pos = None;

		for i in 0..self.alloc_cell_w {
			for j in 0..self.alloc_cell_w {
				let mut fits = true;

				for k in 0..w {
					for l in 0..h {
						match self.alloc.get(i + k).and_then(|xarr| xarr.get(j + l)) {
							Some(cell) =>
								if cell.is_some() {
									fits = false;
									break;
								},
							None => {
								fits = false;
								break;
							},
						}
					}
					if !fits {
						break;
					}
				}

				if fits {
					cell_pos = Some((i, j));
					break;
				}
			}

			if cell_pos.is_some() {
				break;
			}
		}

		let (x, y) = cell_pos?;
		Some(Region {
			x,
			y,
			w,
			h,
		})
	}

	fn insert(&mut self, region: &Region, sub_img_id: SubImageID, coords: Coords, img: Image) {
		for x in region.x..(region.x + region.w) {
			for y in region.y..(region.y + region.h) {
				self.alloc[x][y] = Some(sub_img_id);
			}
		}

		self.sub_imgs.insert(sub_img_id, SubImage {
			coords,
			img,
		});
	}
}
