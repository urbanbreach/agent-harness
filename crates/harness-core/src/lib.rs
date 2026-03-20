//! Core runtime and domain crate for Agent Harness.
//!
//! This crate owns event schema/building, coordinator invariants, scheduling,
//! permissions, projections, configuration, and deterministic storage. Keep
//! state transitions and persisted event rules here rather than in UI or tool
//! crates.

pub mod agent;
pub mod clock;
pub mod config;
pub mod coord;
pub mod edit;
pub mod event;
pub mod perm;
pub mod proj;
pub mod redact;
pub mod sched;
pub mod store;
pub mod tool;
