use std::sync::atomic::{self, AtomicBool, AtomicUsize};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use vulkano::device::{Device, DeviceOwned};
use vulkano::format::{Format, FormatFeatures};
use vulkano::image::view::{ImageView, ImageViewCreationError, ImageViewType};
use vulkano::image::{
    AttachmentImage, ImageAccess, ImageDescriptorLayouts, ImageDimensions, ImageInner, ImageLayout,
    ImageSubresourceRange, ImageUsage, ImageViewAbstract, ImmutableImage, StorageImage,
};
use vulkano::sampler::ycbcr::SamplerYcbcrConversion;
use vulkano::sampler::ComponentMapping;
use vulkano::VulkanObject;

#[derive(Debug)]
enum ImageVarient {
    Storage(Arc<StorageImage>),
    Immutable(Arc<ImmutableImage>),
    Attachment(Arc<AttachmentImage>),
}

enum ViewVarient {
    Parent(ParentView),
    Child(ChildView),
}

struct ParentView {
    view: Arc<ImageView<ImageVarient>>,
    children: Mutex<Vec<Weak<BstImageView>>>,
    children_alive: AtomicUsize,
    drop_fn: Mutex<Option<Box<dyn FnMut() + Send + 'static>>>,
}

struct ChildView {
    parent: Arc<BstImageView>,
    stale: AtomicBool,
}

pub struct BstImageView {
    view: ViewVarient,
}

impl BstImageView {
    /// Create a `BstImageView` from a vulkano `StorageImage`.
    pub fn from_storage(image: Arc<StorageImage>) -> Result<Arc<Self>, ImageViewCreationError> {
        Ok(Arc::new(BstImageView {
            view: ViewVarient::Parent(ParentView {
                view: ImageView::new_default(Arc::new(ImageVarient::Storage(image)))?,
                children: Mutex::new(Vec::new()),
                children_alive: AtomicUsize::new(0),
                drop_fn: Mutex::new(None),
            }),
        }))
    }

    /// Create a `BstImageView` from a vulkano `ImmutableImage`.
    pub fn from_immutable(image: Arc<ImmutableImage>) -> Result<Arc<Self>, ImageViewCreationError> {
        Ok(Arc::new(BstImageView {
            view: ViewVarient::Parent(ParentView {
                view: ImageView::new_default(Arc::new(ImageVarient::Immutable(image)))?,
                children: Mutex::new(Vec::new()),
                children_alive: AtomicUsize::new(0),
                drop_fn: Mutex::new(None),
            }),
        }))
    }

    /// Create a `BstImageView` from a vulkano `AttachmentImage`.
    pub fn from_attachment(
        image: Arc<AttachmentImage>,
    ) -> Result<Arc<Self>, ImageViewCreationError> {
        Ok(Arc::new(BstImageView {
            view: ViewVarient::Parent(ParentView {
                view: ImageView::new_default(Arc::new(ImageVarient::Attachment(image)))?,
                children: Mutex::new(Vec::new()),
                children_alive: AtomicUsize::new(0),
                drop_fn: Mutex::new(None),
            }),
        }))
    }

    /// Set function to be called with all temporary views are dropped.
    ///
    /// Overwrites existing drop function if it was previously set.
    ///
    /// # Panics
    ///
    /// Panics if this method is called on a temporary view.
    pub fn set_drop_fn<F: FnMut() + Send + 'static>(&self, drop_fn_op: Option<F>) {
        match &self.view {
            ViewVarient::Parent(ParentView {
                drop_fn, ..
            }) => {
                *drop_fn.lock() = match drop_fn_op {
                    Some(func) => Some(Box::new(func)),
                    None => None,
                };
            },
            _ => panic!("Attempted to set drop function on a temporary view."),
        }
    }

    fn parent(self: &Arc<Self>) -> Arc<Self> {
        match &self.view {
            ViewVarient::Parent(_) => self.clone(),
            ViewVarient::Child(ChildView {
                parent, ..
            }) => parent.parent(),
        }
    }

    /// Create a clone of this view that is intented to be temporary.
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
        matches!(&self.view, ViewVarient::Child(_))
    }

    /// Amount of temporary views this image has.
    pub fn temporary_views(self: &Arc<Self>) -> usize {
        match &self.parent().view {
            ViewVarient::Parent(ParentView {
                children_alive, ..
            }) => children_alive.load(atomic::Ordering::SeqCst),
            _ => panic!("temporary_views() called on a temporary image."),
        }
    }

    /// Marks temporary views stale.
    ///
    /// # Notes
    /// - This is a NO-OP on a temporary view.
    pub fn mark_stale(&self) {
        if let ViewVarient::Parent(ParentView {
            children, ..
        }) = &self.view
        {
            let mut children = children.lock();

            children.retain(|c| {
                match c.upgrade() {
                    Some(c) => {
                        match &c.view {
                            ViewVarient::Child(ChildView {
                                stale, ..
                            }) => {
                                stale.store(true, atomic::Ordering::SeqCst);
                                true
                            },
                            _ => unreachable!(),
                        }
                    },
                    None => false,
                }
            });
        }
    }

    /// Check if this view is stale. Always returns false for Parent views.
    pub fn is_stale(&self) -> bool {
        match &self.view {
            ViewVarient::Parent(_) => false,
            ViewVarient::Child(ChildView {
                stale, ..
            }) => stale.load(atomic::Ordering::SeqCst),
        }
    }

    #[inline]
    fn image_view(&self) -> Arc<ImageView<ImageVarient>> {
        match &self.view {
            ViewVarient::Parent(ParentView {
                view, ..
            }) => view.clone(),
            ViewVarient::Child(ChildView {
                parent, ..
            }) => parent.image_view(),
        }
    }

    #[inline]
    fn image_view_ref(&self) -> &Arc<ImageView<ImageVarient>> {
        match &self.view {
            ViewVarient::Parent(ParentView {
                ref view, ..
            }) => view,
            ViewVarient::Child(ChildView {
                ref parent, ..
            }) => parent.image_view_ref(),
        }
    }

    /// Fetch the dimensions of this image.
    #[inline]
    pub fn dimensions(&self) -> ImageDimensions {
        self.image_view().image().dimensions()
    }
}

impl Drop for BstImageView {
    fn drop(&mut self) {
        match &self.view {
            ViewVarient::Parent(_) => (),
            ViewVarient::Child(ChildView {
                parent, ..
            }) => {
                match &parent.view {
                    ViewVarient::Parent(ParentView {
                        children,
                        children_alive,
                        drop_fn,
                        ..
                    }) => {
                        if children_alive.fetch_sub(1, atomic::Ordering::SeqCst) == 1 {
                            {
                                // Scope to drop lock before calling drop function
                                let mut children_gu = children.lock();
                                children_gu.retain(|child_wk| child_wk.strong_count() > 0);
                                assert!(children_gu.is_empty());
                            }

                            if let Some(drop_fn) = &mut *drop_fn.lock() {
                                drop_fn();
                            }
                        }
                    },
                    _ => unreachable!(),
                }
            },
        }
    }
}

unsafe impl ImageAccess for ImageVarient {
    #[inline]
    fn inner(&self) -> ImageInner<'_> {
        match self {
            Self::Storage(i) => i.inner(),
            Self::Immutable(i) => i.inner(),
            Self::Attachment(i) => i.inner(),
        }
    }

    #[inline]
    fn initial_layout_requirement(&self) -> ImageLayout {
        match self {
            Self::Storage(i) => i.initial_layout_requirement(),
            Self::Immutable(i) => i.initial_layout_requirement(),
            Self::Attachment(i) => i.initial_layout_requirement(),
        }
    }

    #[inline]
    fn final_layout_requirement(&self) -> ImageLayout {
        match self {
            Self::Storage(i) => i.final_layout_requirement(),
            Self::Immutable(i) => i.final_layout_requirement(),
            Self::Attachment(i) => i.final_layout_requirement(),
        }
    }

    #[inline]
    fn descriptor_layouts(&self) -> Option<ImageDescriptorLayouts> {
        match self {
            Self::Storage(i) => i.descriptor_layouts(),
            Self::Immutable(i) => i.descriptor_layouts(),
            Self::Attachment(i) => i.descriptor_layouts(),
        }
    }

    #[inline]
    unsafe fn layout_initialized(&self) {
        match self {
            Self::Storage(i) => i.layout_initialized(),
            Self::Immutable(i) => i.layout_initialized(),
            Self::Attachment(i) => i.layout_initialized(),
        }
    }

    #[inline]
    fn is_layout_initialized(&self) -> bool {
        match self {
            Self::Storage(i) => i.is_layout_initialized(),
            Self::Immutable(i) => i.is_layout_initialized(),
            Self::Attachment(i) => i.is_layout_initialized(),
        }
    }
}

unsafe impl DeviceOwned for ImageVarient {
    fn device(&self) -> &Arc<Device> {
        match self {
            Self::Storage(i) => i.device(),
            Self::Immutable(i) => i.device(),
            Self::Attachment(i) => i.device(),
        }
    }
}

unsafe impl ImageViewAbstract for BstImageView {
    #[inline]
    fn image(&self) -> Arc<dyn ImageAccess> {
        self.image_view_ref().image().clone() as Arc<dyn ImageAccess>
    }

    #[inline]
    fn component_mapping(&self) -> ComponentMapping {
        self.image_view_ref().component_mapping()
    }

    #[inline]
    fn filter_cubic(&self) -> bool {
        self.image_view_ref().filter_cubic()
    }

    #[inline]
    fn filter_cubic_minmax(&self) -> bool {
        self.image_view_ref().filter_cubic_minmax()
    }

    #[inline]
    fn format(&self) -> Option<Format> {
        self.image_view_ref().format()
    }

    #[inline]
    fn format_features(&self) -> &FormatFeatures {
        self.image_view_ref().format_features()
    }

    #[inline]
    fn sampler_ycbcr_conversion(&self) -> Option<&Arc<SamplerYcbcrConversion>> {
        self.image_view_ref().sampler_ycbcr_conversion()
    }

    #[inline]
    fn subresource_range(&self) -> &ImageSubresourceRange {
        self.image_view_ref().subresource_range()
    }

    #[inline]
    fn usage(&self) -> &ImageUsage {
        self.image_view_ref().usage()
    }

    #[inline]
    fn view_type(&self) -> ImageViewType {
        self.image_view_ref().view_type()
    }
}

unsafe impl VulkanObject for BstImageView {
    type Object = ash::vk::ImageView;

    #[inline]
    fn internal_object(&self) -> ash::vk::ImageView {
        self.image_view_ref().internal_object()
    }
}

unsafe impl DeviceOwned for BstImageView {
    fn device(&self) -> &Arc<Device> {
        self.image_view_ref().device()
    }
}

impl PartialEq for BstImageView {
    fn eq(&self, other: &Self) -> bool {
        self.image_view_ref() == other.image_view_ref()
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
    unsafe fn layout_initialized(&self) {
        self.image_view_ref().image().layout_initialized()
    }

    #[inline]
    fn is_layout_initialized(&self) -> bool {
        self.image_view_ref().image().is_layout_initialized()
    }
}
