use std::collections::BTreeSet;
use std::sync::Arc;

use harness_testkit::tui_fidelity_scheduler::BoundedScheduler;
use harness_testkit::tui_fidelity_staging::{AttemptPolicy, KeyState, StagingArea};
use harness_testkit::tui_fidelity_verify::{
    build_plan, execute_plan, PlanSelection, VerificationProfile, VerifyConfig,
};

use super::fixture::{synthetic_fixture, synthetic_keys};

#[test]
fn staging_resume_resets_interrupted_and_required_cancelled_keys() {
    // Given: an interrupted development attempt with running and cancelled keys.
    let root = tempfile::tempdir().expect("evidence root");
    let keys = synthetic_keys();
    let staging = StagingArea::open(
        root.path(),
        &"a".repeat(40),
        "attempt-1",
        &keys,
        AttemptPolicy::Development,
    )
    .expect("staging area");
    staging.mark_running(&keys[0]).expect("running state");
    staging.mark_cancelled(&keys[1]).expect("cancelled state");

    // When: the same attempt is resumed.
    let resumed = StagingArea::open(
        root.path(),
        &"a".repeat(40),
        "attempt-1",
        &keys,
        AttemptPolicy::Development,
    )
    .expect("resumed staging area");

    // Then: required interrupted work is pending again.
    assert_eq!(
        resumed.state(&keys[0]).expect("first state"),
        KeyState::Pending
    );
    assert_eq!(
        resumed.state(&keys[1]).expect("second state"),
        KeyState::Pending
    );
}

#[test]
fn final_all_rejects_resume_after_one_genuine_failure() {
    // Given: a final-all attempt with one failed key.
    let root = tempfile::tempdir().expect("evidence root");
    let keys = synthetic_keys();
    let staging = StagingArea::open(
        root.path(),
        &"b".repeat(40),
        "attempt-final",
        &keys,
        AttemptPolicy::FinalAll,
    )
    .expect("staging area");
    staging.mark_running(&keys[0]).expect("running state");
    staging
        .mark_failed(&keys[0], "genuine mismatch")
        .expect("failed state");

    // When: final-all resume is attempted.
    let error = StagingArea::open(
        root.path(),
        &"b".repeat(40),
        "attempt-final",
        &keys,
        AttemptPolicy::FinalAll,
    )
    .expect_err("final failure must be terminal");

    // Then: the retry guard identifies the genuine final failure.
    assert!(error.to_string().contains("final-all failure"));
}

#[test]
fn development_resume_stops_retrying_after_two_genuine_failures() {
    // Given: a development key that has failed twice across resumed attempts.
    let root = tempfile::tempdir().expect("evidence root");
    let keys = synthetic_keys();
    let candidate = "d".repeat(40);
    let attempt = "attempt-retry-cap";
    let first = StagingArea::open(
        root.path(),
        &candidate,
        attempt,
        &keys,
        AttemptPolicy::Development,
    )
    .expect("first attempt");
    first.mark_running(&keys[0]).expect("first running");
    first.mark_failed(&keys[0], "first").expect("first failure");
    let second = StagingArea::open(
        root.path(),
        &candidate,
        attempt,
        &keys,
        AttemptPolicy::Development,
    )
    .expect("second attempt");
    second.mark_running(&keys[0]).expect("second running");
    second
        .mark_failed(&keys[0], "second")
        .expect("second failure");

    // When: development resume is requested after the retry cap.
    let capped = StagingArea::open(
        root.path(),
        &candidate,
        attempt,
        &keys,
        AttemptPolicy::Development,
    )
    .expect("capped attempt");

    // Then: the failed key remains terminal instead of becoming pending.
    assert_eq!(
        capped.state(&keys[0]).expect("capped state"),
        KeyState::Failed
    );
}

#[test]
fn scheduler_executes_each_deduplicated_key_once_with_bounded_isolation() {
    // Given: three deduplicated jobs and a two-worker staging attempt.
    let root = tempfile::tempdir().expect("evidence root");
    let keys = synthetic_keys();
    let staging = StagingArea::open(
        root.path(),
        &"c".repeat(40),
        "attempt-scheduler",
        &keys,
        AttemptPolicy::Development,
    )
    .expect("staging area");
    let scheduler = BoundedScheduler::new(2).expect("scheduler");
    let executed = Arc::new(std::sync::Mutex::new(BTreeSet::new()));

    // When: all keys are dispatched through isolated job directories.
    let report = scheduler
        .run(&keys, &staging, {
            let executed = Arc::clone(&executed);
            move |key, isolation| {
                executed
                    .lock()
                    .map_err(|error| error.to_string())?
                    .insert(key.stable_id().map_err(|error| error.to_string())?);
                let receipt = isolation.evidence_dir.join("receipt.json");
                std::fs::write(&receipt, b"passed").map_err(|error| error.to_string())?;
                Ok(receipt)
            }
        })
        .expect("scheduled verification");

    // Then: exactly three jobs passed and no key executed twice.
    assert_eq!(report.passed, 3);
    assert_eq!(executed.lock().expect("executed keys").len(), 3);
    assert!(keys
        .iter()
        .all(|key| staging.state(key).expect("key state") == KeyState::Passed));
}

#[test]
fn synthetic_fixture_executes_and_publishes_each_profile() {
    // Given: one synthetic plan for each profile.
    let (inventory, manifest) = synthetic_fixture();
    let changed = inventory
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    let profiles = [
        (VerificationProfile::All, None),
        (VerificationProfile::Changed, Some(&changed)),
        (VerificationProfile::Motion, None),
    ];

    // When: every plan runs through scheduling, staging, and publication.
    for (index, (profile, selected)) in profiles.into_iter().enumerate() {
        let plan = build_plan(
            PlanSelection {
                profile,
                changed: selected,
            },
            &inventory,
            &manifest,
        )
        .expect("profile plan");
        let root = tempfile::tempdir().expect("profile evidence");
        let receipt = execute_plan(
            &VerifyConfig {
                candidate_sha: format!("{:040}", index + 1),
                attempt_id: format!("profile-{index}"),
                evidence_root: root.path().to_path_buf(),
                workers: Some(2),
            },
            &plan,
            |_key, isolation| {
                let artifact = isolation.evidence_dir.join("receipt.json");
                std::fs::write(&artifact, b"passed").map_err(|error| error.to_string())?;
                Ok(artifact)
            },
        )
        .expect("profile execution");

        // Then: final all seals once; development profiles retain staging evidence.
        assert_eq!(receipt.sealed, profile == VerificationProfile::All);
        assert!(std::path::Path::new(&receipt.evidence_path).is_dir());
    }
}
