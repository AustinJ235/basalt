pub mod basic;
pub mod multi_basic;

use std::sync::Arc;
use vulkano::device::{self,Device};
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::image::traits::ImageViewAccess;
use vulkano::sampler::Sampler;
use shaders::vs;

impl_vertex!(Vert, position, normal, color, tex_info, ty);
pub(crate) struct Vert {
	pub position: (f32, f32, f32),
	pub normal: (f32, f32, f32),
	pub color: (f32, f32, f32, f32),
	pub tex_info: (f32, f32, f32, f32),
	pub ty: i32
}

pub(crate) trait Buffer {
	fn draw(&self, device: Arc<Device>, queue: Arc<device::Queue>) ->
		Option<(
			Arc<ImmutableBuffer<vs::ty::Other>>,
			Vec<(
				Arc<ImageViewAccess + Send + Sync>,
				Arc<Sampler>,
				Arc<ImmutableBuffer<[Vert]>>
			)>, Vec<(
				Arc<ImageViewAccess + Send + Sync>,
				Arc<Sampler>,
				Arc<ImmutableBuffer<[Vert]>>
			)>,
		)>;
	fn triangles(&self) -> usize;
}
