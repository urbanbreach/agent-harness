use harness_core::ids::{EntryId, RunId, SessionId, ToolCallId, TurnId};
use harness_core::session::reducer::replay as replay_session;
use harness_core::session::{
    AssistantPart, AssistantToolCall, CanonicalRecord, CanonicalRecordKind, CanonicalSession,
    RecordSequence, RunAttempt, RunStatus, SessionEntry, SessionEntryPayload, SessionError,
    ToolResultStatus,
};

fn record(sequence: u64, entry: SessionEntry) -> CanonicalRecord {
    CanonicalRecord {
        session_id: SessionId::new("session-tools"),
        sequence: RecordSequence::new(sequence),
        kind: CanonicalRecordKind::EntryCommitted { entry },
    }
}

fn replay(
    session_id: SessionId,
    records: &[CanonicalRecord],
) -> Result<CanonicalSession, SessionError> {
    let mut with_run = Vec::with_capacity(records.len() + 1);
    with_run.push(CanonicalRecord {
        session_id: session_id.clone(),
        sequence: RecordSequence::new(1),
        kind: CanonicalRecordKind::RunStarted {
            attempt: RunAttempt {
                run_id: RunId::new("run-tools"),
                status: RunStatus::Active,
                legacy_run_id: None,
            },
        },
    });
    with_run.extend(records.iter().cloned().map(|mut record| {
        record.sequence = RecordSequence::new(record.sequence.get() + 1);
        record
    }));
    replay_session(session_id, &with_run)
}

fn assistant(id: &str, tool_call_id: &str) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: None,
        turn_id: Some(TurnId::new("turn-tools")),
        run_id: RunId::new("run-tools"),
        payload: SessionEntryPayload::AssistantMessage {
            parts: vec![AssistantPart::ToolCall(AssistantToolCall {
                tool_call_id: ToolCallId::new(tool_call_id),
                provider_tool_call_id: None,
                tool_id: "read".to_string(),
                args_summary: "file".to_string(),
                args_digest: "digest".to_string(),
                provider_call_id: None,
            })],
            provenance: None,
        },
    }
}

fn tool_result(id: &str, assistant_id: &str, tool_call_id: &str) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: Some(EntryId::new(assistant_id)),
        turn_id: Some(TurnId::new("turn-tools")),
        run_id: RunId::new("run-tools"),
        payload: SessionEntryPayload::ToolResult {
            tool_call_id: ToolCallId::new(tool_call_id),
            requesting_assistant_entry_id: EntryId::new(assistant_id),
            status: ToolResultStatus::Succeeded,
            output_summary: Some("ok".to_string()),
            output_digest: Some("output-digest".to_string()),
            output_json: None,
        },
    }
}

#[test]
fn canonical_tool_pairing_rejects_orphan_duplicate_off_path_and_split_results() {
    // arrange
    let cases = [
        (
            vec![record(1, tool_result("result", "missing", "tool-1"))],
            SessionError::OrphanToolResult {
                tool_call_id: ToolCallId::new("tool-1"),
            },
        ),
        (
            vec![
                record(1, assistant("assistant-a", "tool-1")),
                record(2, assistant("assistant-b", "tool-1")),
            ],
            SessionError::DuplicateToolCall {
                tool_call_id: ToolCallId::new("tool-1"),
            },
        ),
        (
            vec![
                record(1, assistant("assistant-a", "tool-1")),
                record(2, tool_result("result-a", "assistant-a", "tool-1")),
                record(3, tool_result("result-b", "assistant-a", "tool-1")),
            ],
            SessionError::ToolResultAlreadySettled {
                tool_call_id: ToolCallId::new("tool-1"),
            },
        ),
        (
            vec![
                record(1, assistant("assistant-a", "tool-1")),
                record(2, tool_result("result", "assistant-b", "tool-1")),
            ],
            SessionError::SplitToolPair {
                tool_call_id: ToolCallId::new("tool-1"),
                assistant_entry_id: EntryId::new("assistant-b"),
            },
        ),
    ];

    // act
    // assert
    for (records, expected) in cases {
        assert_eq!(
            replay(SessionId::new("session-tools"), &records),
            Err(expected),
            "invalid tool pairing must return its typed error"
        );
    }
}

#[test]
fn canonical_branch_selection_rejects_tool_result_off_selected_path() {
    // arrange
    let records = vec![
        record(1, assistant("assistant-left", "tool-left")),
        record(2, assistant("assistant-right", "tool-right")),
        CanonicalRecord {
            session_id: SessionId::new("session-tools"),
            sequence: RecordSequence::new(3),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new("assistant-right"),
            },
        },
        record(
            4,
            tool_result("left-result", "assistant-left", "tool-left"),
        ),
    ];

    // act
    let result = replay(SessionId::new("session-tools"), &records);

    // assert
    assert_eq!(
        result,
        Err(SessionError::ToolResultOffActivePath {
            tool_call_id: ToolCallId::new("tool-left"),
        })
    );
}

#[test]
fn canonical_branch_selection_revalidates_tool_pairing() {
    // arrange
    let records = vec![
        record(1, assistant("assistant-root", "tool-root")),
        record(
            2,
            tool_result("tool-result", "assistant-root", "tool-root"),
        ),
        record(
            3,
            SessionEntry {
                id: EntryId::new("alternate-leaf"),
                parent_id: Some(EntryId::new("assistant-root")),
                turn_id: Some(TurnId::new("turn-tools")),
                run_id: RunId::new("run-tools"),
                payload: SessionEntryPayload::UserMessage {
                    text: "alternate branch".to_string(),
                    attachments: Vec::new(),
                },
            },
        ),
        CanonicalRecord {
            session_id: SessionId::new("session-tools"),
            sequence: RecordSequence::new(4),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new("alternate-leaf"),
            },
        },
    ];

    // act
    let result = replay(SessionId::new("session-tools"), &records);

    // assert
    assert_eq!(
        result,
        Err(SessionError::ToolResultOffActivePath {
            tool_call_id: ToolCallId::new("tool-root"),
        })
    );
}
