//! Test-only helpers for secret scanning and deterministic verification lanes.
//!
//! Keep runtime-independent testing utilities here; PTY/live workflow code
//! belongs under `crates/harness-testkit/tests/` with local support modules.

pub mod binary_receipt;
pub mod fakes;
pub mod parity;
pub mod secret_scanner;
pub mod simulation;
pub mod tui_fidelity;
pub mod tui_fidelity_runner;
pub mod workspace;

pub use harness_core::UnwrapOrAbort;
