use vulkano::image::traits::ImageViewAccess;
use std::sync::Barrier;
use vulkano::image::ImageLayout;
use vulkano::image::sys::UnsafeImageView;
use vulkano::image::Dimensions;
use vulkano::image::traits::ImageAccess;
use std::sync::Arc;

/// An abstraction on ImageViewAccess to provide a lease like function. This
/// simply wraps ImageViewAccess and provides a barrier that will be used when the
/// wrapper drops. The provided barrier should have wait() called before dropping.
pub struct TmpImageViewAccess {
	inner: Arc<ImageViewAccess + Send + Sync>,
	barrier: Arc<Barrier>,
}

impl TmpImageViewAccess {
	pub fn new(from: Arc<ImageViewAccess + Send + Sync>) -> (Self, Arc<Barrier>) {
		let barrier = Arc::new(Barrier::new(2));
		
		(TmpImageViewAccess {
			inner: from,
			barrier: barrier.clone(),
		}, barrier)
	}
}

impl Drop for TmpImageViewAccess {
	fn drop(&mut self) {
		self.barrier.wait();
	}
}

unsafe impl ImageViewAccess for TmpImageViewAccess {
	fn parent(&self) -> &ImageAccess {
		self.inner.parent()
	}
	
	fn dimensions(&self) -> Dimensions {
		self.inner.dimensions()
	}
	
	fn inner(&self) -> &UnsafeImageView {
		self.inner.inner()
	}
	
	fn descriptor_set_storage_image_layout(&self) -> ImageLayout {
		self.inner.descriptor_set_storage_image_layout()
	}
	
	fn descriptor_set_combined_image_sampler_layout(&self) -> ImageLayout {
		self.inner.descriptor_set_combined_image_sampler_layout()
	}
	
	fn descriptor_set_sampled_image_layout(&self) -> ImageLayout {
		self.inner.descriptor_set_sampled_image_layout()
	}
	
	fn descriptor_set_input_attachment_layout(&self) -> ImageLayout {
		self.inner.descriptor_set_input_attachment_layout()
	}
	
	fn identity_swizzle(&self) -> bool {
		self.inner.identity_swizzle()
	}
}

