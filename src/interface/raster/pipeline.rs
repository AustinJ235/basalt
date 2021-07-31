use crate::image_view::BstImageView;
use crate::interface::raster::composer::ComposerView;
use crate::interface::raster::{BstRasterTarget, BstRasterTargetInfo};
use crate::Basalt;
use std::sync::Arc;
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};

pub(super) struct BstRasterPipeline {
	bst: Arc<Basalt>,
	context: Option<Context>,
}

struct Context {}

impl BstRasterPipeline {
	pub fn new(bst: Arc<Basalt>) -> Self {
		Self {
			bst,
			context: None,
		}
	}

	pub fn recreate<S: Send + Sync + 'static>(
		&mut self,
		target: BstRasterTarget<'_, S>,
		target_info: &BstRasterTargetInfo,
	) {
		unimplemented!()
	}

	pub fn draw(
		&self,
		view: &ComposerView,
		image_num: usize,
		cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		unimplemented!()
	}
}
