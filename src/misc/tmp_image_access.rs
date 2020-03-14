use std::sync::{
	atomic::{self, AtomicBool},
	Arc, Barrier,
};
use vulkano::image::{
	sys::UnsafeImageView,
	traits::{ImageAccess, ImageViewAccess},
	Dimensions, ImageLayout,
};

/// An abstraction on ImageViewAccess to provide a lease like function. This
/// simply wraps ImageViewAccess and provides a barrier that will be used when the
/// wrapper drops. The provided barrier should have wait() called before dropping.
pub struct TmpImageViewAccess {
	inner: Arc<dyn ImageViewAccess + Send + Sync>,
	barrier: Option<Arc<Barrier>>,
	abool: Option<Arc<AtomicBool>>,
}

impl TmpImageViewAccess {
	pub fn new(from: Arc<dyn ImageViewAccess + Send + Sync>) -> (Self, Arc<Barrier>) {
		let barrier = Arc::new(Barrier::new(2));

		(
			TmpImageViewAccess {
				inner: from,
				barrier: Some(barrier.clone()),
				abool: None,
			},
			barrier,
		)
	}

	pub fn new_abool(from: Arc<dyn ImageViewAccess + Send + Sync>) -> (Self, Arc<AtomicBool>) {
		let abool = Arc::new(AtomicBool::new(true));

		(
			TmpImageViewAccess {
				inner: from,
				barrier: None,
				abool: Some(abool.clone()),
			},
			abool,
		)
	}
}

impl Drop for TmpImageViewAccess {
	fn drop(&mut self) {
		if let Some(abool) = self.abool.take() {
			abool.store(false, atomic::Ordering::Relaxed);
		}

		if let Some(barrier) = self.barrier.take() {
			barrier.wait();
		}
	}
}

unsafe impl ImageViewAccess for TmpImageViewAccess {
	fn parent(&self) -> &dyn ImageAccess {
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
