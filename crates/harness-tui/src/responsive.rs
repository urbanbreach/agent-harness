//! Responsive viewport leaf types for the TUI responsive/terminal shard.
//!
//! These are plain value objects with no shared registry or app-state
//! dependency. They capture the deterministic viewport classification and
//! frame-plan summary that the responsive parity rows (RESP-*) require,
//! without touching the shared `layout.rs` root.

pub mod density;
pub mod viewport;

pub use density::{density_for_viewport, spacing_density_for};
pub use viewport::{
    VIEWPORT_100x30, VIEWPORT_120x40, VIEWPORT_120x50, VIEWPORT_60x20, VIEWPORT_79x24,
    VIEWPORT_80x24, ViewportClassification, ViewportId, ViewportPlan, VIEWPORT_WIDE,
};
