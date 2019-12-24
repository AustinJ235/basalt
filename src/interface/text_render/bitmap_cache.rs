use std::collections::BTreeMap;
use super::bitmap::{BstGlyphBitmap,ShaderVert};
use super::error::BstTextError;
use super::glyph::{BstGlyph,BstGlyphRaw};
use std::sync::Arc;
use crate::Basalt;
use crate::shaders::{glyph_base_fs,glyph_post_fs,square_vs};
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::sampler::Sampler;

pub struct BstGlyphBitmapCache {
	cached: BTreeMap<u16, BstGlyphBitmap>,
	pub(super) basalt: Arc<Basalt>,
	pub(super) square_vs: square_vs::Shader,
	pub(super) glyph_base_fs: glyph_base_fs::Shader,
	pub(super) glyph_post_fs: glyph_post_fs::Shader,
	pub(super) square_buf: Arc<CpuAccessibleBuffer<[ShaderVert]>>,
	pub(super) sampler: Arc<Sampler>,
}

impl BstGlyphBitmapCache {
	pub fn new(basalt: Arc<Basalt>) -> Self {
		let square_vs = square_vs::Shader::load(basalt.device()).unwrap();
		let glyph_base_fs = glyph_base_fs::Shader::load(basalt.device()).unwrap();
		let glyph_post_fs = glyph_post_fs::Shader::load(basalt.device()).unwrap();
		
		// TODO: Use DeviceLocalBuffer
		let square_buf = CpuAccessibleBuffer::from_iter(
			basalt.device(),
			BufferUsage {
				vertex_buffer: true,
				.. BufferUsage::none()
			},
			[
				ShaderVert { position: [-1.0, -1.0] },
				ShaderVert { position: [1.0, -1.0] },
				ShaderVert { position: [1.0, 1.0] },
				ShaderVert { position: [1.0, 1.0] },
				ShaderVert { position: [-1.0, 1.0] },
				ShaderVert { position: [-1.0, -1.0] }
			].iter().cloned()
		).unwrap();
		
		let sampler = Sampler::new(
			basalt.device(),
			vulkano::sampler::Filter::Nearest,
			vulkano::sampler::Filter::Nearest,
			vulkano::sampler::MipmapMode::Nearest,
			vulkano::sampler::SamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack),
			vulkano::sampler::SamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack),
			vulkano::sampler::SamplerAddressMode::ClampToBorder(
				vulkano::sampler::BorderColor::IntTransparentBlack),
			0.0, 1.0, 0.0, 1000.0
		).unwrap();
	
		BstGlyphBitmapCache {
			basalt,
			square_vs,
			glyph_base_fs,
			glyph_post_fs,
			square_buf,
			sampler,
			cached: BTreeMap::new()
		}
	}

	pub fn bitmap_for_glyph(&mut self, glyph: &BstGlyph) -> Result<&BstGlyphBitmap, BstTextError> {
		self.bitmap_for_glyph_raw(&glyph.glyph_raw)
	}
	
	pub fn bitmap_for_glyph_raw(&mut self, glyph_raw: &Arc<BstGlyphRaw>) -> Result<&BstGlyphBitmap, BstTextError> {
		if self.cached.get(&glyph_raw.index).is_none() {
			let mut bitmap = BstGlyphBitmap::new(glyph_raw.clone());
			bitmap.create_outline();
			bitmap.draw_gpu(self)?;
			bitmap.create_atlas_image(&self.basalt)?;
			self.cached.insert(glyph_raw.index, bitmap);
		}
		
		Ok(self.cached.get(&glyph_raw.index).unwrap())
	}
}
