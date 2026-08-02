mod cli_harness;
mod repo_root;

pub(crate) use cli_harness::{CliHarness, CliHarnessOutput};
pub use repo_root::repo_root;

/// Strict journey/core-audit signoff mode.
///
/// When `HARNESS_JOURNEY_STRICT=1` is present, evidence validators require
/// referenced gitignored signoff artifacts (L1/L3/L4/L6 receipts) to exist on
/// disk. Ordinary `cargo nextest` runs skip those paths so that the manifest
/// structure and committed source owners can be validated from a clean checkout.
pub(crate) fn strict_journey_signoff() -> bool {
    std::env::var_os("HARNESS_JOURNEY_STRICT").is_some_and(|v| v == "1")
}
