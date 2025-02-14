use std::fmt::{self, Display, Formatter};

/// An error that can occur during image operations.
#[derive(Debug)]
pub enum ImageError {
    /// The length of data provided doesn't match the expected size.
    InvalidLength,
    /// The `ImageKey` is not suitable for the method utilized.
    UnsuitableKey,
    /// A build feature is missing to use this functionality.
    MissingFeature,
    /// An error occurred while decoding.
    #[cfg(feature = "image_decode")]
    Decode(image::ImageError),
    /// An error occurred while opening or reading a file.
    #[cfg(feature = "image_decode")]
    Open(std::io::Error),
    /// An error occurred while parsing a url.
    #[cfg(feature = "image_download")]
    Url(url::ParseError),
    /// An error occurred while downloading.
    #[cfg(feature = "image_download")]
    Download(curl::Error),
}

#[cfg(feature = "image_decode")]
impl From<image::ImageError> for ImageError {
    fn from(e: image::ImageError) -> Self {
        ImageError::Decode(e)
    }
}

#[cfg(feature = "image_decode")]
impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        ImageError::Open(e)
    }
}

#[cfg(feature = "image_download")]
impl From<url::ParseError> for ImageError {
    fn from(e: url::ParseError) -> Self {
        ImageError::Url(e)
    }
}

#[cfg(feature = "image_download")]
impl From<curl::Error> for ImageError {
    fn from(e: curl::Error) -> Self {
        ImageError::Download(e)
    }
}

impl Display for ImageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::InvalidLength => {
                f.write_str("The length of data provided doesn't match the expected size.")
            },
            Self::UnsuitableKey => {
                f.write_str("The `ImageKey` is not suitable for the method utilized.")
            },
            Self::MissingFeature => {
                f.write_str("A build feature is missing to use this functionality.")
            },
            Self::Decode(e) => write!(f, "{}", e),
            Self::Open(e) => write!(f, "{}", e),
            Self::Url(e) => write!(f, "{}", e),
            Self::Download(e) => write!(f, "{}", e),
        }
    }
}
