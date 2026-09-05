use harness_core::attachment_transport::AttachmentMetadata;
use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits};
use harness_core::event::ProviderRequestStartedMetadata;
use harness_core::ids::{EntryId, ProviderRequestId, RunId, SessionId, ToolCallId, TurnId};
use harness_core::session::reducer::replay;
use harness_core::session::{
    AssistantPart, AssistantToolCall, CanonicalPendingPrompt, CanonicalRecord,
    CanonicalRecordKind, CanonicalRuntimeSelection, CanonicalSession, ProviderProvenance,
    ProviderViewError, ProviderViewInput, ProviderViewOwner, RecordSequence, RunAttempt, RunStatus,
    SessionEntry, SessionEntryPayload, ToolResultStatus, UsageBoundaryKind,
};
use harness_core::UnwrapOrAbort;
use harness_providers::CompletionUsage;

fn runtime_selection() -> CanonicalRuntimeSelection {
    CanonicalRuntimeSelection::new(
        Some("reasoning-profile".to_string()),
        "mock",
        "model-a",
        AgentModelSettings {
            variant: Some("high".to_string()),
            reasoning_effort: Some("high".to_string()),
            text_verbosity: Some("low".to_string()),
            reasoning_summary: Some("auto".to_string()),
            thinking: Some(serde_json::json!({"budget_tokens": 4096})),
        },
        ResolvedModelLimits::from_values(
            Some(128_000),
            Some(120_000),
            Some(8_000),
            ModelLimitProvenance::explicit("selected profile"),
        ),
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .unwrap_or_abort()
}

fn entry(id: &str, parent: Option<&str>, payload: SessionEntryPayload) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: parent.map(EntryId::new),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-view"),
        payload,
    }
}

fn session(entries: Vec<SessionEntry>, active_leaf: &str) -> CanonicalSession {
    let session_id = SessionId::new("session-view");
    let mut records = vec![CanonicalRecord {
        session_id: session_id.clone(),
        sequence: RecordSequence::new(1),
        kind: CanonicalRecordKind::RunStarted {
            attempt: RunAttempt {
                run_id: RunId::new("run-view"),
                status: RunStatus::Active,
                legacy_run_id: None,
            },
        },
    }];
    records.extend(entries.into_iter().enumerate().map(|(index, entry)| {
        CanonicalRecord {
            session_id: session_id.clone(),
            sequence: RecordSequence::new(index as u64 + 2),
            kind: CanonicalRecordKind::EntryCommitted { entry },
        }
    }));
    records.push(CanonicalRecord {
        session_id: session_id.clone(),
        sequence: RecordSequence::new(records.len() as u64 + 1),
        kind: CanonicalRecordKind::ActiveLeafSelected {
            entry_id: EntryId::new(active_leaf),
        },
    });
    replay(session_id, &records).unwrap_or_abort()
}

#[test]
fn canonical_provider_view_preserves_selected_protocol_state_and_runtime_selection() {
    let attachment = AttachmentMetadata::from_bytes(
        "資料",
        "image/png",
        None,
        b"redacted-fixture",
        None,
    );
    let usage = CompletionUsage {
        prompt_tokens: 17,
        completion_tokens: 5,
        total_tokens: 22,
    };
    let entries = vec![
        entry(
            "user",
            None,
            SessionEntryPayload::UserMessage {
                text: "selected".to_string(),
                attachments: vec![attachment.clone()],
            },
        ),
        entry(
            "assistant",
            Some("user"),
            SessionEntryPayload::AssistantMessage {
                parts: vec![
                    AssistantPart::ToolCall(AssistantToolCall {
                        tool_call_id: ToolCallId::new("call-b"),
                        provider_tool_call_id: None,
                        tool_id: "read".to_string(),
                        args_summary: "redacted".to_string(),
                        args_digest: "args-digest".to_string(),
                        provider_call_id: None,
                    }),
                    AssistantPart::ToolCall(AssistantToolCall {
                        tool_call_id: ToolCallId::new("call-a-orphan"),
                        provider_tool_call_id: None,
                        tool_id: "grep".to_string(),
                        args_summary: "redacted".to_string(),
                        args_digest: "orphan-digest".to_string(),
                        provider_call_id: None,
                    }),
                ],
                provenance: Some(Box::new(ProviderProvenance {
                    provider_id: "mock".to_string(),
                    model_id: "model-a".to_string(),
                    request_id: ProviderRequestId::new("provider-view"),
                    response_id: None,
                    stop_reason: Some("tool_use".to_string()),
                    usage: Some(usage.clone()),
                    runtime_selection: Some(Box::new(runtime_selection())),
                })),
            },
        ),
        entry(
            "result",
            Some("assistant"),
            SessionEntryPayload::ToolResult {
                tool_call_id: ToolCallId::new("call-b"),
                requesting_assistant_entry_id: EntryId::new("assistant"),
                status: ToolResultStatus::Succeeded,
                output_summary: Some("done".to_string()),
                output_digest: Some("output-digest".to_string()),
                output_json: None,
            },
        ),
        entry(
            "summary-old",
            Some("result"),
            SessionEntryPayload::CompactionSummary {
                summary: "old".to_string(),
                first_kept_entry_id: EntryId::new("user"),
                tokens_after: Some(30),
                summary_usage: Some(usage.clone()),
                summary_provider_id: Some("mock".to_string()),
                summary_model_id: Some("model-a".to_string()),
                preserved_state: None,
            },
        ),
        entry(
            "summary-latest",
            Some("summary-old"),
            SessionEntryPayload::CompactionSummary {
                summary: "latest".to_string(),
                first_kept_entry_id: EntryId::new("result"),
                tokens_after: Some(18),
                summary_usage: Some(usage.clone()),
                summary_provider_id: Some("mock".to_string()),
                summary_model_id: Some("model-a".to_string()),
                preserved_state: None,
            },
        ),
        entry(
            "leaf",
            Some("summary-latest"),
            SessionEntryPayload::UserMessage {
                text: "continue".to_string(),
                attachments: Vec::new(),
            },
        ),
        entry(
            "off-path",
            Some("user"),
            SessionEntryPayload::UserMessage {
                text: "excluded".to_string(),
                attachments: Vec::new(),
            },
        ),
    ];
    let session = session(entries, "leaf");
    let selection = runtime_selection();

    let view = session
        .provider_view(ProviderViewInput {
            owner: ProviderViewOwner::root("root-agent", SessionId::new("session-view")),
            selected_leaf: None,
            pending_prompt: Some(CanonicalPendingPrompt {
                turn_id: TurnId::new("turn-pending"),
                text: "pending".to_string(),
                attachments: Vec::new(),
            }),
            runtime_selection: selection.clone(),
        })
        .unwrap_or_abort();
    let visible_calls = view
        .entries
        .iter()
        .flat_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => Some(call.tool_call_id.clone()),
                    AssistantPart::Text { .. } | AssistantPart::Reasoning { .. } => None,
                })
                .collect::<Vec<_>>(),
            SessionEntryPayload::UserMessage { .. }
            | SessionEntryPayload::ToolResult { .. }
            | SessionEntryPayload::ModelChange { .. }
            | SessionEntryPayload::ReasoningSettingChange { .. }
            | SessionEntryPayload::SystemContextUpdate { .. }
            | SessionEntryPayload::CompactionSummary { .. }
            | SessionEntryPayload::BranchSummary { .. }
            | SessionEntryPayload::CustomPersistedState { .. }
            | SessionEntryPayload::CustomModelVisibleContext { .. }
            | SessionEntryPayload::SessionMetadata { .. } => Vec::new(),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (
            view.active_entry_ids,
            view.entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
            view.tool_pairs,
            visible_calls,
            view.attachments,
            view.usage_boundaries
                .iter()
                .map(|boundary| (&boundary.entry_id, boundary.kind))
                .collect::<Vec<_>>(),
            view.latest_compaction_summary.map(|summary| summary.summary),
            view.watermark,
            view.runtime_selection,
        ),
        (
            vec!["user", "assistant", "result", "summary-old", "summary-latest", "leaf"]
                .into_iter()
                .map(EntryId::new)
                .collect(),
            vec!["user", "assistant", "result", "leaf"],
            vec![harness_core::session::CanonicalToolPair {
                tool_call_id: ToolCallId::new("call-b"),
                assistant_entry_id: EntryId::new("assistant"),
                result_entry_id: EntryId::new("result"),
            }],
            vec![ToolCallId::new("call-b")],
            vec![harness_core::session::CanonicalAttachment {
                entry_id: EntryId::new("user"),
                attachment,
            }],
            vec![
                (&EntryId::new("assistant"), UsageBoundaryKind::Provider),
                (&EntryId::new("summary-old"), UsageBoundaryKind::Compaction),
                (&EntryId::new("summary-latest"), UsageBoundaryKind::Compaction),
            ],
            Some("latest".to_string()),
            session.watermark(),
            selection,
        )
    );
}
#[test]
fn canonical_provider_view_rejects_owner_mismatch_or_malformed_active_path() {
    let valid = session(
        vec![entry(
            "leaf",
            None,
            SessionEntryPayload::UserMessage {
                text: "selected".to_string(),
                attachments: Vec::new(),
            },
        )],
        "leaf",
    );
    let mismatch = valid.provider_view(ProviderViewInput {
        owner: ProviderViewOwner::child(
            "child-agent",
            SessionId::new("other-session"),
            SessionId::new("session-view"),
        ),
        selected_leaf: None,
        pending_prompt: None,
        runtime_selection: runtime_selection(),
    });
    let mut malformed_json = serde_json::to_value(&valid).unwrap_or_abort();
    malformed_json["active_leaf"] = serde_json::json!("missing");
    let malformed: CanonicalSession = serde_json::from_value(malformed_json).unwrap_or_abort();
    let malformed_result = malformed.provider_view(ProviderViewInput {
        owner: ProviderViewOwner::root("root-agent", SessionId::new("session-view")),
        selected_leaf: None,
        pending_prompt: None,
        runtime_selection: runtime_selection(),
    });
    let missing_leaf = CanonicalSession::empty(SessionId::new("session-empty"));
    let missing_leaf_result = missing_leaf.provider_view(ProviderViewInput {
        owner: ProviderViewOwner::root("root-agent", SessionId::new("session-empty")),
        selected_leaf: None,
        pending_prompt: None,
        runtime_selection: runtime_selection(),
    });

    assert_eq!(
        mismatch,
        Err(ProviderViewError::OwnerSessionMismatch {
            expected: SessionId::new("session-view"),
            actual: SessionId::new("other-session"),
        })
    );
    assert_eq!(
        malformed_result,
        Err(ProviderViewError::InvalidSession(
            harness_core::session::SessionError::ActiveLeafMissing {
                entry_id: EntryId::new("missing"),
            }
        ))
    );
    assert_eq!(missing_leaf_result, Err(ProviderViewError::MissingActiveLeaf));
}

#[test]
fn canonical_provider_view_preserves_typed_child_owner_identity() {
    let child = session(
        vec![entry(
            "child-leaf",
            None,
            SessionEntryPayload::UserMessage {
                text: "child".to_string(),
                attachments: Vec::new(),
            },
        )],
        "child-leaf",
    );
    let owner = ProviderViewOwner::child(
        "child-agent",
        SessionId::new("session-view"),
        SessionId::new("root-session"),
    );

    let view = child
        .provider_view(ProviderViewInput {
            owner: owner.clone(),
            selected_leaf: Some(EntryId::new("child-leaf")),
            pending_prompt: None,
            runtime_selection: runtime_selection(),
        })
        .unwrap_or_abort();

    assert_eq!((view.owner, view.active_entry_ids), (owner, vec![EntryId::new("child-leaf")]));
}

#[test]
fn canonical_provider_view_runtime_selection_old_and_new_shapes_round_trip() {
    let old_metadata: ProviderRequestStartedMetadata =
        serde_json::from_value(serde_json::json!({"turn_id":"turn-old"})).unwrap_or_abort();
    assert_eq!(old_metadata.runtime_selection, None);

    let selection = runtime_selection();
    let metadata = ProviderRequestStartedMetadata {
        turn_id: Some("turn-new".to_string()),
        runtime_selection: Some(Box::new(selection.clone())),
        ..ProviderRequestStartedMetadata::default()
    };
    let encoded = serde_json::to_value(&metadata).unwrap_or_abort();
    let restored: ProviderRequestStartedMetadata = serde_json::from_value(encoded).unwrap_or_abort();
    assert_eq!(restored.runtime_selection, Some(Box::new(selection)));

    let old_provenance: ProviderProvenance = serde_json::from_value(serde_json::json!({
        "provider_id":"mock",
        "model_id":"model-old",
        "request_id":"request-old",
        "response_id":null,
        "stop_reason":"stop"
    }))
    .unwrap_or_abort();
    assert_eq!(old_provenance.runtime_selection, None);
    let new_provenance = ProviderProvenance {
        provider_id: "mock".to_string(),
        model_id: "model-a".to_string(),
        request_id: ProviderRequestId::new("request-new"),
        response_id: None,
        stop_reason: Some("stop".to_string()),
        usage: None,
        runtime_selection: Some(Box::new(runtime_selection())),
    };
    let encoded = serde_json::to_value(&new_provenance).unwrap_or_abort();
    let serialized = serde_json::to_string(&encoded).unwrap_or_abort();
    assert!(!serialized.contains("system_prompt"));
    assert!(!serialized.contains("tool_schema"));
    assert!(!serialized.contains("secret"));
    let restored: ProviderProvenance = serde_json::from_value(encoded).unwrap_or_abort();
    assert_eq!(restored, new_provenance);
}

#[test]
fn canonical_provider_view_legacy_adapter_maps_runtime_selection_into_provenance() {
    let selection = runtime_selection();
    let events = vec![
        envelope(
            1,
            worker(),
            None,
            EventV1::RunStarted(harness_core::event::RunStartedEvent {
                run_name: "provider-view".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventActor::new(ActorKind::User, None),
            Some("turn-view"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-view".into(),
                text: "continue".to_string(),
            }),
        ),
        envelope(
            3,
            worker(),
            Some("turn-view"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider-view".into(),
                provider_id: "mock".to_string(),
                model_id: "model-a".to_string(),
                prompt_summary: "redacted".to_string(),
                request_digest: "request-digest".to_string(),
                metadata: Some(ProviderRequestStartedMetadata {
                    turn_id: Some("turn-view".to_string()),
                    runtime_selection: Some(Box::new(selection.clone())),
                    ..ProviderRequestStartedMetadata::default()
                }),
            }),
        ),
        envelope(
            4,
            worker(),
            Some("turn-view"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider-view".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("output-digest".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            5,
            worker(),
            Some("turn-view"),
            EventV1::AssistantMessageFinished(
                harness_core::event::AssistantMessageFinishedEvent {
                    request_id: "provider-view".into(),
                    tool_call_count: 0,
                    parts: vec![AssistantPart::Text {
                        text: "done".to_string(),
                    }],
                    provenance: None,
                    assistant_message: None,
                },
            ),
        ),
    ];

    let projection = harness_core::session::CanonicalSessionProjection::from_event_history(&events)
        .unwrap_or_abort();
    let persisted = projection.session.entries().values().find_map(|entry| {
        let SessionEntryPayload::AssistantMessage { provenance, .. } = &entry.payload else {
            return None;
        };
        provenance
            .as_ref()
            .and_then(|value| value.runtime_selection.as_deref())
    });

    assert_eq!(persisted, Some(&selection));
}
