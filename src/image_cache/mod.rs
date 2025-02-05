//! System for storing images used within the UI.

pub(crate) mod convert;

use std::any::{Any, TypeId};
use std::cmp::Ordering;
use std::collections::hash_map::Entry as HashMapEntry;
use std::fmt::Debug;
use std::hash::{BuildHasher, Hash, Hasher};
#[cfg(feature = "image_decode")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use cosmic_text::CacheKey as GlyphCacheKey;
use foldhash::{HashMap, HashMapExt};
use parking_lot::Mutex;
use url::Url;
use vulkano::format::Format as VkFormat;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CacheKeyVariant {
    Url,
    Path,
    Glyph,
    User(TypeId),
}

/// `ImageCacheKey` is a value used to reference an image within the cache.
#[derive(Clone)]
pub struct ImageCacheKey {
    variant: CacheKeyVariant,
    inner: Arc<dyn Any + Send + Sync>,
    hash: u64,
}

impl ImageCacheKey {
    /// Creates an `ImageCacheKey` from the provided URL. This will not load the image.
    pub fn url<U: AsRef<str>>(url: U) -> Result<Self, String> {
        Url::parse(url.as_ref())
            .map_err(|e| format!("Invalid URL: {}", e))
            .map(Self::from)
    }

    /// Returns `true` if this cache key is a url.
    pub fn is_url(&self) -> bool {
        matches!(self.variant, CacheKeyVariant::Url)
    }

    /// Returns a reference to `Url` if the cache key is a url.
    pub fn as_url(&self) -> Option<&Url> {
        if self.is_url() {
            Some(self.inner.downcast_ref::<Url>().unwrap())
        } else {
            None
        }
    }

    /// Create an `ImageCacheKey` from the provided path. This will not load the image.
    pub fn path<P: AsRef<Path>>(path: P) -> Self {
        Self::from(path.as_ref().to_path_buf())
    }

    /// Returns `true` if this cache key is a path.
    pub fn is_path(&self) -> bool {
        matches!(self.variant, CacheKeyVariant::Path)
    }

    /// Returns a reference to `PathBuf` if the cache key is a path.
    pub fn as_path(&self) -> Option<&PathBuf> {
        if self.is_path() {
            Some(self.inner.downcast_ref::<PathBuf>().unwrap())
        } else {
            None
        }
    }

    pub(crate) fn glyph(cache_key: GlyphCacheKey) -> Self {
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        CacheKeyVariant::Glyph.hash(&mut hasher);
        cache_key.hash(&mut hasher);

        Self {
            variant: CacheKeyVariant::Glyph,
            inner: Arc::new(cache_key),
            hash: hasher.finish(),
        }
    }

    pub(crate) fn is_glyph(&self) -> bool {
        matches!(self.variant, CacheKeyVariant::Glyph)
    }

    pub(crate) fn as_glyph(&self) -> Option<&GlyphCacheKey> {
        if self.is_glyph() {
            Some(self.inner.downcast_ref::<GlyphCacheKey>().unwrap())
        } else {
            None
        }
    }

    /// Create an `ImageCacheKey` from the user provided key.
    pub fn user<T>(key: T) -> Self
    where
        T: Any + Hash + Send + Sync,
    {
        let variant = CacheKeyVariant::User(key.type_id());
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        variant.hash(&mut hasher);
        key.hash(&mut hasher);

        Self {
            variant: CacheKeyVariant::User(key.type_id()),
            inner: Arc::new(key),
            hash: hasher.finish(),
        }
    }

    /// Returns `true` if this cache key is the provided user key type.
    pub fn is_user<T>(&self) -> bool
    where
        T: Any,
    {
        if self.is_any_user() {
            self.inner.is::<T>()
        } else {
            false
        }
    }

    /// Returns `true` if this cache key is any user key.
    pub fn is_any_user(&self) -> bool {
        matches!(self.variant, CacheKeyVariant::User(..))
    }

    /// Returns a reference to `T` if the cache key is `T`.
    pub fn as_user<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        self.inner.downcast_ref::<T>()
    }
}

impl Hash for ImageCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl Debug for ImageCacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.variant {
            CacheKeyVariant::Url => f.debug_tuple("Url").field(self.as_url().unwrap()).finish(),
            CacheKeyVariant::Path => {
                f.debug_tuple("Path")
                    .field(self.as_path().unwrap())
                    .finish()
            },
            CacheKeyVariant::Glyph => {
                f.debug_tuple("Glyph")
                    .field(self.as_glyph().unwrap())
                    .finish()
            },
            CacheKeyVariant::User(_) => f.debug_tuple("User").finish_non_exhaustive(),
        }
    }
}

impl PartialEq for ImageCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for ImageCacheKey {}

impl PartialOrd for ImageCacheKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.hash.cmp(&other.hash))
    }
}

impl Ord for ImageCacheKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.hash.cmp(&other.hash)
    }
}

impl From<Url> for ImageCacheKey {
    fn from(url: Url) -> Self {
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        CacheKeyVariant::Url.hash(&mut hasher);
        url.hash(&mut hasher);

        Self {
            variant: CacheKeyVariant::Url,
            inner: Arc::new(url),
            hash: hasher.finish(),
        }
    }
}

impl From<&Path> for ImageCacheKey {
    fn from(path: &Path) -> Self {
        Self::path(path)
    }
}

impl From<PathBuf> for ImageCacheKey {
    fn from(path: PathBuf) -> Self {
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        CacheKeyVariant::Path.hash(&mut hasher);
        path.hash(&mut hasher);

        Self {
            variant: CacheKeyVariant::Path,
            inner: Arc::new(path),
            hash: hasher.finish(),
        }
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
    LMonoA,
    SRGBA,
    SRGB,
    SMono,
    SMonoA,
}

impl ImageFormat {
    pub fn components(&self) -> usize {
        match self {
            Self::LRGBA => 4,
            Self::LRGB => 3,
            Self::LMono => 1,
            Self::LMonoA => 2,
            Self::SRGBA => 4,
            Self::SRGB => 3,
            Self::SMono => 1,
            Self::SMonoA => 2,
        }
    }
}

/// The depth of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageDepth {
    D8,
    D16,
}

/// Raw data for an image. This is not an encoded format such as PNG.
#[derive(Clone)]
pub enum ImageData {
    D8(Vec<u8>),
    D16(Vec<u16>),
}

pub(crate) struct ObtainedImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
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
    unused_since: Option<Instant>,
    lifetime: ImageCacheLifetime,
    associated_data: Arc<dyn Any + Send + Sync>,
}

/// Information about an image including width, height, format and depth.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub depth: ImageDepth,
    pub associated_data: Arc<dyn Any + Send + Sync>,
}

impl ImageInfo {
    pub fn associated_data<T: 'static>(&self) -> Option<&T> {
        self.associated_data.downcast_ref()
    }
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

    /// Load an image from raw data. This is not an encoded format like PNG (See `load_from_bytes`).
    pub fn load_raw_image<D: Any + Send + Sync>(
        &self,
        cache_key: ImageCacheKey,
        lifetime: ImageCacheLifetime,
        format: ImageFormat,
        width: u32,
        height: u32,
        associated_data: D,
        data: ImageData,
    ) -> Result<ImageInfo, String> {
        let expected_data_len = width as usize * height as usize * format.components();

        let (data_len, depth) = match &data {
            ImageData::D8(data) => (data.len(), ImageDepth::D8),
            ImageData::D16(data) => (data.len(), ImageDepth::D16),
        };

        if expected_data_len != data_len {
            return Err(String::from("data invalid length"));
        }

        let associated_data = Arc::new(associated_data);

        match self.images.lock().entry(cache_key) {
            HashMapEntry::Vacant(entry) => {
                entry.insert(ImageEntry {
                    image: Image {
                        format,
                        width,
                        height,
                        data,
                    },
                    refs: 0,
                    unused_since: None,
                    lifetime,
                    associated_data: associated_data.clone(),
                });
            },
            HashMapEntry::Occupied(occupied_entry) => {
                let entry = occupied_entry.get();

                return Ok(ImageInfo {
                    width: entry.image.width,
                    height: entry.image.height,
                    format: entry.image.format,
                    depth: match entry.image.data {
                        ImageData::D8(_) => ImageDepth::D8,
                        ImageData::D16(_) => ImageDepth::D16,
                    },
                    associated_data: entry.associated_data.clone(),
                });
            },
        }

        Ok(ImageInfo {
            width,
            height,
            format,
            depth,
            associated_data,
        })
    }

    /// Load an image from bytes that are encoded format such as PNG.
    #[cfg(feature = "image_decode")]
    pub fn load_from_bytes<B: AsRef<[u8]>, D: Any + Send + Sync>(
        &self,
        cache_key: ImageCacheKey,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        bytes: B,
    ) -> Result<ImageInfo, String> {
        let format = image::guess_format(bytes.as_ref())
            .map_err(|e| format!("Failed to guess image format type: {}", e))?;
        let image = image::load_from_memory_with_format(bytes.as_ref(), format)
            .map_err(|e| format!("Failed to load iamge: {}", e))?;

        let width = image.width();
        let height = image.height();

        let (mut image_format, image_data) = match image {
            image::DynamicImage::ImageLuma8(img) => {
                (ImageFormat::LMono, ImageData::D8(img.into_vec()))
            },
            image::DynamicImage::ImageLumaA8(img) => {
                (ImageFormat::LMonoA, ImageData::D8(img.into_vec()))
            },
            image::DynamicImage::ImageRgb8(img) => {
                (ImageFormat::LRGB, ImageData::D8(img.into_vec()))
            },
            image::DynamicImage::ImageRgba8(img) => {
                (ImageFormat::LRGBA, ImageData::D8(img.into_vec()))
            },
            image::DynamicImage::ImageLuma16(img) => {
                (ImageFormat::LMono, ImageData::D16(img.into_vec()))
            },
            image::DynamicImage::ImageLumaA16(img) => {
                (ImageFormat::LMonoA, ImageData::D16(img.into_vec()))
            },
            image::DynamicImage::ImageRgb16(img) => {
                (ImageFormat::LRGB, ImageData::D16(img.into_vec()))
            },
            image::DynamicImage::ImageRgba16(img) => {
                (ImageFormat::LRGBA, ImageData::D16(img.into_vec()))
            },
            image::DynamicImage::ImageRgb32F(img) => {
                (
                    ImageFormat::LRGB,
                    ImageData::D16(
                        img.into_vec()
                            .into_iter()
                            .map(|val| (val.clamp(0.0, 1.0) * u16::MAX as f32).trunc() as u16)
                            .collect(),
                    ),
                )
            },
            image::DynamicImage::ImageRgba32F(img) => {
                (
                    ImageFormat::LRGBA,
                    ImageData::D16(
                        img.into_vec()
                            .into_iter()
                            .map(|val| (val.clamp(0.0, 1.0) * u16::MAX as f32).trunc() as u16)
                            .collect(),
                    ),
                )
            },
            _ => return Err(String::from("Image format not supported.")),
        };

        let is_linear = !matches!(format, image::ImageFormat::Jpeg);

        if !is_linear {
            image_format = match image_format {
                ImageFormat::LMono => ImageFormat::SMono,
                ImageFormat::LMonoA => ImageFormat::SMonoA,
                ImageFormat::LRGB => ImageFormat::SRGB,
                ImageFormat::LRGBA => ImageFormat::SRGBA,
                _ => unreachable!(),
            };
        }

        self.load_raw_image(
            cache_key,
            lifetime,
            image_format,
            width,
            height,
            associated_data,
            image_data,
        )
    }

    /// Download and load the image from the provided URL.
    #[cfg(feature = "image_download")]
    pub fn load_from_url<U: AsRef<str>, D: Any + Send + Sync>(
        &self,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        url: U,
    ) -> Result<ImageInfo, String> {
        let url = Url::parse(url.as_ref()).map_err(|e| format!("Invalid URL: {}", e))?;
        let mut handle = curl::easy::Easy::new();
        handle.follow_location(true).unwrap();
        handle.url(url.as_str()).unwrap();
        let mut bytes = Vec::new();

        {
            let mut transfer = handle.transfer();

            transfer
                .write_function(|data| {
                    bytes.extend_from_slice(data);
                    Ok(data.len())
                })
                .unwrap();

            transfer
                .perform()
                .map_err(|e| format!("Failed to download: {}", e))?;
        }

        self.load_from_bytes(url.into(), lifetime, associated_data, bytes)
    }

    /// Open and load image from the provided path.
    #[cfg(feature = "image_decode")]
    pub fn load_from_path<P: AsRef<Path>, D: Any + Send + Sync>(
        &self,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        path: P,
    ) -> Result<ImageInfo, String> {
        use std::fs::File;
        use std::io::Read;

        let mut handle =
            File::open(path.as_ref()).map_err(|e| format!("Failed to open file: {}", e))?;

        let mut bytes = Vec::new();

        handle
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        self.load_from_bytes(path.as_ref().into(), lifetime, associated_data, bytes)
    }

    /// Attempt to load an image from `ImageCacheKey`.
    ///
    /// ***Note**: This currently on works for urls and paths.*
    pub fn load_from_cache_key<D: Any + Send + Sync>(
        &self,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        cache_key: &ImageCacheKey,
    ) -> Result<ImageInfo, String> {
        match cache_key.variant {
            CacheKeyVariant::Url => {
                #[cfg(feature = "image_download")]
                {
                    self.load_from_url(
                        lifetime,
                        associated_data,
                        cache_key.as_url().unwrap().as_str(),
                    )
                }
                #[cfg(not(feature = "image_download"))]
                {
                    Err(String::from("'image_download' feature not enabled."))
                }
            },
            CacheKeyVariant::Path => {
                #[cfg(feature = "image_decode")]
                {
                    self.load_from_path(lifetime, associated_data, cache_key.as_path().unwrap())
                }
                #[cfg(not(feature = "image_decode"))]
                {
                    Err(String::from("'image_decode' feature not enabled."))
                }
            },
            CacheKeyVariant::Glyph => {
                Err(String::from(
                    "'load_from_cache_key' does not support glyphs.",
                ))
            },
            CacheKeyVariant::User(..) => {
                Err(String::from(
                    "'load_from_cache_key' does not support user keys.",
                ))
            },
        }
    }

    /// Retrieve image information for multiple images.
    pub fn obtain_image_infos<K: IntoIterator<Item = ImageCacheKey>>(
        &self,
        cache_keys: K,
    ) -> Vec<Option<ImageInfo>> {
        let images = self.images.lock();

        cache_keys
            .into_iter()
            .map(move |cache_key| {
                images.get(&cache_key).map(|entry| {
                    ImageInfo {
                        width: entry.image.width,
                        height: entry.image.height,
                        format: entry.image.format,
                        depth: match entry.image.data {
                            ImageData::D8(_) => ImageDepth::D8,
                            ImageData::D16(_) => ImageDepth::D16,
                        },
                        associated_data: entry.associated_data.clone(),
                    }
                })
            })
            .collect()
    }

    /// Retrieve image information on a single image.
    ///
    /// ***Note:** Where applicable you should use the plural form of this method.*
    pub fn obtain_image_info(&self, cache_key: ImageCacheKey) -> Option<ImageInfo> {
        self.obtain_image_infos([cache_key]).pop().unwrap()
    }

    /// Removes an image from cache. This is useful when an image is added with an `Indefinite`
    /// `ImageCacheLifetime`.
    ///
    /// ***Note:** If an image is in use this will change the lifetime of the image to
    /// `ImageCacheLifetime::Immeditate`.* Allowing it to be removed after it is no longer used.
    pub fn remove_image(&self, cache_key: ImageCacheKey) {
        let mut images = self.images.lock();

        match images.get_mut(&cache_key) {
            Some(entry) if entry.refs > 0 => {
                entry.lifetime = ImageCacheLifetime::Immeditate;
                return;
            },
            Some(_) => (),
            None => return,
        }

        images.remove(&cache_key).unwrap();
    }

    pub(crate) fn obtain_data(
        &self,
        unref_keys: Vec<ImageCacheKey>,
        obtain_keys: Vec<ImageCacheKey>,
        target_format: VkFormat,
    ) -> HashMap<ImageCacheKey, ObtainedImage> {
        let mut images = self.images.lock();

        for cache_key in unref_keys {
            let entry = match images.get_mut(&cache_key) {
                Some(some) => some,
                None => continue,
            };

            if entry.refs > 0 {
                entry.refs -= 1;

                if entry.refs == 0 {
                    entry.unused_since = Some(Instant::now());
                }
            }
        }

        let mut output = HashMap::with_capacity(obtain_keys.len());

        for cache_key in obtain_keys {
            let entry = match images.get_mut(&cache_key) {
                Some(some) => some,
                None => continue,
            };

            entry.refs += 1;

            output.insert(
                cache_key,
                ObtainedImage {
                    width: entry.image.width,
                    height: entry.image.height,
                    data: convert::image_data_to_vulkan_format(
                        entry.image.format,
                        &entry.image.data,
                        target_format,
                    ),
                },
            );
        }

        // Note: It is assumed that an image that has been added and not ever used is to be kept in
        //       the cache. TODO: is this problematic?

        images.retain(|_, entry| {
            if entry.refs == 0 {
                match entry.lifetime {
                    ImageCacheLifetime::Indefinite => true,
                    ImageCacheLifetime::Immeditate => entry.unused_since.is_none(),
                    ImageCacheLifetime::Seconds(seconds) => {
                        match &entry.unused_since {
                            Some(unused_since) => unused_since.elapsed().as_secs() <= seconds,
                            None => true,
                        }
                    },
                }
            } else {
                true
            }
        });

        output
    }
}
