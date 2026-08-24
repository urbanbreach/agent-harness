use super::*;
use harness_core::ids::{RunId, SessionId};
use harness_core::session::legacy::{
    LegacyAdapterError, LegacyEventLogAdapter, LegacyIdentityNamespace, LegacyWarning,
};
use harness_core::session::{AssistantPart, SessionEntryPayload};

#[test]
fn canonical_foreign_identity_is_deterministic_namespaced_and_root_child_isolated() {
    // arrange
    let root_run = RunId::new("shared-raw-id");
    let child_run = RunId::new("child-raw-id");
    let root = LegacyIdentityNamespace::new(&root_run);
    let root_again = LegacyIdentityNamespace::new(&root_run);
    let child = LegacyIdentityNamespace::new(&child_run);

    // act
    let root_session = root.session_id();
    let root_entry = root.entry_id(7, "evt-7", "user_message");
    let root_turn = root.turn_id("request-7");
    let root_request = root.provider_request_id("request-7");

    // assert
    assert_eq!(root_session, root_again.session_id());
    assert_eq!(root_entry, root_again.entry_id(7, "evt-7", "user_message"));
    assert!(root_session.as_str().starts_with("legacy-session-"));
    assert!(root_entry.as_str().starts_with("legacy-entry-"));
    assert!(root_turn.as_str().starts_with("legacy-turn-"));
    assert!(
        root_request
            .as_str()
            .starts_with("legacy-provider-request-")
    );
    assert_ne!(root_session, child.session_id());
    assert_ne!(root_entry, child.entry_id(7, "evt-7", "user_message"));
    assert!(!root_session.as_str().contains(root_run.as_str()));
}

#[test]
fn legacy_adapter_projects_valid_history_without_writing_source() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let marker = root.path().join("source.marker");
    fs::write(&marker, b"unchanged").unwrap_or_abort();
    let before = fs::read(&marker).unwrap_or_abort();
    let events = vec![sample_envelope(
        1,
        "legacy-run",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    )];

    // act
    let result = LegacyEventLogAdapter::new().project(&events);

    // assert
    let Ok(snapshot) = result else {
        assert!(
            result.is_ok(),
            "legacy adapter should project valid history, got {result:?}"
        );
        return;
    };
    assert_eq!(snapshot.session.session_id(), &SessionId::new(snapshot.session.session_id().as_str()));
    assert_eq!(fs::read(&marker).unwrap_or_abort(), before);
    assert_eq!(fs::read_dir(root.path()).unwrap_or_abort().count(), 1);
}

#[test]
fn legacy_adapter_rejects_mixed_run_schema_sequence_duplicate_and_foreign_identity() {
    // arrange
    let base = sample_envelope(
        1,
        "legacy-run",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    );
    let mut wrong_schema = base.clone();
    wrong_schema.schema_version = SCHEMA_VERSION + 1;
    let mut sequence_gap = base.clone();
    sequence_gap.seq = 3;
    sequence_gap.event_id = "evt-3".to_string();
    let mut mixed_run = base.clone();
    mixed_run.seq = 2;
    mixed_run.event_id = "evt-2".to_string();
    mixed_run.run_id = RunId::new("foreign-run");
    let mut foreign_stream = base.clone();
    foreign_stream.stream_key = Some("run:foreign-run".to_string());
    let adapter = LegacyEventLogAdapter::new();

    // act
    // assert
    assert_eq!(
        adapter.project(&[wrong_schema]),
        Err(LegacyAdapterError::UnsupportedSchema {
            expected: SCHEMA_VERSION,
            actual: SCHEMA_VERSION + 1,
        })
    );
    assert_eq!(
        adapter.project(&[base.clone(), sequence_gap]),
        Err(LegacyAdapterError::NonContiguousSequence {
            expected_previous: 1,
            actual: 3,
        })
    );
    assert_eq!(
        adapter.project(&[base.clone(), mixed_run]),
        Err(LegacyAdapterError::MixedRun {
            expected: RunId::new("legacy-run"),
            actual: RunId::new("foreign-run"),
        })
    );
    assert_eq!(
        adapter.project(&[base.clone(), base.clone()]),
        Err(LegacyAdapterError::DuplicateEvent {
            event_id: "evt-1".to_string(),
        })
    );
    assert_eq!(
        adapter.project(&[foreign_stream]),
        Err(LegacyAdapterError::InvalidIdentityRelationship {
            event_id: "evt-1".to_string(),
        })
    );
}

#[test]
fn legacy_adapter_preserves_full_provenance_without_writing_source() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let source_path = root.path().join("events.jsonl");
    let attachment = harness_core::attachment_transport::AttachmentMetadata::from_bytes(
        "diagram",
        "image/png",
        None,
        b"fixture-bytes",
        Some(harness_core::attachment_transport::AttachmentDimensions::new(2, 3)),
    );
    let events = vec![
        sample_envelope(
            1,
            "legacy-run",
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "user-1".into(),
                text: "こんにちは, 世界".to_string(),
            }),
        ),
        sample_envelope(
            2,
            "legacy-run",
            EventV1::PromptAttachmentsSubmitted(
                harness_core::event::PromptAttachmentsSubmittedEvent {
                    request_id: "user-1".into(),
                    attachments: vec![attachment.clone()],
                },
            ),
        ),
        sample_envelope(
            3,
            "legacy-run",
            EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: "provider-1".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-unicode".to_string(),
                    prompt_summary: "redacted prompt".to_string(),
                    request_digest: "digest-prompt".to_string(),
                    metadata: None,
                },
            ),
        ),
        sample_envelope(
            4,
            "legacy-run",
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "provider-1".into(),
                delta: "完了".to_string(),
            }),
        ),
        sample_envelope(
            5,
            "legacy-run",
            EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: "provider-1".into(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-output".to_string()),
                    usage: Some(harness_providers::CompletionUsage {
                        prompt_tokens: 21,
                        completion_tokens: 8,
                        total_tokens: 29,
                    }),
                    metadata: None,
                },
            ),
        ),
        sample_envelope(
            6,
            "legacy-run",
            EventV1::AssistantMessageFinished(
                harness_core::event::AssistantMessageFinishedEvent {
                    request_id: "provider-1".into(),
                    tool_call_count: 0,
                    assistant_message: None,
                },
            ),
        ),
        sample_envelope(
            7,
            "legacy-run",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&source_path, &events);
    let before_bytes = fs::read(&source_path).unwrap_or_abort();
    let before_inventory = fs::read_dir(root.path())
        .unwrap_or_abort()
        .map(|entry| entry.map(|value| value.file_name()))
        .collect::<Result<Vec<_>, _>>();
    assert!(
        before_inventory.is_ok(),
        "source inventory should be readable"
    );
    let Ok(mut before_inventory) = before_inventory else {
        return;
    };
    before_inventory.sort();

    // act
    let result = LegacyEventLogAdapter::new().project(&events);

    // assert
    let Ok(snapshot) = result else {
        assert!(
            result.is_ok(),
            "legacy projection should succeed, got {result:?}"
        );
        return;
    };
    assert_eq!(snapshot.provenance.source_event_count, 7);
    assert_eq!(snapshot.audit_timeline.len(), 7);
    assert!(
        snapshot
            .warnings
            .contains(&LegacyWarning::InferredSessionIdentity)
    );
    assert!(snapshot.warnings.iter().any(|warning| matches!(
        warning,
        LegacyWarning::InferredTurnIdentity {
            correlation_id: None
        }
    )));
    let path = snapshot.session.active_path();
    let Ok(path) = path else {
        assert!(
            path.is_ok(),
            "legacy active path should resolve, got {path:?}"
        );
        return;
    };
    assert!(
        matches!(
            &path[0].payload,
            SessionEntryPayload::UserMessage { .. }
        ),
        "first legacy entry should be a user message"
    );
    let SessionEntryPayload::UserMessage { text, attachments } = &path[0].payload else {
        return;
    };
    assert_eq!(text, "こんにちは, 世界");
    assert_eq!(attachments, &[attachment]);
    assert!(
        matches!(
            &path[1].payload,
            SessionEntryPayload::AssistantMessage { .. }
        ),
        "second legacy entry should be an assistant message"
    );
    let SessionEntryPayload::AssistantMessage { parts, provenance } = &path[1].payload else {
        return;
    };
    assert_eq!(
        parts,
        &[AssistantPart::Text {
            text: "完了".to_string(),
        }]
    );
    assert!(
        provenance.is_some(),
        "legacy assistant should retain sanitized provenance"
    );
    let Some(provenance) = provenance else {
        return;
    };
    assert_eq!(provenance.provider_id, "mock");
    assert_eq!(provenance.model_id, "model-unicode");
    assert_eq!(provenance.stop_reason.as_deref(), Some("stop"));
    assert_eq!(
        provenance.usage,
        Some(harness_providers::CompletionUsage {
            prompt_tokens: 21,
            completion_tokens: 8,
            total_tokens: 29,
        })
    );
    assert_eq!(fs::read(&source_path).unwrap_or_abort(), before_bytes);
    let after_inventory = fs::read_dir(root.path())
        .unwrap_or_abort()
        .map(|entry| entry.map(|value| value.file_name()))
        .collect::<Result<Vec<_>, _>>();
    assert!(
        after_inventory.is_ok(),
        "post-projection inventory should be readable"
    );
    let Ok(mut after_inventory) = after_inventory else {
        return;
    };
    after_inventory.sort();
    assert_eq!(after_inventory, before_inventory);
}

#[test]
fn legacy_adapter_covers_semantic_payload_and_loss_warning_inventory() {
    // arrange
    let correlated = |mut event: EventEnvelopeV1, correlation: &str| {
        event.correlation_id = Some(correlation.to_string());
        event
    };
    let events = vec![
        sample_envelope(
            1,
            "legacy-run",
            EventV1::SessionTitleUpdated(harness_core::event::SessionTitleUpdatedEvent {
                title: "Legacy title".to_string(),
            }),
        ),
        sample_envelope(
            2,
            "legacy-run",
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-1".into(),
                text: "use a tool".to_string(),
            }),
        ),
        correlated(
            sample_envelope(
                3,
                "legacy-run",
                EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "provider-1".into(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "redacted".to_string(),
                        request_digest: "digest-request".to_string(),
                        metadata: None,
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                4,
                "legacy-run",
                EventV1::ProviderReasoningDelta(
                    harness_core::event::ProviderReasoningDeltaEvent {
                        request_id: "provider-1".into(),
                        delta: "reasoning".to_string(),
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                5,
                "legacy-run",
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "provider-1".into(),
                    delta: "answer".to_string(),
                }),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                6,
                "legacy-run",
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "provider-1".into(),
                        finish_reason: "tool".to_string(),
                        output_digest: Some("digest-output".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                7,
                "legacy-run",
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "provider-1".into(),
                        tool_call_count: 1,
                        assistant_message: None,
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                8,
                "legacy-run",
                EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "toolcall-1".into(),
                    tool_id: "bash".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                }),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                9,
                "legacy-run",
                EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
                    tool_call_id: "toolcall-1".into(),
                }),
            ),
            "toolcall-1",
        ),
        correlated(
            sample_envelope(
                10,
                "legacy-run",
                EventV1::ToolCallFinished(harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "toolcall-1".into(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            "toolcall-1",
        ),
        sample_envelope(
            11,
            "legacy-run",
            EventV1::SessionCompaction(harness_core::event::SessionCompactionEvent {
                agent_id: "agent-1".to_string(),
                summary: "compact".to_string(),
                first_kept_event_seq: 2,
                first_kept_request_id: None,
                tokens_before: 2_000,
                read_files: Vec::new(),
                modified_files: Vec::new(),
                trigger_reason: "threshold".to_string(),
                from_hook: false,
            }),
        ),
        sample_envelope(
            12,
            "legacy-run",
            EventV1::SessionCompaction(harness_core::event::SessionCompactionEvent {
                agent_id: "agent-1".to_string(),
                summary: "missing boundary".to_string(),
                first_kept_event_seq: 999,
                first_kept_request_id: None,
                tokens_before: 1_000,
                read_files: Vec::new(),
                modified_files: Vec::new(),
                trigger_reason: "threshold".to_string(),
                from_hook: false,
            }),
        ),
        sample_envelope(
            13,
            "legacy-run",
            EventV1::BranchSummary(harness_core::event::BranchSummaryEvent {
                agent_id: "agent-1".to_string(),
                summary: "branch".to_string(),
                from_event_seq: 2,
                read_files: Vec::new(),
                modified_files: Vec::new(),
                from_hook: false,
            }),
        ),
        sample_envelope(
            14,
            "legacy-run",
            EventV1::PromptAttachmentsSubmitted(
                harness_core::event::PromptAttachmentsSubmittedEvent {
                    request_id: "missing-user".into(),
                    attachments: Vec::new(),
                },
            ),
        ),
        sample_envelope(
            15,
            "legacy-run",
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "provider-2".into(),
                provider_id: "mock".to_string(),
                model_id: "model-2".to_string(),
                prompt_summary: "redacted".to_string(),
                request_digest: "digest-request-2".to_string(),
                metadata: None,
            }),
        ),
        sample_envelope(
            16,
            "legacy-run",
            EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: "provider-2".into(),
                    finish_reason: "stop".to_string(),
                    output_digest: None,
                    usage: None,
                    metadata: None,
                },
            ),
        ),
        sample_envelope(
            17,
            "legacy-run",
            EventV1::AssistantMessageFinished(
                harness_core::event::AssistantMessageFinishedEvent {
                    request_id: "provider-2".into(),
                    tool_call_count: 0,
                    assistant_message: None,
                },
            ),
        ),
        sample_envelope(
            18,
            "legacy-run",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    // act
    let snapshot = LegacyEventLogAdapter::new()
        .project(&events)
        .unwrap_or_abort();
    let payloads = snapshot
        .session
        .entries()
        .values()
        .map(|entry| &entry.payload)
        .collect::<Vec<_>>();

    // assert
    assert!(payloads.iter().any(|payload| {
        matches!(
            payload,
            SessionEntryPayload::SessionMetadata {
                title: Some(title)
            } if title == "Legacy title"
        )
    }));
    assert!(payloads.iter().any(|payload| {
        matches!(
            payload,
            SessionEntryPayload::AssistantMessage { parts, .. }
                if parts.iter().any(|part| matches!(
                    part,
                    AssistantPart::Reasoning { text } if text == "reasoning"
                ))
        )
    }));
    assert!(payloads
        .iter()
        .any(|payload| matches!(payload, SessionEntryPayload::ToolResult { .. })));
    assert!(payloads
        .iter()
        .any(|payload| matches!(payload, SessionEntryPayload::CompactionSummary { .. })));
    assert!(payloads
        .iter()
        .any(|payload| matches!(payload, SessionEntryPayload::BranchSummary { .. })));
    assert!(snapshot
        .warnings
        .contains(&LegacyWarning::InferredSessionIdentity));
    assert!(snapshot.warnings.contains(&LegacyWarning::InferredTurnIdentity {
        correlation_id: None,
    }));
    assert!(snapshot
        .warnings
        .contains(&LegacyWarning::MissingAttachmentAssociation {
            request_id: "missing-user".to_string(),
        }));
    assert!(snapshot
        .warnings
        .contains(&LegacyWarning::MissingFinalAssistantContent {
            request_id: "provider-2".to_string(),
        }));
    assert!(snapshot
        .warnings
        .contains(&LegacyWarning::MissingCompactionBoundary {
            first_kept_event_seq: 999,
        }));
}

#[test]
fn legacy_adapter_accepts_real_tool_call_correlation() {
    // arrange
    let correlated = |mut event: EventEnvelopeV1, correlation: &str| {
        event.correlation_id = Some(correlation.to_string());
        event
    };
    let events = vec![
        sample_envelope(
            1,
            "legacy-run",
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-1".into(),
                text: "use a tool".to_string(),
            }),
        ),
        correlated(
            sample_envelope(
                2,
                "legacy-run",
                EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "provider-1".into(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "redacted".to_string(),
                        request_digest: "digest-request".to_string(),
                        metadata: None,
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                3,
                "legacy-run",
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "provider-1".into(),
                        finish_reason: "tool".to_string(),
                        output_digest: Some("digest-output".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                4,
                "legacy-run",
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "provider-1".into(),
                        tool_call_count: 1,
                        assistant_message: None,
                    },
                ),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                5,
                "legacy-run",
                EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "toolcall-1".into(),
                    tool_id: "bash".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                }),
            ),
            "req-1",
        ),
        correlated(
            sample_envelope(
                6,
                "legacy-run",
                EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
                    tool_call_id: "toolcall-1".into(),
                }),
            ),
            "toolcall-1",
        ),
        correlated(
            sample_envelope(
                7,
                "legacy-run",
                EventV1::ToolCallFinished(harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "toolcall-1".into(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            "toolcall-1",
        ),
        sample_envelope(
            8,
            "legacy-run",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    // act
    let result = LegacyEventLogAdapter::new().project(&events);

    // assert
    assert!(result.is_ok(), "real tool-call correlation should project: {result:?}");
}

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
