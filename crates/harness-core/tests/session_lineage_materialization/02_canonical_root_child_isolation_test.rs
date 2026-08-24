use harness_core::ids::{EntryId, ProviderRequestId, RunId, SessionId, ToolCallId, TurnId};
use harness_core::session::reducer::replay;
use harness_core::session::{
    CanonicalRecord, CanonicalRecordKind, RecordSequence, RunAttempt, RunStatus, SessionEntry,
    SessionEntryPayload, SessionError,
};

#[test]
fn canonical_root_child_isolation_rejects_mixed_session_records() {
    // arrange
    let records = vec![
        CanonicalRecord {
            session_id: SessionId::new("root-session"),
            sequence: RecordSequence::new(1),
            kind: CanonicalRecordKind::RunStarted {
                attempt: RunAttempt {
                    run_id: RunId::new("root-run"),
                    status: RunStatus::Active,
                    legacy_run_id: None,
                },
            },
        },
        CanonicalRecord {
            session_id: SessionId::new("root-session"),
            sequence: RecordSequence::new(2),
            kind: CanonicalRecordKind::EntryCommitted {
                entry: SessionEntry {
                    id: EntryId::new("root-entry"),
                    parent_id: None,
                    turn_id: Some(TurnId::new("root-turn")),
                    run_id: RunId::new("root-run"),
                    payload: SessionEntryPayload::BranchSummary {
                        summary: "root".to_string(),
                    },
                },
            },
        },
        CanonicalRecord {
            session_id: SessionId::new("child-session"),
            sequence: RecordSequence::new(3),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new("root-entry"),
            },
        },
    ];

    // act
    let result = replay(SessionId::new("root-session"), &records);

    // assert
    assert_eq!(
        result,
        Err(SessionError::MixedSession {
            expected: SessionId::new("root-session"),
            actual: SessionId::new("child-session"),
        })
    );
}

#[test]
fn canonical_root_child_isolation_keeps_identity_types_and_values_distinct() {
    // arrange
    let session_id = SessionId::new("session-value");
    let entry_id = EntryId::new("entry-value");
    let turn_id = TurnId::new("turn-value");
    let run_id = RunId::new("run-value");
    let request_id = ProviderRequestId::new("request-value");
    let tool_call_id = ToolCallId::new("tool-value");

    // act
    let encoded = serde_json::json!({
        "session": session_id,
        "entry": entry_id,
        "turn": turn_id,
        "run": run_id,
        "request": request_id,
        "tool": tool_call_id,
    });

    // assert
    assert_eq!(encoded["session"], "session-value");
    assert_eq!(encoded["entry"], "entry-value");
    assert_eq!(encoded["turn"], "turn-value");
    assert_eq!(encoded["run"], "run-value");
    assert_eq!(encoded["request"], "request-value");
    assert_eq!(encoded["tool"], "tool-value");
}
