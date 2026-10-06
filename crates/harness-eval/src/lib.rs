//! Persistent language kernels and cell lifecycle owned by native Rust.
//!
//! The host supplies tool and provider callbacks. Kernels cannot authorize
//! those operations themselves; Harness's coordinator remains their owner.

mod cell;
mod execution;
mod helpers;
mod javascript;
mod kernel;
mod kernel_tools;
mod memory;
mod metadata;
mod output;
mod packages;
mod request;
mod retention;
mod sandbox;
mod session;
mod settings;
pub use kernel_tools::KernelToolError;
pub use request::normalize_request;
pub use session::Session;
pub use settings::{MemorySettings, SandboxSettings, SessionOptions, Settings};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;
