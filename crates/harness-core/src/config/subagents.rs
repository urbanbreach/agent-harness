//! Subagent settings and pure, snapshot-based definition resolution.
//! Lifecycle, permissions, catalog fetching and dispatch remain coordinator-owned.
use super::*;
use crate::subagent::SubagentIsolationMode;

mod catalog;
mod definitions;
mod discovery;
mod resolution;
mod settings;
pub use catalog::*;
pub use definitions::*;
pub use discovery::*;
pub use resolution::*;
pub use settings::*;

#[cfg(test)]
mod tests;
