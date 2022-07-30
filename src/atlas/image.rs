use crate::image_view::BstImageView;
use ::image as img;
use ilmenite::ImtImageView;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use vulkano::format::{Format as VkFormat, NumericType as VkFormatType};
use vulkano::image::{ImageAccess, ImageDimensions as VkImgDimensions, SampleCount};

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
	pub(super) fn as_bytes(&self) -> &[u8] {
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
	pub(super) ty: ImageType,
	pub(super) dims: ImageDims,
	pub(super) data: ImageData,
	pub(super) atlas_ready: bool,
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

	pub fn load_from_bytes(bytes: &[u8]) -> Result<Self, String> {
		let format = match img::guess_format(bytes) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to guess image type for data: {}", e)),
		};

		let (w, h, data) = match img::load_from_memory(bytes) {
			Ok(image) => (image.width(), image.height(), image.to_rgba16().into_vec()),
			Err(e) => return Err(format!("Failed to read image: {}", e)),
		};

		let image_type = match format {
			img::ImageFormat::Jpeg => ImageType::SRGBA,
			_ => ImageType::LRGBA,
		};

		Image::new(
			image_type,
			ImageDims {
				w,
				h,
			},
			ImageData::D16(data),
		)
		.map_err(|e| format!("Invalid Image: {}", e))
	}

	pub fn load_from_path<P: Into<PathBuf>>(path: P) -> Result<Self, String> {
		let path_buf = path.into();

		let mut handle = match File::open(path_buf) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to open file: {}", e)),
		};

		let mut bytes = Vec::new();

		if let Err(e) = handle.read_to_end(&mut bytes) {
			return Err(format!("Failed to read file: {}", e));
		}

		Self::load_from_bytes(&bytes)
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

	pub(super) fn atlas_ready(self, format: VkFormat) -> Self {
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
