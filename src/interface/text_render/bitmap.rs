pub use super::font::{BstFont,BstFontWeight};
pub use super::glyph::{BstGlyph,BstGlyphRaw,BstGlyphPos,BstGlyphGeo,BstGlyphPoint};
pub use super::error::{BstTextError,BstTextErrorSrc,BstTextErrorTy};
pub use super::script::{BstTextScript,BstTextLang};
use crate::atlas::{Coords,Image,ImageType,ImageDims,ImageData,SubImageCacheID};
use ordered_float::OrderedFloat;
use std::sync::Arc;
use crate::Basalt;

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyphBitmap {
	pub glyph_raw: Arc<BstGlyphRaw>,
	pub width: u32,
	pub height: u32,
	pub bearing_x: f32,
	pub bearing_y: f32,
	pub data: Vec<Vec<f32>>,
	pub coords: Coords,
	pub lines: Vec<(BstGlyphPoint, BstGlyphPoint)>,
}

impl BstGlyphBitmap {
	pub fn new(glyph_raw: Arc<BstGlyphRaw>) -> BstGlyphBitmap {
		let bearing_x = glyph_raw.min_x.ceil() - 1.0;
		let bearing_y = -1.0;
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
		use vulkano::framebuffer::Framebuffer;
		use vulkano::format::Format;
		use vulkano::command_buffer::AutoCommandBufferBuilder;
		use vulkano::pipeline::GraphicsPipeline;
		use vulkano::framebuffer::Subpass;
		use vulkano::command_buffer::DynamicState;
		use vulkano::pipeline::viewport::Viewport;
		use vulkano::image::ImageUsage;
		use vulkano::image::attachment::AttachmentImage;
		use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
		use vulkano::buffer::BufferUsage;
		use vulkano::command_buffer::CommandBuffer;
		use vulkano::sync::GpuFuture;
		use vulkano::pipeline::input_assembly::PrimitiveTopology;
		
		if self.width == 0 || self.height == 0 {
			return Ok(());
		}
	
		#[derive(Default, Copy, Clone)]
		struct Vertex {
			position: [f32; 2],
		}

		vulkano::impl_vertex!(Vertex, position);
		
		mod vs {
			shader!{
				ty: "vertex",
				src: "
					#version 450
					
					layout(location = 0) in vec2 position;

					void main() {
						gl_Position = vec4(position, 0, 1);
					}
				"
			}
		}
		
		mod fs {
			shader!{
				ty: "fragment",
				src: "
					#version 450
					
					layout(location = 0) out vec4 color;

					void main() {
						color = vec4(1.0);
					}
				"
			}
		}
		
		let vs = vs::Shader::load(basalt.device()).unwrap();
		let fs = fs::Shader::load(basalt.device()).unwrap();
		
		let image = AttachmentImage::with_usage(
			basalt.device(),
			[self.width, self.height],
			Format::R8G8B8A8Unorm,
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
						format: Format::R8G8B8A8Unorm,
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
				.vertex_input_single_buffer::<Vertex>()
				.vertex_shader(vs.main_entry_point(), ())
				.viewports_dynamic_scissors_irrelevant(1)
				.fragment_shader(fs.main_entry_point(), ())
				.primitive_topology(PrimitiveTopology::LineList)
				.polygon_mode_line()
				.depth_stencil_disabled()
				.render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
				.build(basalt.device()).unwrap()
		);

		let framebuffer = Arc::new(
			Framebuffer::start(render_pass.clone())
				.add(image.clone()).unwrap()
				.build().unwrap()
		);
		
		let dynamic_state = DynamicState {
			viewports: Some(vec![Viewport {
				origin: [0.0, 0.0],
				dimensions: [
					self.width as f32,
					self.height as f32,
				],
				depth_range: 0.0 .. 1.0,
			}]),
			.. DynamicState::none()
		};
		
		let width_f = self.glyph_raw.max_x - self.glyph_raw.min_x;
		let height_f = self.glyph_raw.max_y - self.glyph_raw.min_y;
		
		let verts: Vec<_> = self.lines
			.clone()
			.into_iter()
			.flat_map(|(a, b)| vec![
				Vertex {
					position: [a.x, a.y],
				},
				Vertex {
					position: [b.x, b.y],
				}
			]).map(|mut v| {
				v.position[0]  = (((v.position[0] - self.glyph_raw.min_x + 1.0) / self.width as f32) * 2.0) - 1.0;
				v.position[1]  = (((v.position[1] - self.glyph_raw.min_y + 1.0) / self.height as f32) * 2.0) - 1.0;
				v
			}).collect();
		
		let buffer_in = CpuAccessibleBuffer::from_iter(
			basalt.device(),
			BufferUsage::all(), verts.into_iter()
		).unwrap();

		let mut cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(
			basalt.device(),
			basalt.graphics_queue_ref().family()
		).unwrap();
		
		cmd_buf = cmd_buf.begin_render_pass(
			framebuffer.clone(),
			false,
			vec![[0.0, 0.0, 0.0, 0.0].into()]
		).unwrap();
		
		cmd_buf = cmd_buf.draw(pipeline.clone(), &dynamic_state, buffer_in.clone(), (), ()).unwrap();
		cmd_buf = cmd_buf.end_render_pass().unwrap();
		
		cmd_buf
			.build().unwrap()
			.execute(basalt.graphics_queue()).unwrap()
			.then_signal_fence_and_flush().unwrap()
			.wait(None).unwrap();
			
		let buffer_out = CpuAccessibleBuffer::from_iter(
			basalt.device(),
			BufferUsage::all(),
			(0 .. self.width * self.height * 4).map(|_| 0u8)
		).unwrap();
		
		let mut cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(
			basalt.device(),
			basalt.graphics_queue_ref().family()
		).unwrap();
		
		cmd_buf = cmd_buf.copy_image_to_buffer(image.clone(), buffer_out.clone()).unwrap();
		
		cmd_buf
			.build().unwrap()
			.execute(basalt.graphics_queue()).unwrap()
			.then_signal_fence_and_flush().unwrap()
			.wait(None).unwrap();
		
		let buf_read = buffer_out.read().unwrap();
		
		for (y, chunk) in buf_read.chunks(self.width as usize * 4).enumerate() {
			for (x, vals) in chunk.chunks(4).enumerate() {
				self.data[x][y] = vals[0] as f32 / u8::max_value() as f32;
			}
		}
		
		Ok(())
	}
	
	pub fn fill(&mut self) {
		let mut regions = Vec::new();
	
		while let Some((sx, sy)) = {
			let mut seed = None;
		
			for x in 0..(self.width as usize) {
				for y in 0..(self.height as usize) {
					if self.data[x][y] != 1.0 {
						seed = Some((x, y));
						break;
					}
				} if seed.is_some() {
					break;
				}
			}
			
			seed
		} {
			let mut check = Vec::new();
			let mut contains = Vec::new();
			check.push((sx, sy));
			
			while let Some((cx, cy)) = check.pop() {
				if self.data[cx][cy] == 0.0 {
					contains.push((cx, cy));
					self.data[cx][cy] = 1.0;
					
					if cx != 0 {
						check.push((cx-1, cy));
					}
					
					if cx != self.width as usize - 1 {
						check.push((cx+1, cy));
					}
					
					if cy != 0 {
						check.push((cx, cy-1));
					}
					
					if cy != self.height as usize - 1 {
						check.push((cx, cy+1));
					}
				}	
			}
			
			regions.push(contains);
		}
		
		regions.retain(|coords| {
			let mut retain = true;
			
			for (x, y) in coords {
				if *x == 0 || *x == self.width as usize -1 || *y == 0 || *y == self.height as usize -1 {
					retain = false;
					break;
				}
			}
			
			if !retain {
				for (x, y) in coords {
					self.data[*x][*y] = 0.0;
				}
			}
			
			retain
		});
		
		let mut remove_regions = Vec::new();
		
		for (r, coords) in regions.iter().enumerate() {
			let (tx, ty) = coords.first().cloned().unwrap();
			let mut found = 0;
			let mut direction = 0;
			let mut sx = tx;
			let mut sy = ty;
			
			'dir_loop: while direction < 4 {
				if direction == 0 {
					if sx + 1 >= self.width as usize {
						direction += 1;
						sx = tx;
						sy = ty;
						continue;
					}
					
					sx += 1;
				} else if direction == 1 {
					if sx == 0 {
						direction += 1;
						sx = tx;
						sy = ty;
						continue;
					}
					
					sx -= 1;
				} else if direction == 2 {
					if sy + 1 >= self.height as usize {
						direction += 1;
						sx = tx;
						sy = ty;
						continue;
					}
					
					sy += 1;
				} else if direction == 3 {
					if sy == 0 {
						direction += 1;
						sx = tx;
						sy = ty;
						continue;
					}
					
					sy -= 1;
				}
				
				for (i, coords) in regions.iter().enumerate() {
					if coords.contains(&(sx, sy)) {
						if i != r {
							found += 1;
							direction += 1;
							sx = tx;
							sy = ty;
							continue;
						}
					}
				}
			}
			
			if found == 4 {
				remove_regions.push(r);
			}
		}
		
		for i in remove_regions.into_iter().rev() {
			for (x, y) in regions.swap_remove(i) {
				self.data[x][y] = 0.0;
			}
		}
	}
	
	pub fn draw_outline(&mut self) -> Result<(), BstTextError> {
		let glyph_raw = self.glyph_raw.clone();
		
		for geometry in &glyph_raw.geometry {
			self.draw_geometry(geometry)?;
		}
		
		Ok(())
	}
	
	pub fn draw_geometry(&mut self, geo: &BstGlyphGeo) -> Result<(), BstTextError> {
		match geo {
			&BstGlyphGeo::Line(ref points) => self.draw_line(&points[0], &points[1]),
			&BstGlyphGeo::Curve(ref points) => self.draw_curve(&points[0], &points[1], &points[2])
		}
	}
	
	pub fn draw_line(
		&mut self,
		point_a: &BstGlyphPoint,
		point_b: &BstGlyphPoint
	) -> Result<(), BstTextError> {
		self.lines.push((point_a.clone(), point_b.clone()));
		Ok(())
	}
	
	pub fn draw_curve(
		&mut self,
		point_a: &BstGlyphPoint,
		point_b: &BstGlyphPoint,
		point_c: &BstGlyphPoint
	) -> Result<(), BstTextError> {
		let steps = 3_usize;
		let mut last = point_a.clone();
		
		for s in 0..=steps {
			let t = s as f32 / steps as f32;
			let next = point_a.lerp(t, point_b).lerp(t, &point_b.lerp(t, point_c));
			self.draw_line(&last, &next);
			last = next;
		}
		
		Ok(())
	}
}
