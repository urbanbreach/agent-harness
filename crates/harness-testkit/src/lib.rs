//! Test-only helpers for secret scanning and deterministic verification lanes.
//!
//! Keep runtime-independent testing utilities here; PTY/live workflow code
//! belongs under `crates/harness-testkit/tests/` with local support modules.

pub mod fakes;
pub mod secret_scanner;
pub mod simulation;
pub mod workspace;
