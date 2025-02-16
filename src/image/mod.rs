//! Image loading, caching and keys.

pub(crate) mod convert;
mod error;
mod image_key;

use std::any::Any;
use std::fmt::Debug;
use std::hash::Hash;
#[cfg(feature = "image_decode")]
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cosmic_text::CacheKey as GlyphCacheKey;
use parking_lot::Mutex;
#[cfg(feature = "image_download")]
use url::Url;

mod vko {
    pub use vulkano::format::Format;
}

pub use self::error::ImageError;
pub use self::image_key::ImageKey;
pub(crate) use self::image_key::{ImageMap, ImageSet};

/// Specifies how long an image should remain in the cache after it isn't used.
///
/// ***Note:** Once an image is loaded into the `ImageCache` it will remain in the cache until it is
///            used. Once it is used at least once, the specificed lifetime will take effect.*
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ImageCacheLifetime {
    /// Immediately remove the image.
    ///
    /// This is the default.
    #[default]
    Immeditate,
    /// Always keep the images stored.
    Indefinite,
    /// Keep the images stored for a specified duration.
    Duration(Duration),
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
    images: Mutex<ImageMap<ImageEntry>>,
}

impl ImageCache {
    pub(crate) fn new() -> Self {
        Self {
            images: Mutex::new(ImageMap::new()),
        }
    }

    /// Load an image from raw data. This is not an encoded format like PNG (See `load_from_bytes`).
    pub fn load_raw_image<D>(
        &self,
        image_key: ImageKey,
        lifetime: ImageCacheLifetime,
        format: ImageFormat,
        width: u32,
        height: u32,
        associated_data: D,
        data: ImageData,
    ) -> Result<ImageInfo, ImageError>
    where
        D: Any + Send + Sync,
    {
        let expected_data_len = width as usize * height as usize * format.components();

        let data_len = match &data {
            ImageData::D8(data) => data.len(),
            ImageData::D16(data) => data.len(),
        };

        if expected_data_len != data_len {
            return Err(ImageError::InvalidLength);
        }

        let associated_data = Arc::new(associated_data);

        Ok(self.images.lock().try_insert_then(
            &image_key,
            || {
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
                    associated_data,
                }
            },
            |entry| {
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
            },
        ))
    }

    /// Load an image from bytes that are encoded format such as PNG.
    #[cfg(feature = "image_decode")]
    pub fn load_from_bytes<B, D>(
        &self,
        image_key: ImageKey,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        bytes: B,
    ) -> Result<ImageInfo, ImageError>
    where
        B: AsRef<[u8]>,
        D: Any + Send + Sync,
    {
        let format = image::guess_format(bytes.as_ref())?;
        let image = image::load_from_memory_with_format(bytes.as_ref(), format)?;
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
            _ => unimplemented!(),
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
    pub fn load_from_url<U, D>(
        &self,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        url: U,
    ) -> Result<ImageInfo, ImageError>
    where
        U: AsRef<str>,
        D: Any + Send + Sync,
    {
        let url = Url::parse(url.as_ref())?;
        let mut handle = curl::easy::Easy::new();
        handle.follow_location(true)?;
        handle.url(url.as_str())?;
        let mut bytes = Vec::new();

        {
            let mut transfer = handle.transfer();

            transfer.write_function(|data| {
                bytes.extend_from_slice(data);
                Ok(data.len())
            })?;

            transfer.perform()?;
        }

        self.load_from_bytes(url.into(), lifetime, associated_data, bytes)
    }

    /// Open and load image from the provided path.
    #[cfg(feature = "image_decode")]
    pub fn load_from_path<P, D>(
        &self,
        lifetime: ImageCacheLifetime,
        associated_data: D,
        path: P,
    ) -> Result<ImageInfo, ImageError>
    where
        P: AsRef<Path>,
        D: Any + Send + Sync,
    {
        use std::fs::File;
        use std::io::Read;

        let mut handle = File::open(path.as_ref())?;
        let mut bytes = Vec::new();
        handle.read_to_end(&mut bytes)?;
        self.load_from_bytes(path.as_ref().into(), lifetime, associated_data, bytes)
    }

    /// Attempt to load an image from `ImageKey`.
    ///
    /// ***Note**: This currently on works for urls and paths.*
    pub fn load_from_key<D>(
        &self,
        _lifetime: ImageCacheLifetime,
        _associated_data: D,
        image_key: &ImageKey,
    ) -> Result<ImageInfo, ImageError>
    where
        D: Any + Send + Sync,
    {
        if image_key.is_url() {
            #[cfg(feature = "image_download")]
            {
                self.load_from_url(
                    _lifetime,
                    _associated_data,
                    image_key.as_url().unwrap().as_str(),
                )
            }
            #[cfg(not(feature = "image_download"))]
            {
                Err(ImageError::MissingFeature)
            }
        } else if image_key.is_path() {
            #[cfg(feature = "image_decode")]
            {
                self.load_from_path(_lifetime, _associated_data, image_key.as_path().unwrap())
            }
            #[cfg(not(feature = "image_decode"))]
            {
                Err(ImageError::MissingFeature)
            }
        } else {
            Err(ImageError::UnsuitableKey)
        }
    }

    /// Retrieve image information for multiple images.
    pub fn obtain_image_infos<'a, K>(&self, image_keys: K) -> Vec<Option<ImageInfo>>
    where
        K: IntoIterator<Item = &'a ImageKey>,
    {
        let images = self.images.lock();

        image_keys
            .into_iter()
            .map(move |image_key| {
                images.get(image_key).map(|entry| {
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
    pub fn obtain_image_info(&self, image_key: &ImageKey) -> Option<ImageInfo> {
        self.obtain_image_infos([image_key]).pop().unwrap()
    }

    /// Removes an image from cache. This is useful when an image is added with an `Indefinite`
    /// `ImageCacheLifetime`.
    ///
    /// ***Note:** If an image is in use this will change the lifetime of the image to
    /// `ImageCacheLifetime::Immeditate`.* Allowing it to be removed after it is no longer used.
    pub fn remove_image(&self, cache_key: &ImageKey) {
        let mut images = self.images.lock();

        match images.get_mut(cache_key) {
            Some(entry) if entry.refs > 0 => {
                entry.lifetime = ImageCacheLifetime::Immeditate;
                return;
            },
            Some(_) => (),
            None => return,
        }

        images.remove(cache_key).unwrap();
    }

    pub(crate) fn obtain_data(
        &self,
        unref_keys: Vec<ImageKey>,
        obtain_keys: Vec<ImageKey>,
        target_format: vko::Format,
    ) -> ImageMap<ObtainedImage> {
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

        let mut output = ImageMap::with_capacity(obtain_keys.len());

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
                    ImageCacheLifetime::Duration(duration) => {
                        match &entry.unused_since {
                            Some(unused_since) => unused_since.elapsed() <= duration,
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
