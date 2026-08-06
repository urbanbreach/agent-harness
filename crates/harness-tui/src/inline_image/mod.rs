//! Inline image pipeline: decode, scale, cache, protocol selection, fallback.

pub mod cache;
pub mod capability;
pub mod pipeline;

pub use cache::{ImageCache, ImageCacheKey};
pub use capability::{GraphicsProtocol, ImageCapability};
pub use pipeline::{ImageError, ImagePipeline, ImageRequest, ImageResult, ImageState};
