use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::{ResolvedModelLimits, ResolvedModelTarget, ShellAllowlist},
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult},
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio_stream::StreamExt;

struct Probe {
    started: Semaphore,
    release: [Semaphore; 2],
}
#[async_trait::async_trait]
impl Tool for Probe {
    fn id(&self) -> &str {
        "probe"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        vec![(
            "probe".into(),
            if args["deny"] == true {
                "blocked"
            } else {
                "allowed"
            }
            .into(),
        )]
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let slot = args["slot"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n < 2)
            .ok_or_else(|| ToolError::InvalidArguments("slot missing".into()))?;
        self.started.add_permits(1);
        tokio::select! {
            () = ctx.cancellation.cancelled() => return Err(ToolError::Cancelled),
            permit = self.release[slot].acquire() => permit.map_err(|_| ToolError::Cancelled)?.forget(),
        }
        Ok(ToolResult::structured(
            if slot == 0 { "first" } else { "second" },
            json!({"is_error":slot == 1}),
        ))
    }
}

#[tokio::test]
async fn batches_share_capacity_preserve_order_permissions_and_cancellation(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let probe = Arc::new(Probe {
        started: Semaphore::new(0),
        release: [Semaphore::new(0), Semaphore::new(0)],
    });
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    registry.register(Arc::clone(&probe) as Arc<dyn Tool>);
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.provider = Arc::new(harness_providers::mock::MockProvider::script([
        vec![
            harness_providers::ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "child-batch".into(),
                function_name: "batch".into(),
                arguments_json: json!({"tool_calls":[{"tool":"probe","parameters":{"slot":0}}]})
                    .to_string(),
            },
            harness_providers::ProviderStreamEvent::Done { usage: None },
        ],
        vec![
            harness_providers::ProviderStreamEvent::TextDelta("child batch done".into()),
            harness_providers::ProviderStreamEvent::Done { usage: None },
        ],
    ]));
    config.tool_concurrency = 2;
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "probe".into(),
            pattern: "blocked".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let mut profile = AgentProfile::fallback("default");
    profile.model_ref = "mock:batch".into();
    profile.toolset = vec!["batch".into(), "probe".into(), "spawn_subagent".into()];
    config.agent_profiles.insert("default".into(), profile);
    config.agent_model_targets.insert(
        "default".into(),
        ResolvedModelTarget {
            model_ref: "mock:batch".into(),
            provider: "mock".into(),
            model: "batch".into(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            limits: ResolvedModelLimits::default(),
            resolution: harness_core::model_resolution::resolve_model(
                harness_core::model_resolution::ModelResolutionInput {
                    provider: "mock",
                    model: "batch",
                    metadata_family: None,
                    input_modalities: &[],
                    supports_tool_calls: Some(true),
                    supports_reasoning_summaries: Some(true),
                },
            ),
            catalog_entry: None,
        },
    );
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = handle.start_run("batch", root.path()).await?;
    let agent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent));
    let mut events = handle.subscribe_new_events().await?;
    let execution = {
        let handle = handle.clone();
        let actor = actor.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor,
                    None,
                    "batch",
                    json!({"tool_calls":[
                        {"tool":"probe","parameters":{"slot":0}},
                        {"tool":"probe","arguments":{"slot":1}},
                        {"tool":"probe","args":{"slot":0,"deny":true}},
                        {"tool":"batch","parameters":{}},
                        {"tool":"unknown","parameters":{}},
                    ]}),
                )
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), probe.started.acquire_many(2))
        .await??
        .forget();
    probe.release[1].add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::ToolCallFinished(ref t) if t.output_summary.as_deref() == Some("second")) { return Ok::<_, Box<dyn std::error::Error>>(()); }
        }
        Err("second call did not complete".into())
    }).await??;
    probe.release[0].add_permits(1);
    let output = tokio::time::timeout(Duration::from_secs(2), execution).await???;
    assert!(output.is_error());
    let result = output.structured_json.ok_or("batch results missing")?;
    assert_eq!(result["successful"], 1);
    assert_eq!(result["failed"], 4);
    assert_eq!(result["details"][0]["output"], "first");
    assert_eq!(result["details"][1]["output"], "second");
    assert_eq!(result["details"][2]["status"], "failed");
    let history = harness_core::store::read_events(&info.events_path)?;
    let batch = history
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::ToolCallRequested(t) if t.tool_id == "batch" => {
                Some(t.tool_call_id.to_string())
            }
            _ => None,
        })
        .ok_or("batch missing")?;
    let children: Vec<_> = history
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::ToolCallRequested(t) if t.tool_id == "probe" => Some((e, t)),
            _ => None,
        })
        .collect();
    assert_eq!(children.len(), 3);
    for (event, tool) in children {
        assert_eq!(event.actor, actor);
        assert_eq!(
            tool.metadata
                .as_ref()
                .and_then(|m| m.lineage.as_ref())
                .and_then(|l| l.parent_tool_call_id.as_deref()),
            Some(batch.as_str())
        );
    }
    let batch = handle.request_tool_call(actor.clone(), None, "batch", json!({"tool_calls":[{"tool":"probe","parameters":{"slot":0}},{"tool":"probe","parameters":{"slot":1}}]})).await?;
    tokio::time::timeout(Duration::from_secs(2), probe.started.acquire_many(2))
        .await??
        .forget();
    handle.cancel_task(batch.clone(), "cancel batch").await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::ToolCallFinished(ref t) if t.tool_call_id.as_str() == batch) { return Ok::<_, Box<dyn std::error::Error>>(()); }
        }
        Err("cancelled batch did not finish".into())
    }).await??;
    assert!(handle
        .execute_agent_tool_call(actor.clone(), None, "batch", json!({"tool_calls":[]}))
        .await
        .is_err());
    probe.release[0].add_permits(1);
    let child = handle
        .execute_agent_tool_call(
            actor,
            None,
            "spawn_subagent",
            json!({"prompt":"Run a batch.","description":"Run nested batch","background":false}),
        )
        .await?;
    let child = child.structured_json.ok_or("missing child result")?;
    assert_eq!(child["output"], "child batch done");
    let child = child["subagent_id"]
        .as_str()
        .ok_or("missing child id")?
        .to_owned();
    handle.stop_run().await?;
    let events = harness_core::store::read_events(&info.events_path)?;
    let catalog = harness_core::proj::project_session_catalog_entry(
        &events,
        info.run_id.as_str(),
        None,
        None,
        None,
    )?;
    assert_eq!(
        catalog.parent_session_id, None,
        "a child's nested tools cannot turn the root session into a child"
    );
    let nested = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::ToolCallRequested(t)
                if t.tool_id == "probe" && e.actor.agent_id.as_deref() == Some(&child) =>
            {
                t.metadata.as_ref().and_then(|m| m.lineage.as_ref())
            }
            _ => None,
        })
        .ok_or("nested child lineage missing")?;
    assert_eq!(nested.child_provider_id.as_deref(), Some("mock"));
    assert_eq!(nested.child_model_id.as_deref(), Some("batch"));
    assert_eq!(nested.child_session_id.as_deref(), Some(child.as_str()));
    Ok(())
}
