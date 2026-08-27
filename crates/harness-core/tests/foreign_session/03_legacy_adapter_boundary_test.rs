use super::*;
use harness_core::ids::RunId;
use harness_core::session::legacy::{
    LegacyAdapterError, LegacyEventLogAdapter, LegacyIdentityNamespace,
};

#[test]
fn legacy_identity_uses_collision_resistant_digests() {
    // arrange
    let run_id = RunId::new("legacy-run");
    let namespace = LegacyIdentityNamespace::new(&run_id);

    // act
    let session_id = namespace.session_id();
    let entry_id = namespace.entry_id(1, "evt-1", "user");

    // assert
    assert!(session_id.as_str().len() >= "legacy-session-".len() + 32);
    assert!(entry_id.as_str().len() >= "legacy-entry-".len() + 32);
}

#[test]
fn legacy_adapter_handles_sequence_overflow_without_panicking() {
    // arrange
    let mut first = sample_envelope(
        1,
        "legacy-run",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    );
    first.seq = u64::MAX;
    first.event_id = "evt-max".to_string();
    let second = sample_envelope(
        0,
        "legacy-run",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "duplicate".to_string(),
        }),
    );

    // act
    let outcome = std::panic::catch_unwind(|| LegacyEventLogAdapter::new().project(&[first, second]));

    // assert
    assert!(matches!(
        outcome,
        Ok(Err(LegacyAdapterError::NonContiguousSequence {
            expected_previous: 0,
            actual: u64::MAX,
        }))
    ));
}
