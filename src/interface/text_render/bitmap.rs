pub use super::font::{BstFont,BstFontWeight};
pub use super::glyph::{BstGlyph,BstGlyphRaw,BstGlyphPos,BstGlyphGeo,BstGlyphPoint};
pub use super::error::{BstTextError,BstTextErrorSrc,BstTextErrorTy};
pub use super::script::{BstTextScript,BstTextLang};
use crate::atlas::{Coords,Image,ImageType,ImageDims,ImageData,SubImageCacheID};
use ordered_float::OrderedFloat;
use std::sync::Arc;
use crate::Basalt;
use crate::shaders::{glyph_base_fs,glyph_post_fs,square_vs};
use vulkano::descriptor::descriptor_set::PersistentDescriptorSet;
use vulkano::descriptor::PipelineLayoutAbstract;
use vulkano::sampler::Sampler;
use vulkano::framebuffer::Framebuffer;
use vulkano::format::Format;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::framebuffer::Subpass;
use vulkano::pipeline::viewport::Viewport;
use vulkano::image::ImageUsage;
use vulkano::image::attachment::AttachmentImage;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;
use vulkano::pipeline::input_assembly::PrimitiveTopology;

#[derive(Default, Copy, Clone)]
struct ShaderVert {
	position: [f32; 2],
}

vulkano::impl_vertex!(ShaderVert, position);

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyphBitmap {
	pub glyph_raw: Arc<BstGlyphRaw>,
	pub width: u32,
	pub height: u32,
	pub bearing_x: f32,
	pub bearing_y: f32,
	pub coords: Coords,
	pub data: Vec<Vec<f32>>,
	pub lines: Vec<(BstGlyphPoint, BstGlyphPoint)>,
}

impl BstGlyphBitmap {
	pub fn new(glyph_raw: Arc<BstGlyphRaw>) -> BstGlyphBitmap {
		let bearing_x = glyph_raw.min_x - 1.0;
		let bearing_y = glyph_raw.font.ascender - glyph_raw.max_y - 1.0;
		let width = (glyph_raw.max_x.ceil() - glyph_raw.min_x.ceil()) as u32 + 2;
		let height = (glyph_raw.max_y.ceil() - glyph_raw.min_y.ceil()) as u32 + 2;
		
		let mut data = Vec::with_capacity(width as usize);
		data.resize_with(width as usize, || {
			let mut col = Vec::with_capacity(height as usize);
			col.resize(height as usize, 0.0);
			col
		});
		
		BstGlyphBitmap {
			width,
			height,
			bearing_x,
			bearing_y,
			data,
			glyph_raw,
			coords: Coords::none(),
			lines: Vec::new(),
		}
	}
	
	pub fn atlas_cache_id(&self) -> SubImageCacheID {
		SubImageCacheID::BstGlyph(
			self.glyph_raw.font.atlas_iden(),
			OrderedFloat::from(self.glyph_raw.font_height),
			self.glyph_raw.index
		)
	}
	
	pub fn create_atlas_image(&mut self, basalt: &Arc<Basalt>) -> Result<(), BstTextError> {
		if self.width == 0 || self.height == 0 {
			return Ok(());
		}
	
		let data_len = (self.width * self.height) as usize;
		let mut data = Vec::with_capacity(data_len);
		data.resize(data_len, 0_u8);
		
		for x in 0..(self.width as usize) {
			for y in 0..(self.height as usize) {
				data[(self.width as usize * (self.height as usize - 1 - y)) + x] =
					(self.data[x][y] * u8::max_value() as f32).round() as u8;
			}
		}
		
		let atlas_image = Image::new(
			ImageType::LMono,
			ImageDims {
				w: self.width,
				h: self.height
			},
			ImageData::D8(data)
		).map_err(|e| BstTextError::src_and_ty(BstTextErrorSrc::Bitmap, BstTextErrorTy::Other(e)))?;
		
		self.coords = basalt.atlas_ref().load_image(self.atlas_cache_id(), atlas_image)
			.map_err(|e| BstTextError::src_and_ty(BstTextErrorSrc::Bitmap, BstTextErrorTy::Other(e)))?;
			
		Ok(())
	}
	
	pub fn draw_gpu(&mut self, basalt: &Arc<Basalt>) -> Result<(), BstTextError> {
		if self.width == 0 || self.height == 0 {
			return Ok(());
		}
		
		let glyph_base_fs = glyph_base_fs::Shader::load(basalt.device()).unwrap();
		let square_vs = square_vs::Shader::load(basalt.device()).unwrap();
		let glyph_post_fs = glyph_post_fs::Shader::load(basalt.device()).unwrap();
		
		let square_buf = CpuAccessibleBuffer::from_iter(basalt.device(), BufferUsage::all(), [
			ShaderVert { position: [-1.0, -1.0] },
			ShaderVert { position: [1.0, -1.0] },
			ShaderVert { position: [1.0, 1.0] },
			ShaderVert { position: [1.0, 1.0] },
			ShaderVert { position: [-1.0, 1.0] },
			ShaderVert { position: [-1.0, -1.0] }
		].iter().cloned()).unwrap();
		
		let mut line_data = glyph_base_fs::ty::LineData {
			lines: [[0.0; 4]; 256],
			ray_dirs: [[0.0; 4]; 8],
			count: 0,
			width: self.width as f32,
			height: self.height as f32,
		};
		
		for (pt_a, pt_b) in &self.lines {
			let i = line_data.count;
			line_data.lines[i as usize] = [
				pt_a.x - self.glyph_raw.min_x + (self.glyph_raw.min_x.ceil() - self.glyph_raw.min_x) + 1.0,
				pt_a.y - self.glyph_raw.min_y + (self.glyph_raw.min_y.ceil() - self.glyph_raw.min_y) + 1.0,
				pt_b.x - self.glyph_raw.min_x + (self.glyph_raw.min_x.ceil() - self.glyph_raw.min_x) + 1.0,
				pt_b.y - self.glyph_raw.min_y + (self.glyph_raw.min_y.ceil() - self.glyph_raw.min_y) + 1.0
			];
			line_data.count += 1;
		}
		
		for i in 0..line_data.ray_dirs.len() {
			let rad = (i as f32 * (360.0 / line_data.ray_dirs.len() as f32)).to_radians();
			line_data.ray_dirs[i] = [rad.cos(), rad.sin(), 0.0, 0.0];
		}
		
		let line_data_buf = CpuAccessibleBuffer::from_data(basalt.device(), BufferUsage::all(), line_data).unwrap();
		
		let p1_out_image = AttachmentImage::with_usage(
			basalt.device(),
			[self.width, self.height],
			Format::R8Unorm,
			ImageUsage {
				transfer_source: true,
				color_attachment: true,
				sampled: true,
				.. vulkano::image::ImageUsage::none()
			}
		).unwrap();
		
		let render_pass = Arc::new(
			vulkano::single_pass_renderpass!(
				basalt.device(),
				attachments: {
					color: {
						load: Clear,
						store: Store,
						format: Format::R8Unorm,
						samples: 1,
					}
				},
				pass: {
					color: [color],
					depth_stencil: {}
				}
			).unwrap()
		);
		
		let pipeline = Arc::new(
			GraphicsPipeline::start()
				.vertex_input_single_buffer::<ShaderVert>()
				.vertex_shader(square_vs.main_entry_point(), ())
				.fragment_shader(glyph_base_fs.main_entry_point(), ())
				.primitive_topology(PrimitiveTopology::TriangleList)
				.render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [self.width as f32, self.height as f32],
				}))
				.depth_stencil_disabled()
				.build(basalt.device()).unwrap()
		);
		
		let set = PersistentDescriptorSet::start(pipeline.descriptor_set_layout(0).unwrap().clone())
			.add_buffer(line_data_buf.clone()).unwrap()
			.build().unwrap();

		let framebuffer = Arc::new(
			Framebuffer::start(render_pass.clone())
				.add(p1_out_image.clone()).unwrap()
				.build().unwrap()
		);

		AutoCommandBufferBuilder::primary_one_time_submit(
			basalt.device(),
			basalt.graphics_queue_ref().family()
		).unwrap()
			.begin_render_pass(
				framebuffer.clone(),
				false,
				vec![[0.0].into()]
			).unwrap()
			.draw(
				pipeline.clone(),
				&vulkano::command_buffer::DynamicState::none(),
				square_buf.clone(),
				set,
				()
			).unwrap()
			.end_render_pass().unwrap()
			.build().unwrap()
			.execute(basalt.graphics_queue()).unwrap()
			.then_signal_fence_and_flush().unwrap()
			.wait(None).unwrap();
		
		let p2_out_image = AttachmentImage::with_usage(
			basalt.device(),
			[self.width, self.height],
			Format::R8Unorm,
			ImageUsage {
				transfer_source: true,
				color_attachment: true,
				.. vulkano::image::ImageUsage::none()
			}
		).unwrap();
		
		let render_pass = Arc::new(
			vulkano::single_pass_renderpass!(
				basalt.device(),
				attachments: {
					color: {
						load: Clear,
						store: Store,
						format: Format::R8Unorm,
						samples: 1,
					}
				},
				pass: {
					color: [color],
					depth_stencil: {}
				}
			).unwrap()
		);
		
		let pipeline = Arc::new(
			GraphicsPipeline::start()
				.vertex_input_single_buffer::<ShaderVert>()
				.vertex_shader(square_vs.main_entry_point(), ())
				.viewports_dynamic_scissors_irrelevant(1)
				.fragment_shader(glyph_post_fs.main_entry_point(), ())
				.render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [self.width as f32, self.height as f32],
				}))
				.depth_stencil_disabled()
				.build(basalt.device()).unwrap()
		);

		let framebuffer = Arc::new(
			Framebuffer::start(render_pass.clone())
				.add(p2_out_image.clone()).unwrap()
				.build().unwrap()
		);
		
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
		
		let set = PersistentDescriptorSet::start(pipeline.descriptor_set_layout(0).unwrap().clone())
			.add_sampled_image(p1_out_image.clone(), sampler.clone()).unwrap()
			.build().unwrap();
		
		AutoCommandBufferBuilder::primary_one_time_submit(
			basalt.device(),
			basalt.graphics_queue_ref().family()
		).unwrap()
			.begin_render_pass(
				framebuffer.clone(),
				false,
				vec![[0.0].into()]
			).unwrap()
			.draw(
				pipeline.clone(),
				&vulkano::command_buffer::DynamicState::none(),
				square_buf.clone(),
				set,
				()
			).unwrap()
			.end_render_pass().unwrap()
			.build().unwrap()
			.execute(basalt.graphics_queue()).unwrap()
			.then_signal_fence_and_flush().unwrap()
			.wait(None).unwrap();
			
		let buffer_out = CpuAccessibleBuffer::from_iter(
			basalt.device(),
			BufferUsage::all(),
			(0 .. self.width * self.height).map(|_| 0u8)
		).unwrap();
		
		AutoCommandBufferBuilder::primary_one_time_submit(
			basalt.device(),
			basalt.graphics_queue_ref().family()
		).unwrap()
			.copy_image_to_buffer(p2_out_image.clone(), buffer_out.clone()).unwrap()
			.build().unwrap()
			.execute(basalt.graphics_queue()).unwrap()
			.then_signal_fence_and_flush().unwrap()
			.wait(None).unwrap();
		
		let buf_read = buffer_out.read().unwrap();
		
		for (y, chunk) in buf_read.chunks(self.width as usize).enumerate() {
			for (x, val) in chunk.iter().enumerate() {
				self.data[x][y] = *val as f32 / u8::max_value() as f32;
			}
		}
		
		Ok(())
	}
	
	pub fn create_outline(&mut self) {
		let glyph_raw = self.glyph_raw.clone();
		
		for geometry in &glyph_raw.geometry {
			self.draw_geometry(geometry);
		}
	}
	
	pub fn draw_geometry(&mut self, geo: &BstGlyphGeo) {
		match geo {
			&BstGlyphGeo::Line(ref points) => self.draw_line(&points[0], &points[1]),
			&BstGlyphGeo::Curve(ref points) => self.draw_curve(&points[0], &points[1], &points[2])
		}
	}
	
	pub fn draw_line(
		&mut self,
		point_a: &BstGlyphPoint,
		point_b: &BstGlyphPoint
	) {
		self.lines.push((point_a.clone(), point_b.clone()));
	}
	
	pub fn draw_curve(
		&mut self,
		point_a: &BstGlyphPoint,
		point_b: &BstGlyphPoint,
		point_c: &BstGlyphPoint
	) {
		let steps = 3_usize;
		let mut last = point_a.clone();
		
		for s in 0..=steps {
			let t = s as f32 / steps as f32;
			let next = point_a.lerp(t, point_b).lerp(t, &point_b.lerp(t, point_c));
			self.draw_line(&last, &next);
			last = next;
		}
	}
}
