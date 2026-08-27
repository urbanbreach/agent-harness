use harness_core::attachment_transport::AttachmentMetadata;
use harness_core::ids::{EntryId, ProviderRequestId, RunId, SessionId, ToolCallId, TurnId};
use harness_core::session::reducer::replay as replay_session;
use harness_core::session::{
    AssistantPart, AssistantToolCall, CanonicalRecord, CanonicalRecordKind, CanonicalSession,
    ProviderProvenance, RecordSequence, RunAttempt, RunStatus, SessionEntry, SessionEntryPayload,
    ToolResultStatus,
};
use harness_core::UnwrapOrAbort;
use harness_providers::CompletionUsage;

fn record(sequence: u64, entry: SessionEntry) -> CanonicalRecord {
    CanonicalRecord {
        session_id: SessionId::new("session-resume-plan"),
        sequence: RecordSequence::new(sequence),
        kind: CanonicalRecordKind::EntryCommitted { entry },
    }
}

fn replay(records: Vec<CanonicalRecord>) -> CanonicalSession {
    let session_id = SessionId::new("session-resume-plan");
    let mut journal = vec![CanonicalRecord {
        session_id: session_id.clone(),
        sequence: RecordSequence::new(1),
        kind: CanonicalRecordKind::RunStarted {
            attempt: RunAttempt {
                run_id: RunId::new("run-resume-plan"),
                status: RunStatus::Active,
                legacy_run_id: None,
            },
        },
    }];
    journal.extend(records.into_iter().map(|mut record| {
        record.sequence = RecordSequence::new(record.sequence.get() + 1);
        record
    }));
    replay_session(session_id, &journal).unwrap_or_abort()
}

fn user_entry(id: &str, parent_id: Option<&str>, attachments: Vec<AttachmentMetadata>) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: parent_id.map(EntryId::new),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-resume-plan"),
        payload: SessionEntryPayload::UserMessage {
            text: id.to_string(),
            attachments,
        },
    }
}

fn tool_assistant(id: &str, parent_id: &str, tool_call_id: &str) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: Some(EntryId::new(parent_id)),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-resume-plan"),
        payload: SessionEntryPayload::AssistantMessage {
            parts: vec![AssistantPart::ToolCall(AssistantToolCall {
                tool_call_id: ToolCallId::new(tool_call_id),
                provider_tool_call_id: Some("provider-tool-durable".to_string()),
                tool_id: "shell.run".to_string(),
                args_summary: "{\"command\":\"printf durable\"}".to_string(),
                args_digest: "digest-durable-args".to_string(),
                provider_call_id: Some("provider-call-durable".to_string()),
            })],
            provenance: None,
        },
    }
}

fn completed_tool_result(id: &str, parent_id: &str, tool_call_id: &str) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: Some(EntryId::new(parent_id)),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-resume-plan"),
        payload: SessionEntryPayload::ToolResult {
            tool_call_id: ToolCallId::new(tool_call_id),
            requesting_assistant_entry_id: EntryId::new(parent_id),
            status: ToolResultStatus::Succeeded,
            output_summary: Some("durable tool output".to_string()),
            output_digest: Some("digest-durable-output".to_string()),
            output_json: Some(serde_json::json!({"status":"ok"})),
        },
    }
}

fn final_assistant(id: &str, parent_id: &str) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: Some(EntryId::new(parent_id)),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-resume-plan"),
        payload: SessionEntryPayload::AssistantMessage {
            parts: vec![AssistantPart::Text {
                text: "durable continuation".to_string(),
            }],
            provenance: Some(Box::new(ProviderProvenance {
                provider_id: "mock".to_string(),
                model_id: "model-resume-plan".to_string(),
                request_id: ProviderRequestId::new("provider-durable"),
                response_id: Some("response-durable".to_string()),
                stop_reason: Some("stop".to_string()),
                usage: Some(CompletionUsage {
                    prompt_tokens: 31,
                    completion_tokens: 7,
                    total_tokens: 38,
                }),
                runtime_selection: None,
            })),
        },
    }
}

fn interrupted_tool_assistant(id: &str, parent_id: &str) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: Some(EntryId::new(parent_id)),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-resume-plan"),
        payload: SessionEntryPayload::AssistantMessage {
            parts: vec![AssistantPart::ToolCall(AssistantToolCall {
                tool_call_id: ToolCallId::new("tool-off-path-incomplete"),
                provider_tool_call_id: None,
                tool_id: "shell.run".to_string(),
                args_summary: "{\"command\":\"interrupted\"}".to_string(),
                args_digest: "digest-off-path".to_string(),
                provider_call_id: None,
            })],
            provenance: None,
        },
    }
}

#[test]
fn resume_plan_preserves_selected_active_leaf_boundaries() {
    // arrange
    // act
    // assert
    // Given: sibling leaves with one selected completed path and one interrupted off-path turn.
    let attachment = AttachmentMetadata::from_bytes(
        "durable-attachment",
        "image/png",
        None,
        b"typed durable bytes",
        None,
    );
    let records = vec![
        record(1, user_entry("root", None, Vec::new())),
        record(2, user_entry("durable-user", Some("root"), vec![attachment.clone()])),
        record(3, tool_assistant("durable-assistant", "durable-user", "tool-durable")),
        record(
            4,
            completed_tool_result("durable-result", "durable-assistant", "tool-durable"),
        ),
        record(5, final_assistant("durable-final", "durable-result")),
        record(6, user_entry("off-path-user", Some("root"), Vec::new())),
        record(
            7,
            interrupted_tool_assistant("off-path-interrupted", "off-path-user"),
        ),
        CanonicalRecord {
            session_id: SessionId::new("session-resume-plan"),
            sequence: RecordSequence::new(8),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new("durable-final"),
            },
        },
    ];

    // When: the restart plan is reconstructed from the canonical journal.
    let session = replay(records);
    let path = session.active_path().unwrap_or_abort();
    let path_ids = path
        .iter()
        .map(|entry| entry.id.as_str().to_string())
        .collect::<Vec<_>>();

    // Then: only the persisted durable leaf and its complete boundaries are selected.
    assert_eq!(session.active_leaf(), Some(&EntryId::new("durable-final")));
    assert_eq!(session.watermark(), Some(RecordSequence::new(9)));
    assert_eq!(
        path_ids,
        vec![
            "root",
            "durable-user",
            "durable-assistant",
            "durable-result",
            "durable-final",
        ]
    );
    assert!(!path.iter().any(|entry| {
        matches!(
            entry.payload,
            SessionEntryPayload::AssistantMessage { ref parts, .. }
                if parts.iter().any(|part| matches!(
                    part,
                    AssistantPart::ToolCall(call)
                        if call.tool_call_id.as_str() == "tool-off-path-incomplete"
                ))
        )
    }));
    let SessionEntryPayload::UserMessage { attachments, .. } = &path[1].payload else {
        panic!("selected path must retain the durable user attachment");
    };
    assert_eq!(attachments, &[attachment]);
    let SessionEntryPayload::AssistantMessage { parts, .. } = &path[2].payload else {
        panic!("selected path must retain the completed tool call");
    };
    let Some(AssistantPart::ToolCall(tool_call)) = parts.first() else {
        panic!("selected assistant must contain a tool call");
    };
    let SessionEntryPayload::ToolResult {
        tool_call_id,
        requesting_assistant_entry_id,
        status,
        ..
    } = &path[3].payload
    else {
        panic!("selected path must retain the tool result");
    };
    assert_eq!(tool_call.tool_call_id.as_str(), tool_call_id.as_str());
    assert_eq!(requesting_assistant_entry_id.as_str(), "durable-assistant");
    assert_eq!(*status, ToolResultStatus::Succeeded);
    let SessionEntryPayload::AssistantMessage { provenance, .. } = &path[4].payload else {
        panic!("selected path must retain the final assistant provenance");
    };
    assert_eq!(
        provenance.as_ref().and_then(|value| value.usage.as_ref()),
        Some(&CompletionUsage {
            prompt_tokens: 31,
            completion_tokens: 7,
            total_tokens: 38,
        })
    );
    assert!(session.entries().contains_key(&EntryId::new("off-path-interrupted")));
    eprintln!(
        "G007_TASK4 resume_plan selected_leaf=durable-final selected_path=[root,durable-user,durable-assistant,durable-result,durable-final] attachment=durable-attachment usage=31+7=38 tool_pair=tool-durable:durable-result off_path_incomplete=tool-off-path-incomplete excluded=true"
    );
}
