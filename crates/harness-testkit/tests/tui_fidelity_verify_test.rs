#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

#[path = "support/tui_fidelity_verify_deadline_cache.rs"]
mod deadline_cache;
#[path = "support/tui_fidelity_verify_fixture.rs"]
mod fixture;
#[path = "support/tui_fidelity_verify_obligation.rs"]
mod obligation;
#[path = "support/tui_fidelity_verify_staging.rs"]
mod staging;
