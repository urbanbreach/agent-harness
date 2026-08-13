//! Test-only helpers for secret scanning and deterministic verification lanes.
//!
//! Keep runtime-independent testing utilities here; PTY/live workflow code
//! belongs under `crates/harness-testkit/tests/` with local support modules.

pub mod binary_receipt;
pub mod fakes;
pub mod parity;
pub mod reference_authority_receipt;
pub mod secret_scanner;
pub mod simulation;
pub mod tui_dependency_audit;
pub mod tui_fidelity;
pub mod tui_fidelity_aggregate;
pub mod tui_fidelity_cache;
pub mod tui_fidelity_closure;
pub mod tui_fidelity_compare;
pub mod tui_fidelity_deadline;
pub mod tui_fidelity_dependency_cone;
pub mod tui_fidelity_fixture;
pub mod tui_fidelity_matrix;
pub mod tui_fidelity_obligation;
pub mod tui_fidelity_runner;
pub mod tui_fidelity_scheduler;
pub mod tui_fidelity_staging;
pub mod tui_fidelity_task_gate;
pub mod tui_fidelity_verify;
pub mod workspace;

pub use harness_core::UnwrapOrAbort;
