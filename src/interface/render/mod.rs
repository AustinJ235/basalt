pub(crate) mod composer;
mod final_fs;
mod layer_desc_pool;
mod layer_fs;
mod layer_vs;
mod pipeline;
mod square_vs;

use self::composer::{Composer, ComposerEv, ComposerView};
use self::pipeline::{ItfPipeline, ItfPipelineInit};
use crate::atlas::Atlas;
use crate::image_view::BstImageView;
use crate::vulkano::image::ImageAccess;
use crate::{BstMSAALevel, BstOptions};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};
use vulkano::device::{Device, Queue};
use vulkano::format::{Format, Format as VkFormat};
use vulkano::image::view::ImageView;
use vulkano::image::{ImageDimensions, SwapchainImage};

pub(super) struct ItfRenderer {
	composer: Arc<Composer>,
	msaa: BstMSAALevel,
	target_info: ItfDrawTargetInfo,
	composer_view: Option<Arc<ComposerView>>,
	pipeline: ItfPipeline,
	conservative_draw: bool,
}

pub(super) struct ItfRendererInit {
	pub options: BstOptions,
	pub device: Arc<Device>,
	pub transfer_queue: Arc<Queue>,
	pub itf_format: VkFormat,
	pub atlas: Arc<Atlas>,
	pub composer: Arc<Composer>,
}

#[derive(Clone, PartialEq, Eq)]
enum ItfDrawTargetInfo {
	None,
	Image {
		extent: [u32; 2],
		msaa: BstMSAALevel,
	},
	Swapchain {
		extent: [u32; 2],
		image_count: usize,
		hash: u64,
		msaa: BstMSAALevel,
	},
	SwapchainWithSource {
		extent: [u32; 2],
		image_count: usize,
		hash: u64,
		msaa: BstMSAALevel,
	},
}

impl ItfDrawTargetInfo {
	#[inline]
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
			Self::SwapchainWithSource {
				extent,
				..
			} => *extent,
		}
	}

	#[inline]
	fn num_images(&self) -> usize {
		match self {
			Self::None => unreachable!(),
			Self::Image {
				..
			} => 1,
			Self::Swapchain {
				image_count,
				..
			} => *image_count,
			Self::SwapchainWithSource {
				image_count,
				..
			} => *image_count,
		}
	}

	#[inline]
	fn msaa(&self) -> BstMSAALevel {
		match self {
			Self::None => unreachable!(),
			Self::Image {
				msaa,
				..
			} => *msaa,
			Self::Swapchain {
				msaa,
				..
			} => *msaa,
			Self::SwapchainWithSource {
				msaa,
				..
			} => *msaa,
		}
	}
}

pub enum ItfDrawTarget<S: Send + Sync + std::fmt::Debug + 'static> {
	Image {
		extent: [u32; 2],
	},
	Swapchain {
		images: Vec<Arc<ImageView<SwapchainImage<S>>>>,
		image_num: usize,
	},
	SwapchainWithSource {
		source: Arc<BstImageView>,
		images: Vec<Arc<ImageView<SwapchainImage<S>>>>,
		image_num: usize,
	},
}

impl<S: Send + Sync + std::fmt::Debug> ItfDrawTarget<S> {
	#[inline]
	fn image_num(&self) -> usize {
		match self {
			Self::Image {
				..
			} => 0,
			Self::Swapchain {
				image_num,
				..
			} => *image_num,
			Self::SwapchainWithSource {
				image_num,
				..
			} => *image_num,
		}
	}

	#[inline]
	fn format(&self, default: VkFormat) -> Format {
		match self {
			Self::Image {
				..
			} => default,
			Self::Swapchain {
				images,
				..
			} => images[0].image().format(),
			Self::SwapchainWithSource {
				images,
				..
			} => images[0].image().format(),
		}
	}

	#[inline]
	fn is_swapchain(&self) -> bool {
		match self {
			Self::Image {
				..
			} => false,
			Self::Swapchain {
				..
			} => true,
			Self::SwapchainWithSource {
				..
			} => true,
		}
	}

	#[inline]
	fn swapchain_image(&self, i: usize) -> Arc<ImageView<SwapchainImage<S>>> {
		match self {
			Self::Image {
				..
			} => unreachable!(),
			Self::Swapchain {
				images,
				..
			} => images[i].clone(),
			Self::SwapchainWithSource {
				images,
				..
			} => images[i].clone(),
		}
	}

	#[inline]
	fn source_image(&self) -> Option<Arc<BstImageView>> {
		match self {
			Self::Image {
				..
			} => None,
			Self::Swapchain {
				..
			} => None,
			Self::SwapchainWithSource {
				source,
				..
			} => Some(source.clone()),
		}
	}
}

impl ItfRenderer {
	pub fn new(init: ItfRendererInit) -> Self {
		let ItfRendererInit {
			options,
			device,
			transfer_queue,
			itf_format,
			atlas,
			composer,
		} = init;

		Self {
			composer,
			msaa: options.msaa,
			conservative_draw: options.conservative_draw && options.app_loop,
			target_info: ItfDrawTargetInfo::None,
			composer_view: None,
			pipeline: ItfPipeline::new(ItfPipelineInit {
				options,
				device,
				transfer_queue,
				atlas,
				itf_format,
			}),
		}
	}

	pub fn msaa_mut_ref(&mut self) -> &mut BstMSAALevel {
		&mut self.msaa
	}

	pub fn draw<S: Send + Sync + std::fmt::Debug + 'static>(
		&mut self,
		cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
		target: ItfDrawTarget<S>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		let mut recreate_pipeline = false;

		let target_info = match &target {
			ItfDrawTarget::Image {
				extent,
				..
			} =>
				ItfDrawTargetInfo::Image {
					extent: *extent,
					msaa: self.msaa,
				},
			ItfDrawTarget::Swapchain {
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

				ItfDrawTargetInfo::Swapchain {
					extent,
					image_count: images.len(),
					hash: hasher.finish(),
					msaa: self.msaa,
				}
			},
			ItfDrawTarget::SwapchainWithSource {
				images,
				source,
				..
			} => {
				let mut hasher = DefaultHasher::new();

				for img in images.iter() {
					img.image().hash(&mut hasher);
				}

				(source as &(dyn ImageAccess)).hash(&mut hasher);

				let extent = match images[0].image().dimensions() {
					ImageDimensions::Dim2d {
						width,
						height,
						..
					} => [width, height],
					_ => unreachable!(),
				};

				ItfDrawTargetInfo::SwapchainWithSource {
					extent,
					image_count: images.len(),
					hash: hasher.finish(),
					msaa: self.msaa,
				}
			},
		};

		if target_info != self.target_info {
			if self.target_info != ItfDrawTargetInfo::None
				&& target_info.extent() != self.target_info.extent()
			{
				self.composer.send_event(ComposerEv::Extent(target_info.extent()));
			}

			self.target_info = target_info;
			recreate_pipeline = true;
		}

		let wait_for = if self.conservative_draw {
			Some(Duration::from_secs(1))
		} else {
			None
		};

		self.composer_view =
			Some(self.composer.update_view(self.composer_view.take(), wait_for));

		self.pipeline.draw(
			recreate_pipeline,
			self.composer_view.as_ref().unwrap(),
			target,
			&self.target_info,
			cmd,
		)
	}
}
