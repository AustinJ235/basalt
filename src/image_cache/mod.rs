//! System for storing images used within the UI.

mod convert;

use std::any::{Any, TypeId};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Instant;

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
    pub fn url<U: AsRef<str>>(url: U) -> Result<Self, String> {
        Ok(Self::Url(
            Url::parse(url.as_ref()).map_err(|e| format!("Invalid URL: {}", e))?,
        ))
    }

    /// Create an `ImageCacheKey` from the provided path. This will not load the image.
    pub fn path<P: Into<String>>(path: P) -> Self {
        Self::Path(PathBuf::from(path.into()))
    }

    /// Create an `ImageCacheKey` from the user provided key. The key must implement `Hash`.
    pub fn user<K: Any + Hash>(key: K) -> Self {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        Self::User(key.type_id(), hasher.finish())
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
    YUV422,
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
            Self::YUV422 => 3,
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
}

/// Information about an image including width, height, format and depth.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub depth: ImageDepth,
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
        &self,
        cache_key: ImageCacheKey,
        lifetime: ImageCacheLifetime,
        format: ImageFormat,
        width: u32,
        height: u32,
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

        self.images.lock().insert(
            cache_key,
            ImageEntry {
                image: Image {
                    format,
                    width,
                    height,
                    data,
                },
                refs: 0,
                unused_since: None,
                lifetime,
            },
        );

        Ok(ImageInfo {
            width,
            height,
            format,
            depth,
        })
    }

    /// Load an image from bytes that are encoded format such as PNG.
    #[cfg(feature = "image_decode")]
    pub fn from_bytes<B: AsRef<[u8]>>(
        &self,
        cache_key: ImageCacheKey,
        lifetime: ImageCacheLifetime,
        bytes: B,
    ) -> Result<ImageInfo, String> {
        let format = image::guess_format(bytes.as_ref())
            .map_err(|e| format!("Failed to guess image format type: {}", e))?;
        let image = image::load_from_memory_with_format(bytes.as_ref(), format)
            .map_err(|e| format!("Failed to load iamge: {}", e))?;

        let is_linear = match format {
            image::ImageFormat::Jpeg => false,
            _ => true,
        };

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
                            .map(|val| {
                                (val.clamp(0.0, 1.0) * u16::max_value() as f32).trunc() as u16
                            })
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
                            .map(|val| {
                                (val.clamp(0.0, 1.0) * u16::max_value() as f32).trunc() as u16
                            })
                            .collect(),
                    ),
                )
            },
            _ => return Err(format!("Image format not supported.")),
        };

        let is_linear = match format {
            image::ImageFormat::Jpeg => false,
            _ => true,
        };

        if !is_linear {
            image_format = match image_format {
                ImageFormat::LMono => ImageFormat::SMono,
                ImageFormat::LMonoA => ImageFormat::SMonoA,
                ImageFormat::LRGB => ImageFormat::SRGB,
                ImageFormat::LRGBA => ImageFormat::SRGBA,
                _ => unreachable!(),
            };
        }

        self.from_raw_image(cache_key, lifetime, image_format, width, height, image_data)
    }

    /// Download and load the image from the provided URL.
    #[cfg(feature = "image_download")]
    pub fn load_from_url<U: AsRef<str>>(
        &self,
        lifetime: ImageCacheLifetime,
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
                    bytes.extend_from_slice(&data);
                    Ok(data.len())
                })
                .unwrap();

            transfer
                .perform()
                .map_err(|e| format!("Failed to download: {}", e))?;
        }

        self.from_bytes(ImageCacheKey::Url(url), lifetime, bytes)
    }

    /// Open and load image from the provided path.
    #[cfg(feature = "image_decode")]
    pub fn load_from_path<P: AsRef<Path>>(
        &self,
        lifetime: ImageCacheLifetime,
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

        self.from_bytes(
            ImageCacheKey::Path(path.as_ref().to_path_buf()),
            lifetime,
            bytes,
        )
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

    pub(crate) fn obtain_data<U, O>(
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

            let data = convert::image_data_to_vulkan_format(
                entry.image.format,
                &entry.image.data,
                target_format,
            );

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
                    ImageCacheLifetime::Immeditate => !entry.unused_since.is_some(),
                    ImageCacheLifetime::Seconds(seconds) => {
                        match &entry.unused_since {
                            Some(unused_since) => {
                                if unused_since.elapsed().as_secs() > seconds {
                                    false
                                } else {
                                    true
                                }
                            },
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
