use std::fmt::{Display, Formatter};

use super::{
    cache::{CachedImage, ImageCache, ImageCacheKey},
    capability::{GraphicsProtocol, ImageCapability},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageState {
    Pending,
    Decoding,
    Rendered(Vec<u8>),
    Failed(ImageError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageResult {
    pub state: ImageState,
    pub width: u16,
    pub height: u16,
    pub from_cache: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRequest {
    pub source: Vec<u8>,
    pub target_width: u16,
    pub target_height: u16,
    pub post_flush: bool,
}

impl ImageRequest {
    pub fn cache_key(&self, protocol: GraphicsProtocol) -> ImageCacheKey {
        ImageCacheKey::new(
            &self.source,
            self.target_width,
            self.target_height,
            protocol,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    CorruptInput,
    Oversized { actual: (u32, u32), max: (u32, u32) },
    UnsupportedProtocol,
    DecodeTimeout,
    Cancelled,
}

impl Display for ImageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CorruptInput => formatter.write_str("corrupt image input"),
            Self::Oversized { actual, max } => {
                write!(
                    formatter,
                    "image dimensions {}x{} exceed {}x{}",
                    actual.0, actual.1, max.0, max.1
                )
            }
            Self::UnsupportedProtocol => formatter.write_str("unsupported graphics protocol"),
            Self::DecodeTimeout => formatter.write_str("image decode timed out"),
            Self::Cancelled => formatter.write_str("image rendering was cancelled"),
        }
    }
}

impl std::error::Error for ImageError {}

pub struct ImagePipeline {
    capability: ImageCapability,
    cache: ImageCache,
    flush_complete: bool,
}

impl ImagePipeline {
    pub fn new(capability: ImageCapability) -> Self {
        Self {
            capability,
            cache: ImageCache::default(),
            flush_complete: false,
        }
    }

    pub fn mark_flush_complete(&mut self) {
        self.flush_complete = true;
    }

    pub fn reset_flush(&mut self) {
        self.flush_complete = false;
    }

    pub fn submit(&mut self, request: ImageRequest, tick: u64) -> ImageResult {
        let dimensions = (request.target_width, request.target_height);
        if !self.flush_complete {
            return ImageResult {
                state: ImageState::Pending,
                width: dimensions.0,
                height: dimensions.1,
                from_cache: false,
            };
        }
        if !self.capability.is_available() {
            return ImageResult {
                state: ImageState::Failed(ImageError::UnsupportedProtocol),
                width: dimensions.0,
                height: dimensions.1,
                from_cache: false,
            };
        }
        let key = request.cache_key(self.capability.protocol);
        if let Some(image) = self.cache.get(&key) {
            return ImageResult {
                state: ImageState::Rendered(image.rendered_bytes.clone()),
                width: image.width,
                height: image.height,
                from_cache: true,
            };
        }
        if request.source.is_empty() {
            return ImageResult {
                state: ImageState::Failed(ImageError::CorruptInput),
                width: dimensions.0,
                height: dimensions.1,
                from_cache: false,
            };
        }
        let actual = (
            u32::from(request.target_width),
            u32::from(request.target_height),
        );
        let max = (self.capability.max_width, self.capability.max_height);
        if actual.0 > max.0 || actual.1 > max.1 {
            return ImageResult {
                state: ImageState::Failed(ImageError::Oversized { actual, max }),
                width: dimensions.0,
                height: dimensions.1,
                from_cache: false,
            };
        }
        let _ = tick;
        ImageResult {
            state: ImageState::Decoding,
            width: dimensions.0,
            height: dimensions.1,
            from_cache: false,
        }
    }

    pub fn complete_decode(
        &mut self,
        key: &ImageCacheKey,
        rendered: Vec<u8>,
        width: u16,
        height: u16,
        tick: u64,
    ) {
        self.cache.insert(CachedImage {
            key: *key,
            rendered_bytes: rendered,
            width,
            height,
            created_at_tick: tick,
        });
    }

    pub fn cancel(&mut self, _key: &ImageCacheKey) {}

    pub fn cache(&self) -> &ImageCache {
        &self.cache
    }

    pub fn capability(&self) -> &ImageCapability {
        &self.capability
    }
}

impl Default for ImagePipeline {
    fn default() -> Self {
        Self::new(ImageCapability::unsupported())
    }
}
