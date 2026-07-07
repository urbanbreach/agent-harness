//! Formatter subsystem entry point.
//!
//! Split into three files to stay under the 250 pure LOC ceiling:
//! - `runner.rs` — execution side: `run_formatter_for_path`, `run_formatter_for_path_with_discovery`,
//!   `run_single_formatter`, `validate_path_inside_workspace`.
//! - `resolver.rs` — resolution side: `ResolvedFormatter`, `resolve_formatters`,
//!   `file_extension`, `extension_matches`, `normalize_extension`, `resolve_formatter_names`.
//! - `mod.rs` — module declarations, public re-exports, and shared constants.

mod caching;
mod discovery;
mod find_up;
mod real_discovery;
mod registry;
mod resolver;
mod runner;

pub use discovery::{DiscoveryContext, FormatterDiscovery};
pub use real_discovery::RealFormatterDiscovery;
pub use registry::BUILTIN_FORMATTERS;

pub(crate) use caching::CachingFormatterDiscovery;

#[cfg(test)]
pub(in crate::coord) use discovery::FakeFormatterDiscovery;

pub use runner::run_formatter_for_path;
pub(in crate::coord) use runner::run_formatter_for_path_with_discovery;

pub use resolver::{formatter_status, FormatterStatus};

#[cfg(test)]
pub(in crate::coord) use resolver::resolve_formatter_names;
