use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::{load_config_from_str, ShellAllowlist},
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionDecision, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use harness_providers::{
    mock::MockProvider, CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent,
};
use serde_json::json;
use std::{fs, sync::Arc, time::Duration};
use tokio_stream::StreamExt;

struct Script {
    inner: MockProvider,
    started: tokio::sync::Notify,
    gate: tokio::sync::Semaphore,
}
#[async_trait::async_trait]
impl Provider for Script {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        if request
            .messages
            .last()
            .is_some_and(|m| m.content.starts_with("Wait."))
        {
            self.started.notify_one();
            if let Ok(permit) = self.gate.acquire().await {
                permit.forget();
            }
        }
        self.inner.stream_completion(request).await
    }
}

#[tokio::test]
async fn task_skills_require_approval_and_children_continue_under_their_own_policy(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let skill = temp.path().join(".agent-harness/skills/review");
    fs::create_dir_all(&skill)?;
    fs::write(skill.join("SKILL.md"), "---\nname: review\ndescription: Review code\nallowed_tools: [write]\n---\nReview the ownership boundaries.\n")?;
    load_config_from_str("{skills:{global_roots:[], permissions:{review:'ask'}}}")?;
    let provider = Arc::new(Script {
        inner: MockProvider::script(
            [
                "initial report",
                "continued report",
                "background report",
                "resumed report",
            ]
            .into_iter()
            .map(|text| {
                vec![
                    ProviderStreamEvent::TextDelta(text.into()),
                    ProviderStreamEvent::Done { usage: None },
                ]
            }).chain(std::iter::once(vec![ProviderStreamEvent::ToolCallComplete {
                tool_call_id:"spawn-background".into(),function_name:"task".into(),
                arguments_json:json!({"subagent_type":"reviewer","prompt":"Wait. Inspect the change.","run_in_background":true,"load_skills":[]}).to_string(),
            }, ProviderStreamEvent::Done{usage:None}])).chain(["parent acknowledged","child final","parent notified"].into_iter().map(|text|vec![ProviderStreamEvent::TextDelta(text.into()),ProviderStreamEvent::Done{usage:None}])),
        ),
        started: tokio::sync::Notify::new(),
        gate: tokio::sync::Semaphore::new(0),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "skill".into(),
            pattern: "review".into(),
            action: PermissionAction::Ask,
        },
    ])?;
    let mut parent = AgentProfile::fallback("default");
    parent.model_ref = "mock:chosen".into();
    parent.toolset = ["task", "skill", "background_output", "background_cancel"]
        .map(str::to_owned)
        .into();
    config.agent_profiles.insert("default".into(), parent);
    config
        .agent_profiles
        .insert("reviewer".into(), AgentProfile::fallback("reviewer"));
    let handle = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("tasks", temp.path()).await?;
    let parent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent.clone()));
    let args = || json!({"subagent_type":"reviewer","prompt":"Review this change.","description":"Review","run_in_background":false,"load_skills":["review","review"]});
    let mut events = handle.event_store().await?.subscribe(1)?;
    let task = {
        let (handle, actor, args) = (handle.clone(), actor.clone(), args());
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(actor, None, "task", args)
                .await
        })
    };
    let permission = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if let EventV1::PermissionRequested(p) = event?.payload {
                return Ok::<_, harness_core::store::EventStoreError>(p.permission_id);
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "approval missing",
        ))
    })
    .await??;
    assert_eq!(provider.inner.call_count(), 0);
    handle
        .resolve_permission(permission, PermissionDecision::Allow, None)
        .await?;
    let result = tokio::time::timeout(Duration::from_secs(2), task).await???;
    assert_eq!(result.display_text, "initial report");
    let requests = provider.inner.captured_requests().await;
    assert_eq!(requests[0].model_id, "chosen");
    assert!(
        requests[0].tools.as_ref().is_none_or(Vec::is_empty),
        "skill metadata cannot grant tools"
    );
    let prompt = &requests[0].messages.last().ok_or("child prompt")?.content;
    assert_eq!(
        prompt.matches("Review the ownership boundaries.").count(),
        1
    );
    let child = result
        .structured_json
        .as_ref()
        .and_then(|v| v["session_id"].as_str())
        .ok_or("child id")?
        .to_owned();
    let continued=handle.execute_agent_tool_call(actor.clone(),None,"task",json!({"session_id":child,"prompt":"Continue.","run_in_background":false,"load_skills":[]})).await?;
    assert_eq!(continued.display_text, "continued report");
    assert_eq!(
        provider.inner.captured_requests().await[1].messages.len(),
        3
    );
    let output = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "background_output",
            json!({"session_id":child}),
        )
        .await?;
    assert_eq!(output.display_text, "continued report");
    let first_request = result
        .structured_json
        .as_ref()
        .and_then(|v| v["request_id"].as_str())
        .ok_or("first request")?;
    let earlier = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "background_output",
            json!({"request_id":first_request}),
        )
        .await?;
    assert_eq!(earlier.display_text, "initial report");
    let other = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    assert!(handle
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(other)),
            None,
            "background_output",
            json!({"session_id":child})
        )
        .await
        .is_err());
    let mut missing = args();
    missing["load_skills"] = json!(["missing"]);
    assert!(handle
        .execute_agent_tool_call(actor.clone(), None, "task", missing)
        .await
        .is_err());
    assert!(handle.execute_agent_tool_call(actor.clone(),None,"task",json!({"session_id":child,"subagent_type":"default","prompt":"Change role.","run_in_background":false,"load_skills":[]})).await.is_err());
    assert_eq!(provider.inner.call_count(), 2);
    let events = harness_core::store::read_events(&run.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(
                |e| matches!(&e.payload,EventV1::AgentSpawned(a) if a.parent_agent_id.is_some())
            )
            .count(),
        1
    );
    let background=handle.execute_agent_tool_call(actor.clone(),None,"task",json!({"subagent_type":"reviewer","prompt":"Wait.","run_in_background":true,"load_skills":[]})).await?;
    let background = background
        .structured_json
        .as_ref()
        .and_then(|v| v["session_id"].as_str())
        .ok_or("background handle")?
        .to_owned();
    tokio::time::timeout(Duration::from_secs(2), provider.started.notified()).await?;
    let timed_out = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "background_output",
            json!({"session_id":background,"block":true,"timeout_ms":1}),
        )
        .await?;
    assert_eq!(
        timed_out.structured_json.as_ref().ok_or("running output")?["status"],
        "running"
    );
    let waiting = {
        let (handle, actor, id) = (handle.clone(), actor.clone(), background.clone());
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor,
                    None,
                    "background_output",
                    json!({"session_id":id,"block":true,"timeout_ms":1000}),
                )
                .await
        })
    };
    let mut events = handle.event_store().await?.subscribe(1)?;
    tokio::time::timeout(Duration::from_secs(2),async {
        let mut started=0;
        while let Some(event)=events.next().await {
            started+=usize::from(matches!(&event?.payload,EventV1::ToolCallRequested(e) if e.tool_id=="background_output"));
            if started==5 {break;}
        }
        Ok::<_,harness_core::store::EventStoreError>(())
    }).await??;
    provider.gate.add_permits(1);
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(500), waiting)
            .await???
            .display_text,
        "background report"
    );
    let cancelling=handle.execute_agent_tool_call(actor.clone(),None,"task",json!({"session_id":background,"prompt":"Wait. Cancel this.","run_in_background":true,"load_skills":[]})).await?;
    tokio::time::timeout(Duration::from_secs(2), provider.started.notified()).await?;
    let request = cancelling
        .structured_json
        .as_ref()
        .and_then(|v| v["request_id"].as_str())
        .ok_or("cancel handle")?;
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "background_cancel",
            json!({"request_id":request}),
        )
        .await?;
    let cancelled = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "background_output",
            json!({"request_id":request,"block":true}),
        )
        .await?;
    assert_eq!(
        cancelled.structured_json.as_ref().ok_or("cancel output")?["status"],
        "cancelled"
    );
    handle.stop_run().await?;
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
            json!({"session_id":child}),
        )
        .await?;
    assert_eq!(output.display_text, "continued report");
    let continued=resumed.execute_agent_tool_call(actor,None,"task",json!({"session_id":child,"prompt":"After resume.","run_in_background":false,"load_skills":[]})).await?;
    assert_eq!(continued.display_text, "resumed report");
    assert_eq!(
        provider.inner.captured_requests().await[3].messages.len(),
        5
    );
    let mut events = resumed.subscribe_new_events().await?;
    let turn = resumed
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            parent.clone(),
            "Delegate background work.",
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(2), provider.started.notified()).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload,EventV1::TaskCompleted(task) if task.task_id.as_str()==turn)
            {
                return Ok::<_, harness_core::store::EventStoreError>(());
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "parent did not settle",
        ))
    })
    .await??;
    provider.gate.add_permits(1);
    let (seq, notification) = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            let event = event?;
            if let EventV1::BackgroundTaskNotification(notification) = event.payload {
                return Ok::<_, harness_core::store::EventStoreError>((event.seq, notification));
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "notification missing",
        ))
    })
    .await??;
    let follow_up = notification
        .delivered_turn_request_id
        .ok_or("background notification did not wake its parent")?;
    tokio::time::timeout(Duration::from_secs(2),async {
        while let Some(event)=events.next().await {
            let event=event?;
            if matches!(event.payload,EventV1::TaskCompleted(task) if task.task_id.as_str()==follow_up) {assert!(event.seq>seq);return Ok::<_,harness_core::store::EventStoreError>(());}
        }
        Err(harness_core::store::EventStoreError::Invalid("notification turn did not settle"))
    }).await??;
    let last = provider
        .inner
        .captured_requests()
        .await
        .pop()
        .ok_or("follow-up request")?;
    assert!(last
        .messages
        .last()
        .ok_or("follow-up prompt")?
        .content
        .contains("child final"));
    resumed.stop_run().await?;
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "task".into(),
            pattern: "reviewer".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "recheck permissions")
        .await?;
    let before = provider.inner.call_count();
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    assert!(resumed.execute_agent_tool_call(actor,None,"task",json!({"session_id":child,"prompt":"Forbidden continuation.","run_in_background":false,"load_skills":[]})).await.is_err());
    assert_eq!(
        provider.inner.call_count(),
        before,
        "continuation must recheck current profile permissions"
    );
    resumed.stop_run().await?;
    Ok(())
}
