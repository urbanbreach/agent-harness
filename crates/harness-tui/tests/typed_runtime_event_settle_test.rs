#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "runtime event integration tests use fail-fast assertions"
)]

use harness_core::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1, EventV1,
    LiveEventEnvelope, LiveEventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent,
    RuntimeEvent, TaskCompletedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::session::legacy::LegacyWarning;
use harness_core::session::{
    AssistantPart, AssistantToolCall, CanonicalSessionProjection, SessionEntryPayload,
};
use harness_tui::app::AppState;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn durable(seq: u64, payload: EventV1) -> RuntimeEvent {
    RuntimeEvent::Durable(Box::new(durable_envelope(seq, payload)))
}

fn durable_envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-durable-{seq}"),
        seq,
        run_id: "run-typed-runtime".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-1".to_string())),
        correlation_id: Some("turn-1".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent-1".to_string()),
        payload,
    }
}

fn live(event_id: &str, payload: LiveEventV1) -> RuntimeEvent {
    RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: event_id.to_string(),
        run_id: "run-typed-runtime".into(),
        mono_ms: 3,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-1".to_string())),
        correlation_id: Some("turn-1".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent-1".to_string()),
        payload,
    }))
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, 120, 40), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

#[test]
fn interleaved_live_responses_keep_their_place_between_tool_calls() {
    // Given: a turn that has already called a tool.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_runtime_event(durable(
        1,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "turn-1".into(),
            text: "inspect and verify".to_string(),
        }),
    ));
    let start = |seq, request_id: &str| {
        durable(
            seq,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model".to_string(),
                prompt_summary: "inspect and verify".to_string(),
                request_digest: "digest".to_string(),
                metadata: None,
            }),
        )
    };
    let commit = |seq, request_id: &str, parts: Vec<AssistantPart>| {
        durable(
            seq,
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: request_id.into(),
                tool_call_count: parts
                    .iter()
                    .filter(|part| matches!(part, AssistantPart::ToolCall(_)))
                    .count(),
                parts,
                provenance: None,
                assistant_message: None,
            }),
        )
    };
    let tool = |id: &str, command: &str| {
        AssistantPart::ToolCall(AssistantToolCall {
            tool_call_id: id.into(),
            provider_tool_call_id: None,
            tool_id: "bash".to_string(),
            args_summary: serde_json::json!({ "command": command }).to_string(),
            args_digest: "digest".to_string(),
            provider_call_id: None,
        })
    };
    app.ingest_runtime_event(start(2, "provider-1"));
    app.ingest_runtime_event(commit(3, "provider-1", vec![tool("tool-1", "ls")]));
    app.ingest_runtime_event(start(4, "provider-2"));

    // When: the next provider step speaks, calls another tool, and commits.
    app.ingest_runtime_event(live(
        "live-middle",
        LiveEventV1::ProviderTextDelta {
            request_id: "provider-2".into(),
            delta: "Intermediate response".to_string(),
        },
    ));
    app.ingest_runtime_event(commit(
        5,
        "provider-2",
        vec![
            AssistantPart::Text {
                text: "Intermediate response".to_string(),
            },
            tool("tool-2", "pwd"),
        ],
    ));
    app.ingest_runtime_event(start(6, "provider-3"));
    app.ingest_runtime_event(live(
        "live-final",
        LiveEventV1::ProviderTextDelta {
            request_id: "provider-3".into(),
            delta: "Final response".to_string(),
        },
    ));

    // Then: committed and currently streaming text both remain in sequence.
    let rendered = render(&app);
    let positions = ["ls", "Intermediate response", "pwd", "Final response"].map(|text| {
        rendered
            .find(text)
            .unwrap_or_else(|| panic!("missing {text}\n{rendered}"))
    });
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{rendered}"
    );
    assert_eq!(
        rendered.matches("Intermediate response").count(),
        1,
        "{rendered}"
    );
}

#[test]
fn typed_live_fragments_render_then_final_commit_settles_them() {
    // Given: a live turn with its durable request barrier.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_runtime_event(durable(
        1,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "turn-1".into(),
            text: "question".to_string(),
        }),
    ));
    app.ingest_runtime_event(durable(
        2,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider-1".into(),
            provider_id: "mock".to_string(),
            model_id: "model".to_string(),
            prompt_summary: "question".to_string(),
            request_digest: "request-digest".to_string(),
            metadata: None,
        }),
    ));

    // When: typed live fragments arrive without durable sequence numbers.
    app.ingest_runtime_event(live(
        "live-reasoning",
        LiveEventV1::ProviderReasoningDelta {
            request_id: "provider-1".into(),
            delta: "draft reasoning".to_string(),
        },
    ));
    let thinking = render(&app);
    assert!(thinking.contains("draft reasoning"), "{thinking}");
    app.ingest_runtime_event(live(
        "live-text",
        LiveEventV1::ProviderTextDelta {
            request_id: "provider-1".into(),
            delta: "draft answer".to_string(),
        },
    ));
    app.ingest_runtime_event(live(
        "live-tool",
        LiveEventV1::ProviderToolInputDelta {
            request_id: "provider-1".into(),
            tool_call_id: "tool-1".into(),
            delta: "{\"path\":\"draft\"}".to_string(),
        },
    ));

    // Then: live content is visible but is not added to durable event history.
    let transient = render(&app);
    assert!(transient.contains("draft answer"), "{transient}");
    assert!(transient.contains("draft"), "{transient}");
    assert_eq!(app.selected_event().map(|event| event.seq), Some(2));

    // When: the durable assistant commit arrives with canonical content.
    app.ingest_runtime_event(durable(
        3,
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: "provider-1".into(),
            tool_call_count: 1,
            parts: vec![
                AssistantPart::Reasoning {
                    text: "final reasoning".to_string(),
                },
                AssistantPart::Text {
                    text: "final answer".to_string(),
                },
                AssistantPart::ToolCall(AssistantToolCall {
                    tool_call_id: "tool-1".into(),
                    provider_tool_call_id: None,
                    tool_id: "read".to_string(),
                    args_summary: "{\"path\":\"final\"}".to_string(),
                    args_digest: "args-digest".to_string(),
                    provider_call_id: None,
                }),
            ],
            provenance: None,
            assistant_message: None,
        }),
    ));

    // Then: canonical content replaces every transient fragment in place.
    let settled = render(&app);
    assert!(settled.contains("final answer"), "{settled}");
    assert!(settled.contains("final"), "{settled}");
    assert!(!settled.contains("draft answer"), "{settled}");
    assert_eq!(settled.matches("Reading 1 file").count(), 1, "{settled}");
    assert!(!settled.contains("Read draft"), "{settled}");
    assert_eq!(app.selected_event().map(|event| event.seq), Some(3));

    let canonical = app
        .canonical_projection()
        .expect("durable events must update the core projection");
    let canonical_text = canonical
        .session
        .entries()
        .values()
        .filter_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage { parts, .. } => Some(parts.as_slice()),
            _ => None,
        })
        .flatten()
        .filter_map(|part| match part {
            AssistantPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(canonical_text, vec!["final answer"]);
    assert!(canonical
        .compatibility_warnings
        .contains(&LegacyWarning::MissingProviderFinish {
            request_id: "provider-1".to_string(),
        }));
    assert_eq!(app.canonical_projection_error(), None);
}

#[test]
fn tui_durable_content_uses_core_canonical_projection() {
    // arrange
    let events = vec![
        durable_envelope(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-1".into(),
                text: "canonical question".to_string(),
            }),
        ),
        durable_envelope(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider-1".into(),
                provider_id: "mock".to_string(),
                model_id: "model".to_string(),
                prompt_summary: "canonical question".to_string(),
                request_digest: "request-digest".to_string(),
                metadata: None,
            }),
        ),
        durable_envelope(
            3,
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider-1".into(),
                tool_call_count: 0,
                parts: vec![AssistantPart::Text {
                    text: "canonical answer".to_string(),
                }],
                provenance: None,
                assistant_message: None,
            }),
        ),
    ];
    let expected = harness_core::session::CanonicalSessionProjection::from_event_history(&events)
        .expect("fixture must project");

    // act
    let mut app = AppState::new_live(None, false, None);
    app.replace_events(events);

    // assert
    let actual = app
        .canonical_projection()
        .expect("settled history must have a canonical projection");
    assert_eq!(actual.session.entries(), expected.session.entries());
    assert_eq!(actual.transcript.messages, expected.transcript.messages);
    assert_eq!(actual.run_summary.status, expected.run_summary.status);
    let rendered = render(&app);
    assert!(rendered.contains("canonical question"), "{rendered}");
    assert!(rendered.contains("canonical answer"), "{rendered}");
    assert_eq!(app.canonical_projection_generation(), 1);
}

#[test]
fn live_settlement_projects_once_without_replaying_each_durable_event() {
    // Given: the full provider-finish, assistant-finish, task-completion sequence.
    let mut app = AppState::new_live(None, false, None);
    let events = vec![
        durable_envelope(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-1".into(),
                text: "question".to_string(),
            }),
        ),
        durable_envelope(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider-1".into(),
                provider_id: "mock".to_string(),
                model_id: "model".to_string(),
                prompt_summary: "question".to_string(),
                request_digest: "request-digest".to_string(),
                metadata: None,
            }),
        ),
        durable_envelope(
            3,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider-1".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("output-digest".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        durable_envelope(
            4,
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider-1".into(),
                tool_call_count: 0,
                parts: vec![AssistantPart::Text {
                    text: "answer".to_string(),
                }],
                provenance: None,
                assistant_message: None,
            }),
        ),
        durable_envelope(
            5,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task-1".into(),
                result_summary: "answer".to_string(),
                result_digest: "result-digest".to_string(),
                metadata: None,
            }),
        ),
    ];

    // When: each event arrives independently, only the three finish events settle.
    for (index, event) in events.iter().enumerate() {
        app.ingest_runtime_event(RuntimeEvent::Durable(Box::new(event.clone())));
        assert_eq!(
            app.canonical_projection_generation(),
            event.seq.saturating_sub(2)
        );
        assert_eq!(app.canonical_projection_error(), None);
        if index >= 2 {
            let expected = CanonicalSessionProjection::from_event_history(&events[..=index])
                .expect("each finish boundary must project");
            assert_eq!(app.canonical_projection(), Some(&expected));
        } else {
            assert_eq!(app.canonical_projection(), None);
        }
    }

    // Then: a failing settlement reports the fresh-projection error without advancing state.
    let before = app.canonical_projection().cloned();
    let generation = app.canonical_projection_generation();
    let mut invalid = events[4].clone();
    invalid.seq = 6;
    let mut attempted = events.clone();
    attempted.push(invalid.clone());
    let error = CanonicalSessionProjection::from_event_history(&attempted)
        .expect_err("duplicate event identity must fail")
        .to_string();
    app.ingest_runtime_event(RuntimeEvent::Durable(Box::new(invalid)));
    assert_eq!(app.canonical_projection_error(), Some(error.as_str()));
    assert_eq!(app.canonical_projection_generation(), generation);
    assert_eq!(app.canonical_projection(), before.as_ref());
}

#[test]
fn legacy_compaction_display_is_derived_from_canonical_compatibility_projection() {
    // arrange
    let payload = serde_json::from_value(serde_json::json!({
        "event_type": "compaction_applied",
        "data": {
            "checkpoint_id": "checkpoint-legacy",
            "agent_id": "agent-1",
            "through_seq": 1,
            "through_request_id": "turn-1",
            "tokens_before_estimate": 600,
            "tokens_after_estimate": 240,
            "summary_tokens_estimate": 40,
            "compacted_turns": 1,
            "preserved_turns": 1,
            "reduction_tokens_estimate": 360,
            "reduction_percent_estimate": 60,
            "estimate_source": "legacy"
        }
    }))
    .expect("legacy fixture must decode");
    let mut app = AppState::new_live(None, false, None);

    // act
    app.replace_events(vec![durable_envelope(1, payload)]);

    // assert
    let canonical = app
        .canonical_projection()
        .expect("legacy history must project through compatibility");
    assert_eq!(
        canonical
            .run_summary
            .counts
            .by_type
            .get("compaction_applied"),
        Some(&1)
    );
    assert!(canonical.compatibility_warnings.iter().any(|warning| {
        matches!(warning, LegacyWarning::UnsupportedLegacyVariant { event_id } if event_id == "evt-durable-1")
    }));
    assert_eq!(app.canonical_projection_generation(), 1);
    let status = app
        .settled_compaction_status()
        .expect("compatibility projection must yield a read-only status");
    assert_eq!(status.message, "compaction applied · legacy compatibility");
}
