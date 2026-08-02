//! Independent P0 parity contract tests for GEOM / START / COMP / PERM / PAL /
//! TX / SLASH / FILE / PICK / HELP / LIFE.
//!
//! Owned by this file so the cleanroom matrix points at real regression locks
//! (not phantom suite re-runs).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration contract tests use fail-fast asserts for missing layout/render state"
)]

#[path = "support/p0_parity_helpers.rs"]
mod helpers;

#[path = "p0_parity/shell_start_perm_test.rs"]
mod shell_start_perm;

#[path = "p0_parity/composer_slash_file_test.rs"]
mod composer_slash_file;

#[path = "p0_parity/pickers_help_life_test.rs"]
mod pickers_help_life;

#[path = "p0_parity/transcript_test.rs"]
mod transcript;
