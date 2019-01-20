#![allow(warnings)]

use vulkano::sampler::MipmapMode;
use vulkano::sampler::UnnormalizedSamplerAddressMode;
use std::sync::atomic::{self,AtomicU64};
use vulkano::sampler::BorderColor;
use std::path::PathBuf;

#[derive(Clone,Copy,PartialEq,Eq,Debug,Hash)]
pub struct SubImageID(u64);

#[derive(Clone,Copy,PartialEq,Eq,Debug,Hash)]
pub struct AtlasImageID(u64);

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum SubImageCacheID {
	Path(PathBuf),
	Glyph(u32, u64),
	None
}

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub struct SamplerDesc {
	pub mipmap_mode: MipmapMode,
	pub u_addr_mode: UnnormalizedSamplerAddressMode,
	pub v_addr_mode: UnnormalizedSamplerAddressMode,
}

impl Default for SamplerDesc {
	fn default() -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Linear,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
		}
	}
}

impl SamplerDesc {
	pub fn linear_clamp_edge() -> Self {
		Self::default()
	}

	pub fn nearest_clamp_edge() -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Nearest,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
		}
	}
	
	pub fn linear_clamp_border(border_color: BorderColor) -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Linear,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
		}
	}
	
	pub fn nearest_clamp_border(border_color: BorderColor) -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Nearest,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
		}
	}
}

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
pub struct Coords {
	pub image: AtlasImageID,
	pub sub_image: SubImageID,
	pub x: u32,
	pub y: u32,
	pub w: u32,
	pub h: u32,
}

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum DataType {
	LRGBA_8B,
	LRGB_8B,
	LR_8B,
	SRGBA_8B,
	SRGB_8B,
	SR_8B,
	YUV_8B,
}

pub enum Data {
	U8(Vec<u8>),
}

pub struct SubImage {
	pub cache_id: SubImageCacheID,
	pub coords: Coords,
	pub data_type: DataType,
	pub data: Data,
}

pub struct Image {

}

pub struct Atlas {
	sub_image_counter: AtomicU64,
	atlas_image_counter: AtomicU64,
}

impl Atlas {
	pub fn is_cached(cache_id: SubImageCacheID) -> bool {
		unimplemented!()
	}
	
	pub fn load_image_from_path(path_buf: PathBuf, sampler_desc: SamplerDesc) -> SubImageID {
		unimplemented!()
	}
	
	pub fn load_image(cache_id: SubImageCacheID, ty: DataType, sampler_desc: SamplerDesc, data: Data) -> SubImageID {
		unimplemented!()
	}
	
	pub fn image_coords(id: SubImageID) -> Coords {
		unimplemented!()
	}
	
	pub fn cached_image_coords(cache_id: SubImageCacheID) -> Coords {
		unimplemented!()
	}
}

