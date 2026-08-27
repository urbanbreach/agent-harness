use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::ids::RunId;
use harness_core::session::legacy::{LegacyEventLogAdapter, LegacyIdentityNamespace};
use harness_core::session::SessionEntryPayload;
use harness_core::UnwrapOrAbort;
use harness_providers::CompletionUsage;
use serde_json::json;

#[test]
fn session_compaction_defaults_v2_fields_when_legacy_json_is_deserialized() {
    // arrange
    // act
    // assert
    // Given: a SessionCompaction payload written before Compaction V2.
    let json = json!({
        "event_type": "session_compaction",
        "data": {
            "agent_id": "agent-1",
            "summary": "legacy summary",
            "first_kept_event_seq": 2,
            "tokens_before": 100,
            "trigger_reason": "manual",
            "from_hook": false
        }
    });

    // When: the old payload crosses the current serde boundary.
    let EventV1::SessionCompaction(event) = serde_json::from_value(json).unwrap_or_abort() else {
        panic!("expected SessionCompaction")
    };

    // Then: no Compaction V2 value is fabricated.
    assert_eq!(
        (
            event.first_kept_entry_id,
            event.tokens_after,
            event.summary_usage,
            event.summary_provider_id,
            event.summary_model_id,
            event.read_files,
            event.modified_files,
            event.current_intent,
        ),
        (None, None, None, None, None, Vec::new(), Vec::new(), None)
    );
}

#[test]
fn session_compaction_round_trips_every_v2_field() {
    // arrange
    // act
    // assert
    // Given: one fully populated durable Compaction V2 payload.
    let payload = serde_json::from_value::<EventV1>(json!({
        "event_type": "session_compaction",
        "data": {
            "agent_id": "agent-1",
            "summary": "typed summary",
            "first_kept_event_seq": 2,
            "first_kept_entry_id": "entry-2",
            "tokens_before": 100,
            "tokens_after": 37,
            "summary_usage": {
                "prompt_tokens": 11,
                "completion_tokens": 5,
                "total_tokens": 16
            },
            "summary_provider_id": "mock",
            "summary_model_id": "model-1",
            "read_files": ["src/read.rs"],
            "modified_files": ["src/modified.rs"],
            "current_intent": {
                "intent": "current_task",
                "params": { "current_intent": "finish task 12" }
            },
            "trigger_reason": "manual",
            "from_hook": false
        }
    }))
    .unwrap_or_abort();

    // When: the event is serialized and deserialized through the durable variant.
    let round_trip =
        serde_json::from_value::<EventV1>(serde_json::to_value(&payload).unwrap_or_abort())
            .unwrap_or_abort();

    // Then: every typed field survives exactly.
    assert_eq!(round_trip, payload);
}

#[test]
fn canonical_compaction_summary_defaults_v2_fields_when_legacy_json_is_deserialized() {
    // arrange
    // act
    // assert
    // Given: the canonical CompactionSummary shape persisted before Compaction V2.
    let json = json!({
        "kind": "compaction_summary",
        "summary": "legacy canonical summary",
        "first_kept_entry_id": "entry-2"
    });

    // When: the entry payload crosses the current serde boundary.
    let SessionEntryPayload::CompactionSummary {
        tokens_after,
        summary_usage,
        summary_provider_id,
        summary_model_id,
        preserved_state,
        ..
    } = serde_json::from_value(json).unwrap_or_abort()
    else {
        panic!("expected CompactionSummary")
    };

    // Then: no Compaction V2 metadata is fabricated.
    assert_eq!(
        (
            tokens_after,
            summary_usage,
            summary_provider_id,
            summary_model_id,
            preserved_state,
        ),
        (None, None, None, None, None)
    );
}

#[test]
fn session_compaction_adapter_maps_v2_fields_to_canonical_summary() {
    // arrange
    // act
    // assert
    // Given: a typed compaction boundary and all summary-generation metadata.
    let run_id = RunId::new("run-compaction-shape");
    let first_kept_entry_id =
        LegacyIdentityNamespace::new(&run_id).entry_id(2, "event-2", "user_message");
    let events = vec![
        envelope(
            1,
            &run_id,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "compaction-shape".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            &run_id,
            serde_json::from_value(json!({
                "event_type": "user_message_submitted",
                "data": { "request_id": "request-1", "text": "keep me" }
            }))
            .unwrap_or_abort(),
        ),
        envelope(
            3,
            &run_id,
            serde_json::from_value(json!({
                "event_type": "session_compaction",
                "data": {
                    "agent_id": "agent-1",
                    "summary": "typed summary",
                    "first_kept_event_seq": 2,
                    "first_kept_entry_id": first_kept_entry_id,
                    "tokens_before": 100,
                    "tokens_after": 37,
                    "summary_generation_usage": {
                        "prompt_tokens": 11,
                        "completion_tokens": 5,
                        "total_tokens": 16
                    },
                    "summary_provider_id": "mock",
                    "summary_model_id": "model-1",
                    "read_files": ["src/read.rs"],
                    "modified_files": ["src/modified.rs"],
                    "current_intent": {
                        "intent": "current_task",
                        "params": { "current_intent": "finish task 12" }
                    },
                    "trigger_reason": "manual",
                    "from_hook": false
                }
            }))
            .unwrap_or_abort(),
        ),
    ];

    // When: the one EventV1 compatibility adapter builds the canonical session.
    let snapshot = LegacyEventLogAdapter::new()
        .project(&events)
        .unwrap_or_abort();
    let summary = snapshot
        .session
        .entries()
        .values()
        .find_map(|entry| match &entry.payload {
            SessionEntryPayload::CompactionSummary {
                summary,
                first_kept_entry_id,
                tokens_after,
                summary_usage,
                summary_provider_id,
                summary_model_id,
                preserved_state,
            } => Some((
                summary,
                first_kept_entry_id,
                tokens_after,
                summary_usage,
                summary_provider_id,
                summary_model_id,
                preserved_state,
            )),
            _ => None,
        })
        .unwrap_or_abort();

    // Then: canonical history carries the same typed data.
    assert_eq!(
        summary,
        (
            &"typed summary".to_string(),
            &first_kept_entry_id,
            &Some(37),
            &Some(CompletionUsage {
                prompt_tokens: 11,
                completion_tokens: 5,
                total_tokens: 16,
            }),
            &Some("mock".to_string()),
            &Some("model-1".to_string()),
            &Some(Box::new(harness_core::session::CompactionPreservedState {
                read_files: vec!["src/read.rs".to_string()],
                modified_files: vec!["src/modified.rs".to_string()],
                current_intent: Some(harness_core::event::UiIntentReceivedEvent {
                    intent: "current_task".to_string(),
                    params: std::collections::BTreeMap::from([(
                        "current_intent".to_string(),
                        "finish task 12".to_string(),
                    )]),
                }),
            })),
        )
    );
}

fn envelope(seq: u64, run_id: &RunId, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("event-{seq}"),
        seq,
        run_id: run_id.clone(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("agent-1".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload,
    }
}
