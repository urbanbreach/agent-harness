#![allow(clippy::expect_used, reason = "contract-owner fixtures fail fast")]

use harness_testkit::tui_fidelity_scheduler::BoundedScheduler;

#[test]
fn scheduler_rejects_worker_counts_outside_the_typed_contract() {
    // arrange
    // act
    let zero = BoundedScheduler::new(0);
    let above_hard_cap = BoundedScheduler::new(17);
    // assert
    assert!(zero.is_err());
    assert!(above_hard_cap.is_err());
}

#[test]
fn scheduler_reserves_capacity_and_never_exceeds_the_request() {
    // arrange
    // act
    let default = BoundedScheduler::with_default_workers();
    let requested = BoundedScheduler::new(8).expect("valid worker request");
    // assert
    assert!((1..=8).contains(&default.workers()));
    assert!((1..=8).contains(&requested.workers()));
}
