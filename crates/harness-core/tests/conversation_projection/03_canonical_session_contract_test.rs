use std::path::Path;

#[test]
fn canonical_session_type_contract() {
    // arrange
    let ids_source = include_str!("../../src/ids.rs");
    let lib_source = include_str!("../../src/lib.rs");

    // act
    let required_declarations = [
        "id_newtype!(SessionId);",
        "id_newtype!(EntryId);",
        "id_newtype!(TurnId);",
        "id_newtype!(RunId);",
        "id_newtype!(ProviderRequestId);",
        "id_newtype!(ToolCallId);",
    ];

    // assert
    for declaration in required_declarations {
        assert!(
            ids_source.contains(declaration),
            "missing distinct canonical identity declaration `{declaration}`"
        );
    }
    assert!(
        lib_source.contains("pub mod session;"),
        "missing public canonical session domain"
    );
}

#[test]
fn canonical_active_path_contract() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    // act
    let required_modules = [
        source_root.join("session.rs"),
        source_root.join("session/model.rs"),
        source_root.join("session/reducer.rs"),
        source_root.join("session/legacy.rs"),
    ];

    // assert
    for module in required_modules {
        assert!(
            module.is_file(),
            "missing canonical active-path module `{}`",
            module.display()
        );
    }
}

#[test]
fn canonical_projection_owns_core_durable_consumers() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let projection_path = source_root.join("session/projection.rs");
    let session_facade =
        std::fs::read_to_string(source_root.join("session.rs")).expect("read session facade");
    let committed_context = std::fs::read_to_string(
        source_root.join("coord/provider_context/committed.rs"),
    )
    .expect("read committed provider context");

    // act
    let projection_exists = projection_path.is_file();
    let projection_is_exported = session_facade.contains("mod projection;")
        && session_facade.contains("CanonicalSessionProjection");
    let committed_context_is_canonical =
        committed_context.contains("CanonicalSessionProjection")
            && !committed_context.contains("project_conversation");

    // assert
    assert!(
        projection_exists,
        "canonical durable projection must live at `{}`",
        projection_path.display()
    );
    assert!(
        projection_is_exported,
        "session facade must export CanonicalSessionProjection"
    );
    assert!(
        committed_context_is_canonical,
        "live provider context must consume CanonicalSessionProjection instead of project_conversation"
    );
}

#[test]
#[expect(deprecated, reason = "fixture proves shipped V1 compaction decoding")]
fn legacy_decoder_isolated_from_active_runtime_consumers() {
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

    // Given: a shipped V1 compaction lifecycle inside an otherwise canonical journal.
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

    // When: active projections consume the compatibility classification.
    let stable = validate_stable_prefix(&events, 4).unwrap_or_abort();
    let transcript = project_transcript(&events).unwrap_or_abort();
    let timeline = project_timeline_index(&events).unwrap_or_abort();

    // Then: lifecycle state is stable, deprecated records stay presentation-silent, and audit
    // naming remains exact for the immutable journal.
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

#[test]
fn one_provider_context_builder_and_compaction_writer_remain() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let provider_context = std::fs::read_to_string(
        source_root.join("coord/provider_context.rs"),
    )
    .expect("read provider context owner");
    let committed = std::fs::read_to_string(
        source_root.join("coord/provider_context/committed.rs"),
    )
    .expect("read committed context adapter");
    let recovery = std::fs::read_to_string(
        source_root.join("coord/provider_context/restore/lower.rs"),
    )
    .expect("read recovery context adapter");
    let compaction = std::fs::read_to_string(
        source_root.join("coord/session_compaction/pipeline.rs"),
    )
    .expect("read compaction pipeline");

    // act
    let context_builder_count = [&provider_context, &committed, &recovery]
        .iter()
        .map(|source| source.matches("checkpoint: None,").count())
        .sum::<usize>();
    let compaction_writer_count = compaction
        .matches("EventV1::SessionCompaction(SessionCompactionEvent")
        .count();

    // assert
    assert_eq!(
        context_builder_count, 1,
        "provider context construction must have one production owner"
    );
    assert_eq!(
        compaction_writer_count, 1,
        "Compaction V2 must have exactly one success writer"
    );
}

#[test]
fn compaction_boundary_has_no_active_legacy_module() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let cut_point =
        std::fs::read_to_string(source_root.join("coord/compaction/cut_point.rs"))
            .expect("read compaction cut-point boundary");
    let session_projection =
        std::fs::read_to_string(source_root.join("session/projection.rs"))
            .expect("read canonical session projection");

    // act
    let active_legacy_module = cut_point.contains("mod legacy;");
    let session_depends_on_coordinator = session_projection.contains("crate::coord");

    // assert
    assert!(
        !active_legacy_module,
        "active Compaction V2 boundaries must not route through a legacy-named module"
    );
    assert!(
        !session_depends_on_coordinator,
        "pure session projection must not depend on coordinator authority"
    );
}
