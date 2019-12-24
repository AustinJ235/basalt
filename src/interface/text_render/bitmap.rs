pub use super::font::{BstFont,BstFontWeight};
pub use super::glyph::{BstGlyph,BstGlyphRaw,BstGlyphPos,BstGlyphGeo,BstGlyphPoint};
pub use super::error::{BstTextError,BstTextErrorSrc,BstTextErrorTy};
pub use super::script::{BstTextScript,BstTextLang};
pub use super::bitmap_cache::BstGlyphBitmapCache;
use crate::atlas::{Coords,Image,ImageType,ImageDims,ImageData,SubImageCacheID};
use ordered_float::OrderedFloat;
use std::sync::Arc;
use crate::Basalt;
use crate::shaders::glyph_base_fs;
use vulkano::descriptor::descriptor_set::PersistentDescriptorSet;
use vulkano::descriptor::PipelineLayoutAbstract;
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
pub(super) struct ShaderVert {
	pub position: [f32; 2],
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
		let bearing_y = glyph_raw.font.ascender - glyph_raw.max_y.floor() - 1.0;
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
	
	pub fn draw_gpu(&mut self, cache: &BstGlyphBitmapCache) -> Result<(), BstTextError> {
		if self.width == 0 || self.height == 0 {
			return Ok(());
		}
		
		let mut line_data = glyph_base_fs::ty::LineData {
			lines: [[0.0; 4]; 1024],
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
		
		let line_data_buf = CpuAccessibleBuffer::from_data(cache.basalt.device(), BufferUsage::all(), line_data).unwrap();
		
		let p1_out_image = AttachmentImage::with_usage(
			cache.basalt.device(),
			[self.width, self.height],
			Format::R8Unorm,
			ImageUsage {
				transfer_source: true,
				color_attachment: true,
				sampled: true,
				.. vulkano::image::ImageUsage::none()
			}
		).unwrap();
		
		let p1_render_pass = Arc::new(
			vulkano::single_pass_renderpass!(
				cache.basalt.device(),
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
		
		let p1_pipeline = Arc::new(
			GraphicsPipeline::start()
				.vertex_input_single_buffer::<ShaderVert>()
				.vertex_shader(cache.square_vs.main_entry_point(), ())
				.fragment_shader(cache.glyph_base_fs.main_entry_point(), ())
				.primitive_topology(PrimitiveTopology::TriangleList)
				.render_pass(Subpass::from(p1_render_pass.clone(), 0).unwrap())
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [self.width as f32, self.height as f32],
				}))
				.depth_stencil_disabled()
				.build(cache.basalt.device()).unwrap()
		);
		
		let p1_set = PersistentDescriptorSet::start(p1_pipeline.descriptor_set_layout(0).unwrap().clone())
			.add_buffer(line_data_buf.clone()).unwrap()
			.add_buffer(cache.sample_data_buf.clone()).unwrap()
			.add_buffer(cache.ray_data_buf.clone()).unwrap()
			.build().unwrap();

		let p1_framebuffer = Arc::new(
			Framebuffer::start(p1_render_pass.clone())
				.add(p1_out_image.clone()).unwrap()
				.build().unwrap()
		);
		
		let p2_render_pass = Arc::new(
			vulkano::single_pass_renderpass!(
				cache.basalt.device(),
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
		
		let p2_pipeline = Arc::new(
			GraphicsPipeline::start()
				.vertex_input_single_buffer::<ShaderVert>()
				.vertex_shader(cache.square_vs.main_entry_point(), ())
				.viewports_dynamic_scissors_irrelevant(1)
				.fragment_shader(cache.glyph_post_fs.main_entry_point(), ())
				.render_pass(Subpass::from(p2_render_pass.clone(), 0).unwrap())
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [self.width as f32, self.height as f32],
				}))
				.depth_stencil_disabled()
				.build(cache.basalt.device()).unwrap()
		);
		
		let p2_out_image = AttachmentImage::with_usage(
			cache.basalt.device(),
			[self.width, self.height],
			Format::R8Unorm,
			ImageUsage {
				transfer_source: true,
				color_attachment: true,
				.. vulkano::image::ImageUsage::none()
			}
		).unwrap();

		let p2_framebuffer = Arc::new(
			Framebuffer::start(p2_render_pass.clone())
				.add(p2_out_image.clone()).unwrap()
				.build().unwrap()
		);
		
		let p2_set = PersistentDescriptorSet::start(p2_pipeline.descriptor_set_layout(0).unwrap().clone())
			.add_sampled_image(p1_out_image.clone(), cache.sampler.clone()).unwrap()
			.build().unwrap();
			
		let buffer_out = CpuAccessibleBuffer::from_iter(
			cache.basalt.device(),
			BufferUsage::all(),
			(0 .. self.width * self.height).map(|_| 0u8)
		).unwrap();
			
		AutoCommandBufferBuilder::primary_one_time_submit(
			cache.basalt.device(),
			cache.basalt.graphics_queue_ref().family()
		).unwrap()
			.begin_render_pass(
				p1_framebuffer.clone(),
				false,
				vec![[0.0].into()]
			).unwrap()
			.draw(
				p1_pipeline.clone(),
				&vulkano::command_buffer::DynamicState::none(),
				cache.square_buf.clone(),
				p1_set,
				()
			).unwrap()
			.end_render_pass().unwrap()
			.begin_render_pass(
				p2_framebuffer.clone(),
				false,
				vec![[0.0].into()]
			).unwrap()
			.draw(
				p2_pipeline.clone(),
				&vulkano::command_buffer::DynamicState::none(),
				cache.square_buf.clone(),
				p2_set,
				()
			).unwrap()
			.end_render_pass().unwrap()
			.copy_image_to_buffer(p2_out_image.clone(), buffer_out.clone()).unwrap()
			.build().unwrap()
			.execute(cache.basalt.graphics_queue()).unwrap()
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
		let mut length = 0.0;
		let mut last_point = point_a.clone();
		let mut steps = 10_usize;
		
		for s in 1..=steps {
			let t = s as f32 / steps as f32;
			let next_point = BstGlyphPoint {
				x: ((1.0-t).powi(2)*point_a.x)+(2.0*(1.0-t)*t*point_b.x)+(t.powi(2)*point_c.x),
				y: ((1.0-t).powi(2)*point_a.y)+(2.0*(1.0-t)*t*point_b.y)+(t.powi(2)*point_c.y)
			};
			
			length += last_point.dist(&next_point);
			last_point = next_point;
		}
		
		steps = (length * 2.0).ceil() as usize;
		
		if steps < 3 {
			steps = 3;
		}
		
		last_point = point_a.clone();
		
		for s in 1..=steps {
			let t = s as f32 / steps as f32;
			let next_point = BstGlyphPoint {
				x: ((1.0-t).powi(2)*point_a.x)+(2.0*(1.0-t)*t*point_b.x)+(t.powi(2)*point_c.x),
				y: ((1.0-t).powi(2)*point_a.y)+(2.0*(1.0-t)*t*point_b.y)+(t.powi(2)*point_c.y)
			};
			
			self.draw_line(&last_point, &next_point);
			last_point = next_point;
		}
	}
}
