use harness_core::attachment_transport::{AttachmentDimensions, AttachmentMetadata};
use harness_core::ids::{EntryId, ProviderRequestId, RunId, SessionId, TurnId};
use harness_core::session::reducer::replay as replay_session;
use harness_core::session::{
    AssistantPart, CanonicalRecord, CanonicalRecordKind, CanonicalSession, ProviderProvenance,
    RecordSequence, RunAttempt, RunStatus, SessionEntry, SessionEntryPayload, SessionError,
    SessionStatus,
};
use harness_providers::CompletionUsage;

fn user_entry(id: &str, parent_id: Option<&str>) -> SessionEntry {
    SessionEntry {
        id: EntryId::new(id),
        parent_id: parent_id.map(EntryId::new),
        turn_id: Some(TurnId::new(format!("turn-{id}"))),
        run_id: RunId::new("run-root"),
        payload: SessionEntryPayload::UserMessage {
            text: id.to_string(),
            attachments: Vec::new(),
        },
    }
}

fn commit(sequence: u64, entry: SessionEntry) -> CanonicalRecord {
    CanonicalRecord {
        session_id: SessionId::new("session-root"),
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
                run_id: RunId::new("run-root"),
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

#[test]
fn canonical_active_path_reconstructs_selected_sibling_leaf() {
    // arrange
    let records = vec![
        commit(1, user_entry("root", None)),
        commit(2, user_entry("left", Some("root"))),
        commit(3, user_entry("right", Some("root"))),
        CanonicalRecord {
            session_id: SessionId::new("session-root"),
            sequence: RecordSequence::new(4),
            kind: CanonicalRecordKind::ActiveLeafSelected {
                entry_id: EntryId::new("left"),
            },
        },
    ];

    // act
    let result = replay(SessionId::new("session-root"), &records);

    // assert
    let Ok(session) = result else {
        assert!(
            result.is_ok(),
            "canonical active-path reconstruction should succeed, got {result:?}"
        );
        return;
    };
    let path = session.active_path();
    let Ok(path) = path else {
        assert!(
            path.is_ok(),
            "selected active path should be available, got {path:?}"
        );
        return;
    };
    assert_eq!(
        path.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
        vec!["root", "left"]
    );
    assert!(session.entries().contains_key(&EntryId::new("right")));
}

#[test]
fn canonical_session_rejects_missing_duplicate_and_cyclic_parents() {
    // arrange
    let cases = [
        (
            vec![commit(1, user_entry("orphan", Some("missing")))],
            SessionError::MissingParent {
                entry_id: EntryId::new("orphan"),
                parent_id: EntryId::new("missing"),
            },
        ),
        (
            vec![
                commit(1, user_entry("duplicate", None)),
                commit(2, user_entry("duplicate", None)),
            ],
            SessionError::DuplicateEntry {
                entry_id: EntryId::new("duplicate"),
            },
        ),
        (
            vec![commit(1, user_entry("cycle", Some("cycle")))],
            SessionError::ParentCycle {
                entry_id: EntryId::new("cycle"),
            },
        ),
    ];

    // act
    // assert
    for (records, expected) in cases {
        assert_eq!(
            replay(SessionId::new("session-root"), &records),
            Err(expected),
            "malformed parent relationship must return its typed error"
        );
    }
}

#[test]
fn canonical_session_rejects_mutation_after_terminal_status() {
    // arrange
    let records = vec![
        CanonicalRecord {
            session_id: SessionId::new("session-root"),
            sequence: RecordSequence::new(1),
            kind: CanonicalRecordKind::SessionStatusChanged {
                status: SessionStatus::Completed,
            },
        },
        commit(2, user_entry("late", None)),
    ];

    // act
    let result = replay(SessionId::new("session-root"), &records);

    // assert
    assert_eq!(
        result,
        Err(SessionError::TerminalSessionMutation {
            session_id: SessionId::new("session-root"),
        })
    );
}

#[test]
fn canonical_session_restart_preserves_unicode_attachments_usage_and_provenance() {
    // arrange
    let attachment = AttachmentMetadata::from_bytes(
        "diagram",
        "image/png",
        None,
        b"fixture-bytes",
        Some(AttachmentDimensions::new(2, 3)),
    );
    let records = vec![
        commit(
            1,
            SessionEntry {
                id: EntryId::new("user-unicode"),
                parent_id: None,
                turn_id: Some(TurnId::new("turn-unicode")),
                run_id: RunId::new("run-root"),
                payload: SessionEntryPayload::UserMessage {
                    text: "こんにちは, 世界".to_string(),
                    attachments: vec![attachment.clone()],
                },
            },
        ),
        commit(
            2,
            SessionEntry {
                id: EntryId::new("assistant-unicode"),
                parent_id: Some(EntryId::new("user-unicode")),
                turn_id: Some(TurnId::new("turn-unicode")),
                run_id: RunId::new("run-root"),
                payload: SessionEntryPayload::AssistantMessage {
                    parts: vec![
                        AssistantPart::Reasoning {
                            text: "理由".to_string(),
                        },
                        AssistantPart::Text {
                            text: "完了".to_string(),
                        },
                    ],
                    provenance: Some(Box::new(ProviderProvenance {
                        provider_id: "mock".to_string(),
                        model_id: "model-unicode".to_string(),
                        request_id: ProviderRequestId::new("provider-request-unicode"),
                        response_id: Some("response-unicode".to_string()),
                        stop_reason: Some("stop".to_string()),
                        usage: Some(CompletionUsage {
                            prompt_tokens: 21,
                            completion_tokens: 8,
                            total_tokens: 29,
                        }),
                        runtime_selection: None,
                    })),
                },
            },
        ),
    ];
    let live = replay(SessionId::new("session-root"), &records);
    let Ok(live) = live else {
        assert!(
            live.is_ok(),
            "live canonical replay should succeed, got {live:?}"
        );
        return;
    };

    // act
    let encoded = serde_json::to_vec(&records);
    assert!(
        encoded.is_ok(),
        "canonical records should serialize, got {encoded:?}"
    );
    let Ok(encoded) = encoded else {
        return;
    };
    let restored = serde_json::from_slice::<Vec<CanonicalRecord>>(&encoded);
    assert!(
        restored.is_ok(),
        "canonical records should deserialize, got {restored:?}"
    );
    let Ok(restored) = restored else {
        return;
    };
    let restarted = replay(SessionId::new("session-root"), &restored);
    let Ok(restarted) = restarted else {
        assert!(
            restarted.is_ok(),
            "restarted canonical replay should succeed, got {restarted:?}"
        );
        return;
    };

    // assert
    assert_eq!(restarted, live);
    let path = restarted.active_path();
    let Ok(path) = path else {
        assert!(
            path.is_ok(),
            "restarted active path should resolve, got {path:?}"
        );
        return;
    };
    assert!(
        matches!(
            &path[0].payload,
            SessionEntryPayload::UserMessage { .. }
        ),
        "first active-path entry should be a user message"
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
        "second active-path entry should be an assistant message"
    );
    let SessionEntryPayload::AssistantMessage { parts, provenance } = &path[1].payload else {
        return;
    };
    assert_eq!(
        parts,
        &[
            AssistantPart::Reasoning {
                text: "理由".to_string(),
            },
            AssistantPart::Text {
                text: "完了".to_string(),
            },
        ]
    );
    assert_eq!(
        provenance.as_ref().and_then(|value| value.usage.as_ref()),
        Some(&CompletionUsage {
            prompt_tokens: 21,
            completion_tokens: 8,
            total_tokens: 29,
        })
    );
}

#[test]
fn canonical_session_rejects_entries_for_unknown_and_terminal_runs() {
    // arrange
    let unknown = vec![commit(1, user_entry("unknown-run-entry", None))];
    let terminal = vec![
        CanonicalRecord {
            session_id: SessionId::new("session-root"),
            sequence: RecordSequence::new(1),
            kind: CanonicalRecordKind::RunStarted {
                attempt: RunAttempt {
                    run_id: RunId::new("run-root"),
                    status: RunStatus::Active,
                    legacy_run_id: None,
                },
            },
        },
        CanonicalRecord {
            session_id: SessionId::new("session-root"),
            sequence: RecordSequence::new(2),
            kind: CanonicalRecordKind::RunStatusChanged {
                run_id: RunId::new("run-root"),
                status: RunStatus::Completed,
            },
        },
        commit(3, user_entry("terminal-run-entry", None)),
    ];

    // act
    let unknown_result = replay_session(SessionId::new("session-root"), &unknown);
    let terminal_result = replay_session(SessionId::new("session-root"), &terminal);

    // assert
    assert_eq!(
        unknown_result,
        Err(SessionError::UnknownRun {
            run_id: RunId::new("run-root"),
        })
    );
    assert_eq!(
        terminal_result,
        Err(SessionError::TerminalRunMutation {
            run_id: RunId::new("run-root"),
        })
    );
}
