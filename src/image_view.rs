use parking_lot::Mutex;
use std::ops::Range;
use std::sync::atomic::{self, AtomicBool, AtomicUsize};
use std::sync::{Arc, Weak};
use vulkano::format::Format;
use vulkano::image::immutable::SubImage;
use vulkano::image::view::{
	ComponentMapping, ImageView, ImageViewCreationError, ImageViewType, UnsafeImageView,
};
use vulkano::image::{
	AttachmentImage, ImageAccess, ImageDescriptorLayouts, ImageDimensions, ImageInner,
	ImageLayout, ImageViewAbstract, ImmutableImage, StorageImage,
};
use vulkano::sampler::Sampler;
use vulkano::sync::AccessError;

enum ImageVarient {
	Storage(Arc<StorageImage>),
	Immutable(Arc<ImmutableImage>),
	Sub(Arc<SubImage>),
	Attachment(Arc<AttachmentImage>),
}

unsafe impl ImageAccess for ImageVarient {
	#[inline]
	fn inner(&self) -> ImageInner<'_> {
		match self {
			Self::Storage(i) => i.inner(),
			Self::Immutable(i) => i.inner(),
			Self::Sub(i) => i.inner(),
			Self::Attachment(i) => i.inner(),
		}
	}

	#[inline]
	fn initial_layout_requirement(&self) -> ImageLayout {
		match self {
			Self::Storage(i) => i.initial_layout_requirement(),
			Self::Immutable(i) => i.initial_layout_requirement(),
			Self::Sub(i) => i.initial_layout_requirement(),
			Self::Attachment(i) => i.initial_layout_requirement(),
		}
	}

	#[inline]
	fn final_layout_requirement(&self) -> ImageLayout {
		match self {
			Self::Storage(i) => i.final_layout_requirement(),
			Self::Immutable(i) => i.final_layout_requirement(),
			Self::Sub(i) => i.final_layout_requirement(),
			Self::Attachment(i) => i.final_layout_requirement(),
		}
	}

	#[inline]
	fn descriptor_layouts(&self) -> Option<ImageDescriptorLayouts> {
		match self {
			Self::Storage(i) => i.descriptor_layouts(),
			Self::Immutable(i) => i.descriptor_layouts(),
			Self::Sub(i) => i.descriptor_layouts(),
			Self::Attachment(i) => i.descriptor_layouts(),
		}
	}

	#[inline]
	fn conflict_key(&self) -> u64 {
		match self {
			Self::Storage(i) => i.conflict_key(),
			Self::Immutable(i) => i.conflict_key(),
			Self::Sub(i) => i.conflict_key(),
			Self::Attachment(i) => i.conflict_key(),
		}
	}

	#[inline]
	fn current_miplevels_access(&self) -> Range<u32> {
		match self {
			Self::Storage(i) => i.current_miplevels_access(),
			Self::Immutable(i) => i.current_miplevels_access(),
			Self::Sub(i) => i.current_miplevels_access(),
			Self::Attachment(i) => i.current_miplevels_access(),
		}
	}

	#[inline]
	fn current_layer_levels_access(&self) -> Range<u32> {
		match self {
			Self::Storage(i) => i.current_layer_levels_access(),
			Self::Immutable(i) => i.current_layer_levels_access(),
			Self::Sub(i) => i.current_layer_levels_access(),
			Self::Attachment(i) => i.current_layer_levels_access(),
		}
	}

	#[inline]
	fn try_gpu_lock(
		&self,
		exclusive_access: bool,
		uninitialized_safe: bool,
		expected_layout: ImageLayout,
	) -> Result<(), AccessError> {
		match self {
			Self::Storage(i) =>
				i.try_gpu_lock(exclusive_access, uninitialized_safe, expected_layout),
			Self::Immutable(i) =>
				i.try_gpu_lock(exclusive_access, uninitialized_safe, expected_layout),
			Self::Sub(i) =>
				i.try_gpu_lock(exclusive_access, uninitialized_safe, expected_layout),
			Self::Attachment(i) =>
				i.try_gpu_lock(exclusive_access, uninitialized_safe, expected_layout),
		}
	}

	#[inline]
	unsafe fn increase_gpu_lock(&self) {
		match self {
			Self::Storage(i) => i.increase_gpu_lock(),
			Self::Immutable(i) => i.increase_gpu_lock(),
			Self::Sub(i) => i.increase_gpu_lock(),
			Self::Attachment(i) => i.increase_gpu_lock(),
		}
	}

	#[inline]
	unsafe fn unlock(&self, transitioned_layout: Option<ImageLayout>) {
		match self {
			Self::Storage(i) => i.unlock(transitioned_layout),
			Self::Immutable(i) => i.unlock(transitioned_layout),
			Self::Sub(i) => i.unlock(transitioned_layout),
			Self::Attachment(i) => i.unlock(transitioned_layout),
		}
	}
}

enum ViewVarient {
	Parent(ParentView),
	Child(ChildView),
}

struct ParentView {
	view: Arc<ImageView<ImageVarient>>,
	children: Mutex<Vec<Weak<BstImageView>>>,
	children_alive: AtomicUsize,
}

struct ChildView {
	parent: Arc<BstImageView>,
	stale: AtomicBool,
}

pub struct BstImageView {
	view: ViewVarient,
}

impl BstImageView {
	pub fn from_storage(image: Arc<StorageImage>) -> Result<Arc<Self>, ImageViewCreationError> {
		Ok(Arc::new(BstImageView {
			view: ViewVarient::Parent(ParentView {
				view: ImageView::new(Arc::new(ImageVarient::Storage(image)))?,
				children: Mutex::new(Vec::new()),
				children_alive: AtomicUsize::new(0),
			}),
		}))
	}

	pub fn from_immutable(
		image: Arc<ImmutableImage>,
	) -> Result<Arc<Self>, ImageViewCreationError> {
		Ok(Arc::new(BstImageView {
			view: ViewVarient::Parent(ParentView {
				view: ImageView::new(Arc::new(ImageVarient::Immutable(image)))?,
				children: Mutex::new(Vec::new()),
				children_alive: AtomicUsize::new(0),
			}),
		}))
	}

	pub fn from_sub(image: Arc<SubImage>) -> Result<Arc<Self>, ImageViewCreationError> {
		Ok(Arc::new(BstImageView {
			view: ViewVarient::Parent(ParentView {
				view: ImageView::new(Arc::new(ImageVarient::Sub(image)))?,
				children: Mutex::new(Vec::new()),
				children_alive: AtomicUsize::new(0),
			}),
		}))
	}

	pub fn from_attachment(
		image: Arc<AttachmentImage>,
	) -> Result<Arc<Self>, ImageViewCreationError> {
		Ok(Arc::new(BstImageView {
			view: ViewVarient::Parent(ParentView {
				view: ImageView::new(Arc::new(ImageVarient::Attachment(image)))?,
				children: Mutex::new(Vec::new()),
				children_alive: AtomicUsize::new(0),
			}),
		}))
	}

	fn parent(self: &Arc<Self>) -> Arc<Self> {
		match &self.view {
			ViewVarient::Parent(_) => self.clone(),
			ViewVarient::Child(ChildView {
				parent,
				..
			}) => parent.parent(),
		}
	}

	/// Create a clone of this view that is intented to be temporary. This method will return
	/// a clone of this view along with an `AtomicBool`. The `AtomicBool` will be set to false
	/// when the cloned copy is dropped.
	pub fn create_tmp(self: &Arc<Self>) -> Arc<Self> {
		let parent = self.parent();
		let child = Arc::new(Self {
			view: ViewVarient::Child(ChildView {
				parent: parent.clone(),
				stale: AtomicBool::new(false),
			}),
		});

		match &parent.view {
			ViewVarient::Parent(ParentView {
				children,
				children_alive,
				..
			}) => {
				children_alive.fetch_add(1, atomic::Ordering::SeqCst);
				children.lock().push(Arc::downgrade(&child));
			},
			_ => unreachable!(),
		}

		child
	}

	/// Check whether this view is temporary. In the case it is the method that provided this
	/// view intended for it be dropped after use.
	pub fn is_temporary(&self) -> bool {
		match &self.view {
			ViewVarient::Child(_) => true,
			_ => false,
		}
	}

	pub fn temporary_views(self: &Arc<Self>) -> usize {
		match &self.parent().view {
			ViewVarient::Parent(ParentView {
				children_alive,
				..
			}) => children_alive.load(atomic::Ordering::SeqCst),
			_ => unreachable!(),
		}
	}

	/// Marks temporary views stale.
	pub fn mark_stale(&self) {
		match &self.view {
			ViewVarient::Parent(ParentView {
				children,
				..
			}) => {
				let mut children = children.lock();

				children.retain(|c| {
					match c.upgrade() {
						Some(c) =>
							match &c.view {
								ViewVarient::Child(ChildView {
									stale,
									..
								}) => {
									stale.store(true, atomic::Ordering::SeqCst);
									true
								},
								_ => unreachable!(),
							},
						None => false,
					}
				});
			},
			_ => (), // NO-OP
		}
	}

	/// Check if this view is stale. Always returns false for Parent views.
	pub fn is_stale(&self) -> bool {
		match &self.view {
			ViewVarient::Parent(_) => false,
			ViewVarient::Child(ChildView {
				stale,
				..
			}) => stale.load(atomic::Ordering::SeqCst),
		}
	}

	#[inline]
	fn image_view(&self) -> Arc<ImageView<ImageVarient>> {
		match &self.view {
			ViewVarient::Parent(ParentView {
				view,
				..
			}) => view.clone(),
			ViewVarient::Child(ChildView {
				parent,
				..
			}) => parent.image_view(),
		}
	}

	#[inline]
	fn image_view_ref(&self) -> &Arc<ImageView<ImageVarient>> {
		match &self.view {
			ViewVarient::Parent(ParentView {
				ref view,
				..
			}) => view,
			ViewVarient::Child(ChildView {
				ref parent,
				..
			}) => parent.image_view_ref(),
		}
	}

	#[inline]
	pub fn dimensions(&self) -> ImageDimensions {
		self.image_view().image().dimensions()
	}
}

unsafe impl ImageViewAbstract for BstImageView {
	#[inline]
	fn image(&self) -> Arc<dyn ImageAccess> {
		self.image_view_ref().image().clone() as Arc<dyn ImageAccess>
	}

	#[inline]
	fn inner(&self) -> &UnsafeImageView {
		self.image_view_ref().inner()
	}

	#[inline]
	fn array_layers(&self) -> Range<u32> {
		self.image_view_ref().array_layers()
	}

	#[inline]
	fn format(&self) -> Format {
		self.image_view_ref().format()
	}

	#[inline]
	fn component_mapping(&self) -> ComponentMapping {
		self.image_view_ref().component_mapping()
	}

	#[inline]
	fn ty(&self) -> ImageViewType {
		self.image_view_ref().ty()
	}

	#[inline]
	fn can_be_sampled(&self, _sampler: &Sampler) -> bool {
		self.image_view_ref().can_be_sampled(_sampler)
	}
}

impl PartialEq for BstImageView {
	fn eq(&self, other: &Self) -> bool {
		(self as &dyn ImageViewAbstract).inner() == (other as &dyn ImageViewAbstract).inner()
	}
}

impl Eq for BstImageView {}

impl std::fmt::Debug for BstImageView {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match &self.view {
			ViewVarient::Parent(_) => write!(f, "BstImageView(Owned)"),
			ViewVarient::Child(..) => write!(f, "BstImageView(Temporary)"),
		}
	}
}

unsafe impl ImageAccess for BstImageView {
	#[inline]
	fn inner(&self) -> ImageInner<'_> {
		self.image_view_ref().image().inner()
	}

	#[inline]
	fn initial_layout_requirement(&self) -> ImageLayout {
		self.image_view_ref().image().initial_layout_requirement()
	}

	#[inline]
	fn final_layout_requirement(&self) -> ImageLayout {
		self.image_view_ref().image().final_layout_requirement()
	}

	#[inline]
	fn descriptor_layouts(&self) -> Option<ImageDescriptorLayouts> {
		self.image_view_ref().image().descriptor_layouts()
	}

	#[inline]
	fn conflict_key(&self) -> u64 {
		self.image_view_ref().image().conflict_key()
	}

	#[inline]
	fn current_miplevels_access(&self) -> Range<u32> {
		self.image_view_ref().image().current_miplevels_access()
	}

	#[inline]
	fn current_layer_levels_access(&self) -> Range<u32> {
		self.image_view_ref().image().current_layer_levels_access()
	}

	#[inline]
	fn try_gpu_lock(
		&self,
		exclusive_access: bool,
		uninitialized_safe: bool,
		expected_layout: ImageLayout,
	) -> Result<(), AccessError> {
		self.image_view_ref().image().try_gpu_lock(
			exclusive_access,
			uninitialized_safe,
			expected_layout,
		)
	}

	#[inline]
	unsafe fn increase_gpu_lock(&self) {
		self.image_view_ref().image().increase_gpu_lock()
	}

	#[inline]
	unsafe fn unlock(&self, transitioned_layout: Option<ImageLayout>) {
		self.image_view_ref().image().unlock(transitioned_layout)
	}
}

impl Drop for BstImageView {
	fn drop(&mut self) {
		match &self.view {
			ViewVarient::Parent(_) => (),
			ViewVarient::Child(ChildView {
				parent,
				..
			}) =>
				match &parent.view {
					ViewVarient::Parent(ParentView {
						children_alive,
						..
					}) => {
						children_alive.fetch_sub(1, atomic::Ordering::SeqCst);
					},
					_ => unreachable!(),
				},
		}
	}
}
