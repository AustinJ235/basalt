//! System for storing images used within the UI.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};

use cosmic_text::CacheKey as GlyphCacheKey;
use parking_lot::Mutex;
use url::Url;
use vulkano::format::Format as VkFormat;

/// `ImageCacheKey` is a value used to refrence an image within the cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageCacheKey {
    Url(Url),
    Path(PathBuf),
    Glyph(GlyphCacheKey),
    User(TypeId, u64),
}

impl ImageCacheKey {
    /// Creates an `ImageCacheKey` from the provided URL. This will not load the image.
    pub fn url<U: AsRef<str>>(url: U) -> Result<Self, ()> {
        todo!()
    }

    /// Create an `ImageCacheKey` from the provided path. This will not load the image.
    pub fn path<P: AsRef<str>>(path: P) -> Result<Self, ()> {
        todo!()
    }

    /// Create an `ImageCacheKey` from the user provided key. The key must implement `Hash`.
    pub fn user<K: Any + Hash>(key: K) -> Self {
        todo!()
    }
}

/// Specifies how long an image should remain in the cache after it isn't used.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ImageCacheLifetime {
    /// Immediately remove the image.
    #[default]
    Immeditate,
    /// Always keep the images stored.
    Indefinite,
    /// Keep the images stored for a specficed time.
    Seconds(u64),
}

/// Specifies the layout and colorspace of the image data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    LRGBA,
    LRGB,
    LMono,
    SRGBA,
    SRGB,
    SMono,
    YUV444,
    YUV422,
}

/// Raw data for an image. This is not an encoded format such as PNG.
#[derive(Clone)]
pub enum ImageData {
    D8(Vec<u8>),
    D16(Vec<u16>),
}

pub(crate) struct ImageCacheRef {
    pub key: ImageCacheKey,
}

struct Image {
    format: ImageFormat,
    width: u32,
    height: u32,
    data: ImageData,
}

struct ImageEntry {
    image: Image,
    refs: usize,
}

/// System for storing images used within the UI.
pub struct ImageCache {
    images: Mutex<HashMap<ImageCacheKey, ImageEntry>>,
}

impl ImageCache {
    pub(crate) fn new() -> Self {
        Self {
            images: Mutex::new(HashMap::new()),
        }
    }

    /// Load an image from raw data. This is not an encoded format like PNG (See `from_bytes`).
    pub fn from_raw_image(
        cache_key: ImageCacheKey,
        lifetime: ImageCacheLifetime,
        format: ImageFormat,
        width: u32,
        height: u32,
        data: ImageData,
    ) -> Result<(), ()> {
        todo!()
    }

    /// Load an image from bytes that are encoded format such as PNG.
    pub fn from_bytes(
        cache_key: ImageCacheKey,
        lifetime: ImageCacheLifetime,
        bytes: Vec<u8>,
    ) -> Result<(), ()> {
        todo!()
    }

    /// Download and load the image from the provided URL.
    pub fn load_from_url<U: AsRef<str>>(lifetime: ImageCacheLifetime, url: U) -> Result<(), ()> {
        todo!()
    }

    /// Open and load image from the provided path.
    pub fn load_from_path<P: AsRef<Path>>(lifetime: ImageCacheLifetime, path: P) -> Result<(), ()> {
        todo!()
    }

    pub(crate) fn obtain_data<K: IntoIterator<Item = ImageCacheKey>>(
        &self,
        target: VkFormat,
        cache_keys: K,
    ) -> Vec<(ImageCacheRef, Vec<u8>)> {
        todo!()
    }
}
