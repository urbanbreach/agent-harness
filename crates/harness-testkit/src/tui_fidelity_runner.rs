//! Fail-closed PTY execution and shared terminal replay for TUI fidelity evidence.

mod actions;
mod bounded_command;
mod cleanup;
mod error;
mod lifecycle_diagnostics;
mod preflight;
mod process;
mod process_checkpoints;
mod process_io;
mod process_readiness;
pub(crate) mod process_tree;
mod process_wait;
mod pty_child;
mod renderer;
mod renderer_command;
mod runner;
mod runtime_workspace;
mod source_guard;
mod types;
mod util;

pub use cleanup::record_preflight_failure;
pub use error::RunnerError;
pub use runner::{run_compare, run_compare_with_cached_reference};
pub use types::{
    AdapterReceipt, ArtifactDigest, BrowserCapabilities, CandidateBinding, CheckpointReceipt,
    CleanupReceipt, DualRuntimeReceipt, RendererConfig, RunnerConfig, RunnerTiming, RuntimeBinary,
    SourceGuardConfig,
};

pub const RUNNER_RECEIPT_SCHEMA: &str = "harness.tui-fidelity.runner.v2";
