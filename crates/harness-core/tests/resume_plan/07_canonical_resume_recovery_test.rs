use harness_core::ids::RunId;
use harness_core::session::legacy::{
    recover_event_history, LegacyHistoryRecoveryError, LegacyWarning,
};

fn recovery_fixture() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "canonical-recovery".into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "recoverable prefix".to_string(),
            }),
        ),
    ]
}

#[test]
fn canonical_resume_recovers_only_supported_final_corrupt_tail() {
    // arrange
    // act
    // assert
    // Given: a contiguous durable prefix followed by one malformed final record.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_fixture";
    let run_dir = temp_dir.path().join(run_id);
    write_events(&run_dir, &recovery_fixture());
    let events_path = run_dir.join("events.jsonl");
    let mut body = std::fs::read_to_string(&events_path).unwrap_or_abort();
    body.push_str("{\"schema_version\":1,\"truncated\":");
    std::fs::write(&events_path, body).unwrap_or_abort();

    // When: the read-only legacy boundary recovers the journal.
    let recovered = recover_event_history(&events_path, &RunId::new(run_id)).unwrap_or_abort();

    // Then: only the malformed final record is dropped and a typed warning is returned.
    assert_eq!(recovered.events(), recovery_fixture());
    assert_eq!(
        recovered.warnings(),
        &[LegacyWarning::RecoveredCorruptFinalLine { line_number: 3 }]
    );
}

#[test]
fn canonical_resume_rejects_non_final_corruption_without_side_effects() {
    // arrange
    // act
    // assert
    // Given: malformed JSON between two otherwise valid journal records.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_fixture";
    let events_path = temp_dir.path().join(run_id).join("events.jsonl");
    std::fs::create_dir_all(events_path.parent().unwrap_or_abort()).unwrap_or_abort();
    let events = recovery_fixture();
    let body = format!(
        "{}\n{{not-json}}\n{}\n",
        serde_json::to_string(&events[0]).unwrap_or_abort(),
        serde_json::to_string(&events[1]).unwrap_or_abort()
    );
    std::fs::write(&events_path, &body).unwrap_or_abort();
    let before = std::fs::read(&events_path).unwrap_or_abort();

    // When: recovery encounters the non-final malformed record.
    let error = recover_event_history(&events_path, &RunId::new(run_id))
        .expect_err("non-final corruption must fail closed");

    // Then: rejection is typed and the journal remains byte-for-byte unchanged.
    assert!(matches!(
        error,
        LegacyHistoryRecoveryError::InvalidEvent { line_number: 2, .. }
    ));
    assert_eq!(std::fs::read(&events_path).unwrap_or_abort(), before);
}

#[test]
fn canonical_resume_rejects_complete_invalid_final_event() {
    // arrange
    // act
    // assert
    // Given: a physically complete final JSON record with an invalid typed payload.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_complete_invalid_final";
    let events_path = temp_dir.path().join("events.jsonl");
    let mut valid = envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "invalid-final".into(),
            workspace_root: "/workspace/project".to_string(),
        }),
    );
    valid.run_id = RunId::new(run_id);
    let mut invalid_event = envelope(
        2,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "complete record".to_string(),
        }),
    );
    invalid_event.run_id = RunId::new(run_id);
    let mut invalid = serde_json::to_value(invalid_event).unwrap_or_abort();
    invalid["payload"]["event_type"] =
        serde_json::Value::String("future_unknown_event".to_string());
    std::fs::write(
        &events_path,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&valid).unwrap_or_abort(),
            serde_json::to_string(&invalid).unwrap_or_abort()
        ),
    )
    .unwrap_or_abort();

    // When: recovery reads the complete invalid typed record.
    let error = recover_event_history(&events_path, &RunId::new(run_id))
        .expect_err("complete invalid final event must fail closed");

    // Then: it is rejected instead of being classified as a truncated append.
    assert!(matches!(
        error,
        LegacyHistoryRecoveryError::InvalidEvent { line_number: 2, .. }
    ));
}

#[test]
fn canonical_resume_rejects_event_from_unexpected_run() {
    // arrange
    // act
    // assert
    // Given: a valid event envelope from another run.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let events_path = temp_dir.path().join("events.jsonl");
    std::fs::write(
        &events_path,
        format!(
            "{}\n",
            serde_json::to_string(&{
                let mut foreign = envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "foreign".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
                );
                foreign.run_id = RunId::new("run_foreign");
                foreign
            })
            .unwrap_or_abort()
        ),
    )
    .unwrap_or_abort();

    // When: recovery is scoped to a different expected run.
    let error = recover_event_history(&events_path, &RunId::new("run_expected"))
        .expect_err("foreign run must fail at the recovery boundary");

    // Then: the typed run mismatch identifies both ids.
    assert!(matches!(
        error,
        LegacyHistoryRecoveryError::RunMismatch {
            line_number: 1,
            ref expected,
            ref actual,
        }
        if expected == &RunId::new("run_expected") && actual == &RunId::new("run_foreign")
    ));
}
