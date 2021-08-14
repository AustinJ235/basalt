pub(crate) mod composer;
mod final_fs;
mod layer_fs;
mod layer_vs;
mod pipeline;
mod square_vs;

use self::composer::{ComposerEv, ComposerView};
use self::pipeline::ItfPipeline;
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

pub(super) struct ItfRenderer {
	bst: Arc<Basalt>,
	msaa: BstMSAALevel,
	target_info: ItfDrawTargetInfo,
	composer_view: Option<Arc<ComposerView>>,
	pipeline: ItfPipeline,
}

#[derive(Clone, PartialEq, Eq)]
enum ItfDrawTargetInfo {
	None,
	Image {
		extent: [u32; 2],
	},
	Swapchain {
		extent: [u32; 2],
		image_count: usize,
		hash: u64,
	},
	SwapchainWithSource {
		extent: [u32; 2],
		image_count: usize,
		hash: u64,
	},
}

impl ItfDrawTargetInfo {
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
}

pub enum ItfDrawTarget<S: Send + Sync + 'static> {
	Image {
		extent: [u32; 2],
	},
	Swapchain {
		images: Vec<Arc<ImageView<Arc<SwapchainImage<S>>>>>,
		image_num: usize,
	},
	SwapchainWithSource {
		source: Arc<BstImageView>,
		images: Vec<Arc<ImageView<Arc<SwapchainImage<S>>>>>,
		image_num: usize,
	},
}

impl<S: Send + Sync> ItfDrawTarget<S> {
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

	fn format(&self, bst: &Arc<Basalt>) -> Format {
		match self {
			Self::Image {
				..
			} => bst.formats_in_use().interface,
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

	fn swapchain_image(&self, i: usize) -> Arc<ImageView<Arc<SwapchainImage<S>>>> {
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
	pub fn new(bst: Arc<Basalt>) -> Self {
		Self {
			msaa: bst.options_ref().msaa,
			target_info: ItfDrawTargetInfo::None,
			composer_view: None,
			pipeline: ItfPipeline::new(bst.clone()),
			bst,
		}
	}

	pub fn draw<S: Send + Sync + 'static>(
		&mut self,
		cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
		target: ItfDrawTarget<S>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		let mut recreate_pipeline = false;
		let bst = self.bst.clone(); // TODO: weird partial borrow things
		let composer = self.bst.interface_ref().composer_ref().clone();

		bst.poll_events_internal(|ev| {
			match ev {
				BstEvent::BstItfEv(itf_ev) =>
					match itf_ev {
						BstItfEv::ScaleChanged => {
							composer.send_event(ComposerEv::Scale(
								self.bst.interface_ref().scale(),
							));
							false
						},
						BstItfEv::MSAAChanged => {
							self.msaa = self.bst.interface_ref().msaa();
							recreate_pipeline = true;
							false
						},
					},
				_ => true,
			}
		});

		let target_info = match &target {
			ItfDrawTarget::Image {
				extent,
				..
			} =>
				ItfDrawTargetInfo::Image {
					extent: *extent,
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

				(source as &(dyn ImageAccess + Send + Sync)).hash(&mut hasher);

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
				}
			},
		};

		if target_info != self.target_info {
			if self.target_info != ItfDrawTargetInfo::None
				&& target_info.extent() != self.target_info.extent()
			{
				composer.send_event(ComposerEv::Extent(target_info.extent()));
			}

			self.target_info = target_info;
			recreate_pipeline = true;
		}

		self.composer_view = Some(composer.check_view(self.composer_view.take()));

		self.pipeline.draw(
			recreate_pipeline,
			&self.composer_view.as_ref().unwrap(),
			target,
			&self.target_info,
			cmd,
		)
	}
}