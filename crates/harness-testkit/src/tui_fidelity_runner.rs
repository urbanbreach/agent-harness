//! Fail-closed PTY execution and shared terminal replay for TUI fidelity evidence.

mod actions;
mod error;
mod process;
mod process_tree;
mod process_wait;
mod renderer;
mod runner;
mod runtime_workspace;
mod types;
mod util;

pub use error::RunnerError;
pub use runner::run_compare;
pub use types::{
    AdapterReceipt, ArtifactDigest, BrowserCapabilities, CheckpointReceipt, CleanupReceipt,
    DualRuntimeReceipt, RendererConfig, RunnerConfig, RunnerTiming, RuntimeBinary,
    SourceGuardConfig,
};

pub const RUNNER_RECEIPT_SCHEMA: &str = "harness.tui-fidelity.runner.v1";
