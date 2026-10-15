//! Token counting, model identity classification and image size probing.
//!
//! The crate is a pure accounting domain: a [`Counter`] is an immutable pricing
//! domain built from the active reader, [`classify`] and [`encoding_for`] map a
//! provider and model id to a model identity and a native encoding, and
//! [`image_size_from_data_url`] probes image dimensions from a bounded header.

mod count;
mod engines;
mod identity;
mod image;

pub use count::{CountMode, Counter, Encoding, Probe, Reader, ReaderApi, ReasoningPricing};
pub use identity::{classify, encoding_for, AnthropicTier, Family, ModelIdentity};
pub use image::{image_size_from_bytes, image_size_from_data_url, ImageDetail, ImageSize};
