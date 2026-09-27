use crate::{
    event::*,
    session_lineage::*,
    store::{EventStore, Journal},
};
use serde_json::json;

pub(crate) fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        seq,
        event_id: format!("source-{seq}"),
        run_id: "source".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:source".into()),
        payload,
    }
}

#[tokio::test]
async fn child_materialization_validates_artifacts_and_publishes_without_changing_source(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    let writer = Journal::open(temp.path(), "source", true)?;
    crate::store::create_private_dir(&source.join("artifacts"))?;
    let artifact = source.join("artifacts/result.txt");
    std::fs::write(&artifact, b"result")?;
    let payloads = vec![
        EventV1::RunStarted(RunStartedEvent {
            run_name: "source".into(),
            workspace_root: temp.path().to_string_lossy().into(),
        }),
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: "agent".into(),
            profile: "default".into(),
            parent_agent_id: None,
        }),
        EventV1::ArtifactWritten(ArtifactWrittenEvent {
            path: "artifacts/result.txt".into(),
            digest: blake3::hash(b"result").to_hex().to_string(),
            bytes: 6,
            tool_call_id: None,
            tool_metadata: None,
            metadata: Default::default(),
        }),
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".into(),
        }),
    ];
    let events: Vec<_> = payloads
        .into_iter()
        .enumerate()
        .map(|(i, p)| writer.append(event(i as u64 + 1, p).into()))
        .collect::<Result<_, _>>()?;
    let source_bytes = std::fs::read(source.join("events.jsonl"))?;
    std::fs::write(
        source.join("meta.json"),
        serde_json::to_vec(
            &json!({"run_id":"source","run_name":"source","workspace_root":temp.path(),"config_digest":"config","harness_version":"test","mode_source":"interactive_mock","recorded_runtime_context":{"profile":"default","provider":"mock","model":"selected"}}),
        )?,
    )?;
    let stable = latest_clone_stable_prefix(&events)?;
    let request = |source_kind| ChildSessionMaterializationRequest {
        source_run_dir: &source,
        events: &events,
        stable_prefix: &stable,
        source_kind,
    };
    assert!(materialize_child_session(request(
        ChildSessionMaterializationSourceKind::DiskRunDirectory
    ))
    .is_err());
    let child = materialize_child_session(request(
        ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    ))?;
    assert_eq!(child.artifact_count, 1);
    assert_eq!(
        std::fs::read(child.child_run_dir.join("artifacts/result.txt"))?,
        b"result"
    );
    let copied = crate::store::read_events(&child.child_run_dir.join("events.jsonl"))?;
    assert_eq!(copied.len(), events.len());
    assert!(copied
        .iter()
        .all(|e| e.run_id.as_str() == child.child_run_id && !e.event_id.starts_with("source-")));
    assert!(crate::proj::inspect_resume_plan(&child.child_run_dir).is_resumable);
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(child.child_run_dir.join("meta.json"))?)?;
    assert_eq!(metadata["harness_lineage"]["parent_run_id"], "source");
    assert_eq!(metadata["recorded_runtime_context"]["model"], "selected");
    let before: Vec<_> = std::fs::read_dir(temp.path())?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    std::fs::write(&artifact, b"damage")?;
    assert!(materialize_child_session(request(
        ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot
    ))
    .is_err());
    assert_eq!(std::fs::read_dir(temp.path())?.count(), before.len());
    assert_eq!(std::fs::read(source.join("events.jsonl"))?, source_bytes);
    writer.close_writer()?;
    use std::sync::Arc;
    use tokio_stream::StreamExt;
    let provider = Arc::new(harness_providers::mock::MockProvider::default());
    let mut config = crate::coord::CoordinatorConfig::new(temp.path());
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let resumed = crate::coord::spawn_coordinator(
        config,
        Arc::new(crate::clock::FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    resumed
        .resume_run(child.child_run_id, "child resumed")
        .await?;
    let turn = resumed
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            "agent",
            "first turn",
        )
        .await?;
    let mut stream = resumed.event_store().await?.subscribe(1)?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = stream.next().await {
            if matches!(event?.payload,EventV1::TaskCompleted(e) if e.task_id.as_str()==turn) {
                return Ok::<_, crate::store::EventStoreError>(());
            }
        }
        Err(crate::store::EventStoreError::Invalid("missing child turn"))
    })
    .await??;
    resumed.stop_run().await?;
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(child.child_run_dir.join("meta.json"))?)?;
    assert_eq!(saved["harness_lineage"], metadata["harness_lineage"]);
    assert_eq!(provider.captured_requests().await[0].model_id, "selected");
    Ok(())
}

#[test]
fn live_forks_close_pending_work_and_lineage_trees_handle_cycles(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    std::fs::create_dir(&source)?;
    let payloads = vec![
        EventV1::RunStarted(RunStartedEvent {
            run_name: "live".into(),
            workspace_root: temp.path().to_string_lossy().into(),
        }),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "turn".into(),
            text: "pending".into(),
        }),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "turn".into(),
            state: TaskScheduleState::Started,
            queue_key: None,
            metadata: None,
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider".into(),
            provider_id: "mock".into(),
            model_id: "selected".into(),
            prompt_summary: "pending".into(),
            request_digest: "digest".into(),
            metadata: Some(ProviderRequestStartedMetadata {
                turn_id: Some("turn".into()),
                ..Default::default()
            }),
        }),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool".into(),
            tool_id: "read".into(),
            args_summary: "{}".into(),
            args_digest: "digest".into(),
            metadata: None,
        }),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "approval".into(),
            kind: "read".into(),
            tool_call_id: Some("tool".into()),
            summary: "read".into(),
            request_digest: "digest".into(),
            timeout_ms: 0,
            default_decision: PermissionDecision::Deny,
        }),
        EventV1::EditProposed(EditProposedEvent {
            edit_id: "edit".into(),
            path: "https://example.test/?token=private-historical-value".into(),
            summary: "write".into(),
            patch_digest: "digest".into(),
        }),
    ];
    let events: Vec<_> = payloads
        .into_iter()
        .enumerate()
        .map(|(i, p)| event(i as u64 + 1, p))
        .collect();
    assert!(latest_clone_stable_prefix(&events).is_err());
    let stable = validate_tui_fork_stable_prefix(&events, events.len() as u64)?;
    let child = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source,
        events: &events,
        stable_prefix: &stable,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    })?;
    let copied = crate::store::read_events(&child.child_run_dir.join("events.jsonl"))?;
    assert!(!serde_json::to_string(&copied)?.contains("private-historical-value"));
    validate_stable_prefix(&copied, copied.len() as u64)?;
    assert!(matches!(
        copied.last().map(|e| &e.payload),
        Some(EventV1::RunFinished(_))
    ));
    assert_eq!(
        copied
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ToolCallFinished(_)))
            .count(),
        1
    );
    assert!(!source.join("events.jsonl").exists());
    let catalog =
        crate::proj::project_session_catalog_entry(&copied, &child.child_run_id, None, None, None)?;
    let mut a = catalog.clone();
    a.run_id = "a".into();
    a.parent_session_id = Some("b".into());
    let mut b = catalog.clone();
    b.run_id = "b".into();
    b.parent_session_id = Some("a".into());
    let mut orphan = catalog;
    orphan.run_id = "orphan".into();
    orphan.parent_session_id = Some("missing".into());
    let tree = project_lineage_tree([a, b, orphan]);
    let mut stack: Vec<_> = tree.roots.iter().collect();
    let mut ids = std::collections::BTreeSet::new();
    while let Some(node) = stack.pop() {
        assert!(ids.insert(&node.entry.run_id));
        stack.extend(&node.children);
    }
    assert_eq!(ids.len(), 3);
    Ok(())
}

#[test]
fn forks_refuse_secret_artifacts_even_when_their_digests_are_valid(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    std::fs::create_dir_all(source.join("artifacts"))?;
    let bytes = b"api_key=do-not-copy-this";
    std::fs::write(source.join("artifacts/output.txt"), bytes)?;
    let events = vec![
        event(
            1,
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: "artifacts/output.txt".into(),
                digest: blake3::hash(bytes).to_hex().to_string(),
                bytes: bytes.len() as u64,
                tool_call_id: None,
                tool_metadata: None,
                metadata: Default::default(),
            }),
        ),
        event(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".into(),
            }),
        ),
    ];
    let stable = validate_tui_fork_stable_prefix(&events, 2)?;
    assert!(
        materialize_child_session(ChildSessionMaterializationRequest {
            source_run_dir: &source,
            events: &events,
            stable_prefix: &stable,
            source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
        })
        .is_err()
    );
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 1);
    assert_eq!(std::fs::read(source.join("artifacts/output.txt"))?, bytes);
    Ok(())
}
