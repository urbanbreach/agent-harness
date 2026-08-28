#[test]
#[expect(deprecated, reason = "fixture proves shipped V1 compaction decoding")]
fn legacy_compaction_replay_is_presentation_silent() {
    use harness_core::event::{
        ActorKind, CompactionAppliedEvent, CompactionRequestedEvent, EventActor, EventEnvelopeV1,
        EventV1, RunFinishedEvent, RunStartedEvent, SCHEMA_VERSION,
    };
    use harness_core::proj::project_timeline_index;
    use harness_core::session_lineage::validate_stable_prefix;
    use harness_core::transcript_projection::project_transcript;
    use harness_core::UnwrapOrAbort;

    fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq}"),
            seq,
            run_id: "run-legacy-boundary".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    // arrange
    let events = vec![
        event(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "legacy boundary".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        event(
            2,
            EventV1::CompactionRequested(CompactionRequestedEvent {
                checkpoint_id: "checkpoint-1".to_string(),
                agent_id: "agent-1".to_string(),
                trigger_reason: "legacy fixture".to_string(),
                through_seq: 1,
                through_request_id: None,
                provider_id: None,
                model_id: None,
                tokens_before: None,
                tokens_before_estimate: None,
                estimate_source: None,
            }),
        ),
        event(
            3,
            EventV1::CompactionApplied(CompactionAppliedEvent {
                checkpoint_id: "checkpoint-1".to_string(),
                agent_id: "agent-1".to_string(),
                through_seq: 1,
                through_request_id: None,
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                estimate_source: None,
            }),
        ),
        event(
            4,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    // act
    let stable = validate_stable_prefix(&events, 4).unwrap_or_abort();
    let transcript = project_transcript(&events).unwrap_or_abort();
    let timeline = project_timeline_index(&events).unwrap_or_abort();

    // assert
    assert_eq!(stable.cutoff_seq, 4);
    assert!(transcript.compaction_checkpoints.is_empty());
    assert_eq!(
        timeline
            .events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "run_started",
            "compaction_requested",
            "compaction_applied",
            "run_finished",
        ]
    );
}
