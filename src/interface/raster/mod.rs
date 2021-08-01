mod blend_fs;
mod composer;
mod final_fs;
mod layer_fs;
mod layer_vs;
mod pipeline;
mod square_vs;

use self::composer::{Composer, ComposerView};
use self::pipeline::BstRasterPipeline;
use crate::image_view::BstImageView;
use crate::vulkano::image::ImageAccess;
use crate::{Basalt, BstEvent, BstItfEv, BstMSAALevel};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{ImageDimensions, SwapchainImage};

pub struct BstRaster {
	bst: Arc<Basalt>,
	scale: f32,
	msaa: BstMSAALevel,
	target_info: BstRasterTargetInfo,
	composer: Composer,
	composer_view: Option<ComposerView>,
	pipeline: BstRasterPipeline,
}

#[derive(Clone, PartialEq, Eq)]
enum BstRasterTargetInfo {
	None,
	Image {
		extent: [u32; 2],
		image_count: usize,
	},
	Swapchain {
		extent: [u32; 2],
		image_count: usize,
		hash: u64,
	},
}

impl BstRasterTargetInfo {
	fn extent(&self) -> [u32; 2] {
		match self {
			Self::None => unreachable!(),
			Self::Image {
				extent,
				..
			} => *extent,
			Self::Swapchain {
				extent,
				..
			} => *extent,
		}
	}

	fn num_images(&self) -> usize {
		match self {
			Self::None => unreachable!(),
			Self::Image {
				image_count,
				..
			} => *image_count,
			Self::Swapchain {
				image_count,
				..
			} => *image_count,
		}
	}
}

pub enum BstRasterTarget<S: Send + Sync + 'static> {
	Image {
		extent: [u32; 2],
		image_count: usize,
		image_num: usize,
	},
	Swapchain {
		images: Vec<Arc<ImageView<Arc<SwapchainImage<S>>>>>,
		image_num: usize,
	},
}

impl<S: Send + Sync> BstRasterTarget<S> {
	fn image_num(&self) -> usize {
		match self {
			Self::Image {
				image_num,
				..
			} => *image_num,
			Self::Swapchain {
				image_num,
				..
			} => *image_num,
		}
	}

	fn format(&self) -> Format {
		match self {
			Self::Image {
				..
			} => Format::R16G16B16A16Unorm,
			Self::Swapchain {
				images,
				..
			} => images[0].image().format(),
		}
	}

	fn is_swapchain(&self) -> bool {
		match self {
			Self::Image {
				..
			} => false,
			Self::Swapchain {
				..
			} => true,
		}
	}

	fn swapchain_image(&self, i: usize) -> Arc<ImageView<Arc<SwapchainImage<S>>>> {
		match self {
			Self::Image {
				..
			} => unreachable!(),
			Self::Swapchain {
				images,
				..
			} => images[i].clone(),
		}
	}
}

impl BstRaster {
	pub fn new(bst: Arc<Basalt>) -> Self {
		Self {
			scale: bst.options_ref().scale,
			msaa: bst.options_ref().msaa,
			target_info: BstRasterTargetInfo::None,
			composer: Composer::new(bst.clone()),
			composer_view: None,
			pipeline: BstRasterPipeline::new(bst.clone()),
			bst,
		}
	}

	pub fn draw<S: Send + Sync + 'static>(
		&mut self,
		cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
		target: BstRasterTarget<S>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		let bst = self.bst.clone();
		let mut recreate_pipeline = false;

		bst.poll_events_internal(|ev| {
			match ev {
				BstEvent::BstItfEv(itf_ev) =>
					match itf_ev {
						BstItfEv::ScaleChanged => {
							self.scale = bst.interface_ref().scale();
							false
						},
						BstItfEv::MSAAChanged => {
							self.msaa = bst.interface_ref().msaa();
							recreate_pipeline = true;
							false
						},
						_ => true,
					},
				_ => true,
			}
		});

		let target_info = match &target {
			BstRasterTarget::Image {
				image_count,
				extent,
				..
			} =>
				BstRasterTargetInfo::Image {
					extent: *extent,
					image_count: *image_count,
				},
			BstRasterTarget::Swapchain {
				images,
				..
			} => {
				let mut hasher = DefaultHasher::new();

				for img in images.iter() {
					img.image().hash(&mut hasher);
				}

				let extent = match images[0].image().dimensions() {
					ImageDimensions::Dim2d {
						width,
						height,
						..
					} => [width, height],
					_ => unreachable!(),
				};

				BstRasterTargetInfo::Swapchain {
					extent,
					image_count: images.len(),
					hash: hasher.finish(),
				}
			},
		};

		if target_info != self.target_info {
			self.target_info = target_info;
			recreate_pipeline = true;
		}

		self.composer.update_and_compose(self.scale, self.target_info.extent());
		self.composer_view = Some(self.composer.check_view(self.composer_view.take()));

		self.pipeline.draw(
			recreate_pipeline,
			&self.composer_view.as_ref().unwrap(),
			target,
			&self.target_info,
			cmd,
		)
	}
}
