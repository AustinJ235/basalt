//! System for storing images used within the UI.

pub(crate) mod convert;
#[allow(warnings)] // TODO: Remove
mod image_key;

use std::any::Any;
use std::collections::hash_map::Entry as HashMapEntry;
use std::fmt::Debug;
use std::hash::Hash;
#[cfg(feature = "image_decode")]
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use cosmic_text::CacheKey as GlyphCacheKey;
use foldhash::{HashMap, HashMapExt};
use parking_lot::Mutex;
use url::Url;
use vulkano::format::Format as VkFormat;

pub use self::image_key::ImageKey;
pub(crate) use self::image_key::{ImageMap, ImageMapIntoIterator, ImageSet, ImageSetIntoIterator};

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
    images: Mutex<HashMap<ImageKey, ImageEntry>>,
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
        image_key: ImageKey,
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

        match self.images.lock().entry(image_key) {
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
        image_key: ImageKey,
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
            image_key,
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

    /// Attempt to load an image from `ImageKey`.
    ///
    /// ***Note**: This currently on works for urls and paths.*
    pub fn load_from_key<D: Any + Send + Sync>(
        &self,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        image_key: &ImageKey,
    ) -> Result<ImageInfo, String> {
        if !image_key.is_image_cache() {
            Err(String::from("'ImageKey' is not suitable for `ImageCache`."))
        } else if let Some(url) = image_key.as_url() {
            #[cfg(feature = "image_download")]
            {
                self.load_from_url(lifetime, associated_data, url.as_str())
            }
            #[cfg(not(feature = "image_download"))]
            {
                Err(String::from("'image_download' feature not enabled."))
            }
        } else if let Some(path) = image_key.as_path() {
            #[cfg(feature = "image_decode")]
            {
                self.load_from_path(lifetime, associated_data, path)
            }
            #[cfg(not(feature = "image_decode"))]
            {
                Err(String::from("'image_decode' feature not enabled."))
            }
        } else if image_key.is_glyph() {
            Err(String::from("'load_from_key' does not support glyphs."))
        } else {
            Err(String::from("'load_from_key' does not support user keys."))
        }
    }

    /// Retrieve image information for multiple images.
    pub fn obtain_image_infos<K: IntoIterator<Item = ImageKey>>(
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
    pub fn obtain_image_info(&self, cache_key: ImageKey) -> Option<ImageInfo> {
        self.obtain_image_infos([cache_key]).pop().unwrap()
    }

    /// Removes an image from cache. This is useful when an image is added with an `Indefinite`
    /// `ImageCacheLifetime`.
    ///
    /// ***Note:** If an image is in use this will change the lifetime of the image to
    /// `ImageCacheLifetime::Immeditate`.* Allowing it to be removed after it is no longer used.
    pub fn remove_image(&self, cache_key: ImageKey) {
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
        unref_keys: Vec<ImageKey>,
        obtain_keys: Vec<ImageKey>,
        target_format: VkFormat,
    ) -> HashMap<ImageKey, ObtainedImage> {
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
