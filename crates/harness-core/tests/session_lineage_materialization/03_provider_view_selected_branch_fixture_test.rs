use harness_core::attachment_transport::{AttachmentDimensions, AttachmentMetadata};
use harness_core::ids::{EntryId, ProviderRequestId, RunId, SessionId, ToolCallId, TurnId};
use harness_core::session::{
    AssistantPart, AssistantToolCall, CanonicalRecord, CanonicalRecordKind, ProviderProvenance,
    RecordSequence, RunAttempt, RunStatus, SessionEntry, SessionEntryPayload, ToolResultStatus,
};
use harness_providers::CompletionUsage;
use serde_json::json;

pub fn fixture_records(session_id: &SessionId) -> Vec<CanonicalRecord> {
    let run_id = RunId::new("run-child");
    let attachment = AttachmentMetadata::from_bytes(
        "名-é.png",
        "image/png",
        None,
        b"typed attachment bytes",
        Some(AttachmentDimensions::new(4, 5)),
    );
    let mut records = vec![
        CanonicalRecord {
            session_id: session_id.clone(),
            sequence: RecordSequence::new(1),
            kind: CanonicalRecordKind::RunStarted {
                attempt: RunAttempt {
                    run_id: run_id.clone(),
                    status: RunStatus::Active,
                    legacy_run_id: None,
                },
            },
        },
        commit(
            2,
            user("root-user", None, &run_id, "root prompt", Vec::new()),
        ),
        commit(
            3,
            assistant(
                "root-assistant",
                Some("root-user"),
                &run_id,
                vec![
                    AssistantPart::Text {
                        text: "root answer".to_string(),
                    },
                    tool_call("selected-tool-call"),
                ],
                10,
            ),
        ),
        commit(
            4,
            SessionEntry {
                id: EntryId::new("root-tool-result"),
                parent_id: Some(EntryId::new("root-assistant")),
                turn_id: Some(TurnId::new("turn-root")),
                run_id: run_id.clone(),
                payload: SessionEntryPayload::ToolResult {
                    tool_call_id: ToolCallId::new("selected-tool-call"),
                    requesting_assistant_entry_id: EntryId::new("root-assistant"),
                    status: ToolResultStatus::Succeeded,
                    output_summary: Some("selected result".to_string()),
                    output_digest: Some("selected-result-digest".to_string()),
                    output_json: Some(json!({"selected": true})),
                },
            },
        ),
        commit(
            5,
            user(
                "selected-user",
                Some("root-tool-result"),
                &run_id,
                "selected prompt 日本語 😀",
                vec![attachment],
            ),
        ),
        commit(
            6,
            assistant(
                "selected-assistant",
                Some("selected-user"),
                &run_id,
                vec![
                    AssistantPart::Reasoning {
                        text: "selected reasoning".to_string(),
                    },
                    AssistantPart::Text {
                        text: "selected answer".to_string(),
                    },
                ],
                118,
            ),
        ),
        commit(
            7,
            user(
                "source-tail-user",
                Some("selected-assistant"),
                &run_id,
                "abandoned source tail",
                Vec::new(),
            ),
        ),
        commit(
            8,
            assistant(
                "source-tail-assistant",
                Some("source-tail-user"),
                &run_id,
                vec![AssistantPart::Text {
                    text: "abandoned source answer".to_string(),
                }],
                999,
            ),
        ),
        commit(
            9,
            SessionEntry {
                id: EntryId::new("legacy-delta"),
                parent_id: Some(EntryId::new("source-tail-assistant")),
                turn_id: Some(TurnId::new("turn-legacy")),
                run_id: run_id.clone(),
                payload: SessionEntryPayload::CustomModelVisibleContext {
                    key: "legacy-delta".to_string(),
                    context: "legacy delta and live delta must not resume".to_string(),
                },
            },
        ),
        commit(
            10,
            user(
                "off-path-user",
                Some("root-user"),
                &run_id,
                "off path user",
                Vec::new(),
            ),
        ),
        commit(
            11,
            assistant(
                "off-path-interrupted",
                Some("off-path-user"),
                &run_id,
                vec![
                    AssistantPart::Text {
                        text: "interrupted off path".to_string(),
                    },
                    tool_call("off-path-tool-call"),
                ],
                999,
            ),
        ),
        CanonicalRecord {
            session_id: session_id.clone(),
            sequence: RecordSequence::new(12),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new("selected-assistant"),
            },
        },
        commit(
            13,
            user(
                "sibling-tail",
                Some("root-user"),
                &run_id,
                "abandoned sibling tail",
                Vec::new(),
            ),
        ),
    ];
    for record in &mut records {
        record.session_id = session_id.clone();
    }
    records
}

fn commit(sequence: u64, entry: SessionEntry) -> CanonicalRecord {
    CanonicalRecord {
        session_id: SessionId::new("placeholder"),
        sequence: RecordSequence::new(sequence),
        kind: CanonicalRecordKind::EntryCommitted { entry },
    }
}

fn user(
    id: &str,
    parent_id: Option<&str>,
    run_id: &RunId,
    text: &str,
    attachments: Vec<AttachmentMetadata>,
) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: parent_id.map(EntryId::new),
        turn_id: Some(TurnId::new(id)),
        run_id: run_id.clone(),
        payload: SessionEntryPayload::UserMessage {
            text: text.to_string(),
            attachments,
        },
    }
}

fn assistant(
    id: &str,
    parent_id: Option<&str>,
    run_id: &RunId,
    parts: Vec<AssistantPart>,
    total_tokens: u32,
) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: parent_id.map(EntryId::new),
        turn_id: Some(TurnId::new(id)),
        run_id: run_id.clone(),
        payload: SessionEntryPayload::AssistantMessage {
            parts,
            provenance: Some(Box::new(ProviderProvenance {
                provider_id: "mock".to_string(),
                model_id: "selected-model".to_string(),
                request_id: ProviderRequestId::new(format!("request-{id}")),
                response_id: Some(format!("response-{id}")),
                stop_reason: Some("stop".to_string()),
                usage: Some(CompletionUsage {
                    prompt_tokens: total_tokens.saturating_sub(10),
                    completion_tokens: 10,
                    total_tokens,
                }),
                runtime_selection: None,
            })),
        },
    }
}

fn tool_call(id: &str) -> AssistantPart {
    AssistantPart::ToolCall(AssistantToolCall {
        tool_call_id: ToolCallId::new(id),
        provider_tool_call_id: Some(format!("provider-{id}")),
        tool_id: "projection_probe".to_string(),
        args_summary: "{\"selected\":true}".to_string(),
        args_digest: format!("args-{id}"),
        provider_call_id: Some(format!("call-{id}")),
    })
}
