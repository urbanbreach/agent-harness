//! Fail-closed PTY execution and shared terminal replay for TUI fidelity evidence.

mod actions;
mod bounded_command;
mod cleanup;
mod error;
mod interaction_queue;
mod lifecycle_diagnostics;
mod native_sidecar;
mod preflight;
mod presentation_receipt;
mod presentation_validation;
mod process;
mod process_checkpoints;
mod process_io;
mod process_readiness;
pub(crate) mod process_tree;
mod process_wait;
mod pty_child;
mod pty_observation;
mod receipt_presentation;
mod renderer;
mod renderer_command;
mod runner;
mod runtime_workspace;
mod source_guard;
mod types;
mod util;

pub use cleanup::record_preflight_failure;
pub use error::RunnerError;
pub use native_sidecar::read_native_trace;
pub use presentation_receipt::*;
pub use presentation_validation::{validate_presentation_evidence, PresentationValidationError};
pub use process_io::PtyRead;
pub use pty_observation::{PtyObservationError, PtyObserver};
pub use runner::{run_compare, run_compare_with_cached_reference};
pub use types::{
    AdapterReceipt, ArtifactDigest, BrowserCapabilities, CandidateBinding, CheckpointReceipt,
    CleanupReceipt, DualRuntimeReceipt, PresentationCaptureBinding, RendererConfig, RunnerConfig,
    RunnerTiming, RuntimeBinary, SourceGuardConfig,
};

pub const RUNNER_RECEIPT_SCHEMA: &str = "harness.tui-fidelity.runner.v3";
