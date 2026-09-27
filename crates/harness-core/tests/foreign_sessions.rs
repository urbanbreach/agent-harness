use harness_core::{event::*, foreign_session::*, proj::SessionModeSource};

#[test]
fn discovery_and_import_preserve_sources_and_publish_only_valid_replay_sessions(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("codex-source");
    let broken = temp.path().join("claude-broken");
    std::fs::create_dir(&source)?;
    std::fs::create_dir(&broken)?;
    let event = EventEnvelopeV1 {
        schema_version: 1,
        event_id: "old-1".into(),
        seq: 1,
        run_id: "old".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: EventV1::RunFinished(RunFinishedEvent {
            summary: "done api_key=private-key".into(),
        }),
    };
    let bytes = serde_json::to_vec(&event)?;
    std::fs::write(source.join("events.jsonl"), &bytes)?;
    std::fs::write(
        broken.join("events.jsonl"),
        [bytes.as_slice(), b"\n{broken"].concat(),
    )?;
    let summary = summarize_discover_candidates(&discover_foreign_sessions(temp.path())?);
    assert_eq!((summary.importable, summary.corrupt), (1, 1));
    assert_eq!(std::fs::read_dir(&source)?.count(), 1);
    let destination = temp.path().join("imported");
    assert!(import_foreign_session_as_replay(&broken, &destination).is_err());
    assert!(!destination.exists());
    assert!(import_foreign_session_as_replay(&source, &source).is_err());
    let imported = import_foreign_session_as_replay(&source, &destination)?;
    assert_eq!(imported.mode_source, SessionModeSource::ReplayOnly);
    let events = harness_core::store::read_events(&imported.run_dir.join("events.jsonl"))?;
    assert_eq!(events.len(), imported.event_count);
    assert!(events
        .iter()
        .all(|e| e.run_id.as_str() == imported.run_id && e.event_id != "old-1"));
    assert!(!serde_json::to_string(&events)?.contains("private-key"));
    assert_eq!(std::fs::read(source.join("events.jsonl"))?, bytes);
    assert!(!harness_core::proj::inspect_resume_plan(&imported.run_dir).is_resumable);
    assert_eq!(
        harness_core::proj::read_run_metadata(&imported.run_dir)?
            .ok_or("metadata missing")?
            .mode_source,
        Some(SessionModeSource::ReplayOnly)
    );
    assert!(refuse_import_into_active_session(&source, &imported.run_dir).is_err());
    Ok(())
}
