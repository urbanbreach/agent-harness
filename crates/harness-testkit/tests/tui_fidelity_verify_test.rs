#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

#[path = "support/tui_fidelity_verify_completion_support.rs"]
mod completion;
#[path = "support/tui_fidelity_verify_deadline_cache_support.rs"]
mod deadline_cache;
#[path = "support/tui_fidelity_verify_fixture.rs"]
mod fixture;
#[path = "support/harness_bin.rs"]
mod harness_bin;
#[path = "support/tui_fidelity_verify_obligation_support.rs"]
mod obligation;
#[path = "support/tui_fidelity_verify_staging_support.rs"]
mod staging;
