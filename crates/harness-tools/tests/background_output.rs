use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::json;
use std::sync::Arc;

struct NotificationGate {
    parent: tokio::sync::Semaphore,
    child: tokio::sync::Semaphore,
    parent_started: tokio::sync::Notify,
    child_started: tokio::sync::Notify,
}
#[async_trait::async_trait]
impl harness_providers::Provider for NotificationGate {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        let Some(last) = request.messages.last() else {
            return Box::pin(tokio_stream::iter([Stream::error("missing prompt")]));
        };
        if last.content == "launch" {
            return Box::pin(tokio_stream::iter([Stream::ToolCallComplete {
                tool_call_id: "launch-child".into(), function_name: "task".into(),
                arguments_json: json!({"subagent_type":"child","prompt":"child work","run_in_background":true,"load_skills":[]}).to_string(),
            }, Stream::Done { usage: None }]));
        }
        let answer = if last.content == "child work" {
            self.child_started.notify_one();
            if let Ok(permit) = self.child.acquire().await {
                permit.forget();
            }
            "child report"
        } else if last.role == harness_providers::MessageRole::Tool {
            self.parent_started.notify_one();
            if let Ok(permit) = self.parent.acquire().await {
                permit.forget();
            }
            "parent finished"
        } else {
            assert!(
                last.content.contains("child report"),
                "notification must include the child result"
            );
            "notification delivered"
        };
        Box::pin(tokio_stream::iter([
            Stream::TextDelta(answer.into()),
            Stream::Done { usage: None },
        ]))
    }
}

#[tokio::test]
async fn accepted_background_work_reserves_parent_queue_capacity_for_its_notification(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::event::EventV1;
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(NotificationGate {
        parent: tokio::sync::Semaphore::new(0),
        child: tokio::sync::Semaphore::new(0),
        parent_started: tokio::sync::Notify::new(),
        child_started: tokio::sync::Notify::new(),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config.command_buffer = 1;
    config.provider_model_concurrency = 2;
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = vec!["task".into()];
    config.agent_profiles.insert("default".into(), parent);
    config
        .agent_profiles
        .insert("child".into(), AgentProfile::fallback("child"));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("notification capacity", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            parent.clone(),
            "launch",
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(
            provider.parent_started.notified(),
            provider.child_started.notified()
        )
    })
    .await?;
    assert!(
        coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                parent.clone(),
                "manual queued work"
            )
            .await
            .is_err(),
        "the accepted child must retain a notification slot"
    );
    provider.child.add_permits(1);
    let notification = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if let EventV1::BackgroundTaskNotification(e) = event?.payload {
                return Ok::<_, harness_core::store::EventStoreError>(e);
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "missing notification",
        ))
    })
    .await??;
    let followup = notification
        .delivered_turn_request_id
        .ok_or("lost notification")?;
    assert!(
        coordinator
            .compact_agent_context(parent, None, "manual")
            .await
            .is_err(),
        "manual compaction must respect the same queue capacity"
    );
    provider.parent.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCompleted(e) if e.task_id.as_str() == followup && e.result_summary == "notification delivered") { return Ok::<_, harness_core::store::EventStoreError>(()); }
        }
        Err(harness_core::store::EventStoreError::Invalid("notification was not consumed"))
    }).await??;
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn background_history_is_owned_paginated_and_redacted_without_retaining_reasoning(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(
        temp.path().join("note"),
        "child tool output credential-for-history-redaction",
    )?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ReasoningDelta("private reasoning".into()),
            Stream::ToolCallComplete {
                tool_call_id: "read-note".into(),
                function_name: "read".into(),
                arguments_json: json!({"filePath":"note"}).to_string(),
            },
            Stream::ToolCallComplete {
                tool_call_id: "write-note".into(),
                function_name: "write".into(),
                arguments_json: json!({"filePath":"child.txt","content":"created by the child\n"})
                    .to_string(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("first report".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("continued report".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("fork report".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config
        .secret_values
        .push("credential-for-history-redaction".into());
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = vec!["task".into(), "background_output".into()];
    config.agent_profiles.insert("default".into(), parent);
    let mut child = AgentProfile::fallback("child");
    child.toolset = vec!["read".into(), "write".into()];
    config.agent_profiles.insert("child".into(), child);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("background history", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let output = coordinator.execute_agent_tool_call(actor.clone(), None, "task", json!({"subagent_type":"child","prompt":"Read the note", "run_in_background":false,"load_skills":[]})).await?;
    let child = output.structured_json.ok_or("missing child")?["session_id"]
        .as_str()
        .ok_or("missing child id")?
        .to_owned();
    coordinator.execute_agent_tool_call(actor.clone(), None, "task", json!({"session_id":child,"prompt":"Continue", "run_in_background":false,"load_skills":[]})).await?;
    let child_dir = config.session_dir.join(&child);
    let child_events = harness_core::store::read_events(&child_dir.join("events.jsonl"))?;
    assert!(child_events.iter().all(|e| e.run_id.as_str() == child));
    assert!(
        harness_core::store::Journal::open_existing(&config.session_dir, &child, false).is_err(),
        "the parent owns its child's writer until shutdown"
    );
    for artifact in child_events.iter().filter_map(|e| match &e.payload {
        harness_core::event::EventV1::ArtifactWritten(e) => Some(e),
        _ => None,
    }) {
        let bytes = std::fs::read(child_dir.join(&artifact.path))?;
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), artifact.digest);
    }
    assert!(child_events
        .iter()
        .any(|e| matches!(e.payload, harness_core::event::EventV1::EditApplied(_))));
    let output = coordinator.execute_agent_tool_call(actor.clone(), None, "background_output", json!({"session_id":child,"full_session":true,"include_tool_results":true,"include_thinking":true,"thinking_max_chars":1,"timeout":0})).await?;
    let value = output.structured_json.ok_or("missing history")?;
    assert_eq!(value["full_session"]["message_count"], 5);
    assert_eq!(
        value["full_session"]["tool_results"]
            .as_array()
            .ok_or("missing tool results")?
            .len(),
        2
    );
    assert!(value["full_session"]["tool_results"]
        .as_array()
        .ok_or("missing tool output")?
        .iter()
        .any(|v| v["text"]
            .as_str()
            .is_some_and(|s| s.contains("child tool output"))));
    assert_eq!(value["thinking"]["unavailable"], true);
    assert!(!value.to_string().contains("private reasoning"));
    assert!(!value
        .to_string()
        .contains("credential-for-history-redaction"));
    assert!(!value.to_string().contains("arguments_json"));
    let cursor = value["full_session"]["messages"][2]["event_id"]
        .as_str()
        .ok_or("missing cursor")?;
    let output = coordinator.execute_agent_tool_call(actor.clone(), None, "background_output", json!({"session_id":child,"full_session":true,"since_message_id":cursor,"message_limit":1,"from_end":true})).await?;
    let page = output.structured_json.ok_or("missing page")?;
    assert_eq!(page["full_session"]["message_count"], 1);
    assert_eq!(
        page["full_session"]["messages"][0]["text"],
        "continued report"
    );
    assert_eq!(page["full_session"]["message_truncated"], true);
    for args in [
        json!({"session_id":child,"full_session":true,"since_message_id":"unknown"}),
        json!({"request_ids":[child,"other"],"full_session":true,"wait_mode":"all"}),
    ] {
        assert!(coordinator
            .execute_agent_tool_call(actor.clone(), None, "background_output", args)
            .await
            .is_err());
    }
    let other = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    assert!(coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(other)),
            None,
            "background_output",
            json!({"session_id":child,"full_session":true})
        )
        .await
        .is_err());
    coordinator.stop_run().await?;
    let child_events = harness_core::store::read_events(&child_dir.join("events.jsonl"))?;
    let metadata = harness_core::proj::read_run_metadata(&child_dir)?
        .map(|m| harness_core::proj::SessionCatalogMetadata::from(&m));
    let catalog = harness_core::proj::project_session_catalog_entry(
        &child_events,
        &child,
        metadata.as_ref(),
        None,
        None,
    )?;
    assert_eq!(
        catalog.parent_session_id.as_deref(),
        Some(run.run_id.as_str())
    );
    assert!(catalog.is_resumable);
    assert_eq!(catalog.profile_preset.as_deref(), Some("child"));
    let prefix = std::fs::read(child_dir.join("events.jsonl"))?;
    let root_prefix = std::fs::read(&run.events_path)?;
    let root_events = harness_core::store::read_events(&run.events_path)?;
    let stable = harness_core::session_lineage::latest_clone_stable_prefix(&root_events)?;
    let fork = harness_core::session_lineage::materialize_child_session(
        harness_core::session_lineage::ChildSessionMaterializationRequest {
            source_run_dir: &run.run_dir, events: &root_events, stable_prefix: &stable,
            source_kind: harness_core::session_lineage::ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
    )?;
    let fork_events = harness_core::store::read_events(&fork.child_run_dir.join("events.jsonl"))?;
    let fork_child = fork_events
        .iter()
        .find_map(|e| match &e.payload {
            harness_core::event::EventV1::AgentSpawned(e) if e.parent_agent_id.is_some() => {
                Some(e.agent_id.clone())
            }
            _ => None,
        })
        .ok_or("fork lost child")?;
    assert_ne!(fork_child, child, "a branch must own its child sessions");
    let branch = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    branch.resume_run(fork.child_run_id, "parent fork").await?;
    let output = branch.execute_agent_tool_call(actor.clone(), None, "task", json!({"session_id":fork_child,"prompt":"branch only", "run_in_background":false,"load_skills":[]})).await?;
    assert_eq!(output.display_text, "fork report");
    branch.stop_run().await?;
    assert_eq!(std::fs::read(&run.events_path)?, root_prefix);
    assert_eq!(std::fs::read(child_dir.join("events.jsonl"))?, prefix);
    let standalone = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    standalone
        .resume_run(child.clone(), "standalone child")
        .await?;
    assert!(coordinator
        .resume_run(run.run_id.to_string(), "conflicting parent")
        .await
        .is_err());
    assert_eq!(std::fs::read(&run.events_path)?, root_prefix);
    standalone.stop_run().await?;
    assert!(std::fs::read(child_dir.join("events.jsonl"))?.starts_with(&prefix));
    for missing in [false, true] {
        if missing {
            std::fs::remove_dir_all(&child_dir)?;
        } else {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(child_dir.join("events.jsonl"))?
                .write_all(b"{\"")?;
        }
        let resumed = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed
            .resume_run(run.run_id.to_string(), "resumed")
            .await?;
        let output = resumed
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "background_output",
                json!({"session_id":child,"full_session":true,"message_limit":200}),
            )
            .await?;
        assert_eq!(
            output.structured_json.ok_or("missing resumed history")?["full_session"]
                ["message_count"],
            5
        );
        assert_eq!(provider.call_count(), 4);
        resumed.stop_run().await?;
        let recovered = harness_core::store::read_events(&child_dir.join("events.jsonl"))?;
        harness_core::proj::project_resume_plan(&recovered, &child)?;
        assert_eq!(
            recovered
                .iter()
                .filter(|e| matches!(
                    e.payload,
                    harness_core::event::EventV1::UserMessageSubmitted(_)
                ))
                .count(),
            2
        );
    }
    Ok(())
}
