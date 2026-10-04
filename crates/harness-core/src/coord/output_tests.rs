use super::tests::CountTool;
use super::*;
use crate::perm::{PermissionAction, PermissionRule};
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError},
};
use harness_providers::{mock::MockProvider, MessageRole, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

struct LargeOutput;
#[async_trait::async_trait]
impl Tool for LargeOutput {
    fn id(&self) -> &str {
        "large"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, _: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let mut output = ToolResult::structured(
            format!(
                "{}sk-1234567890abcdef\nAPI_KEY=\"private-quoted\"",
                "line\n".repeat(20_000)
            ),
            json!({"password":"private-output", "is_error":true}),
        );
        if let Some(kind) = args["attachment"].as_str() {
            let (mime, content) = match kind {
                "credential" => ("text/plain", "API_KEY=\"attachment-secret\""),
                "invalid" => ("image/png", "not an image"),
                _ => ("text/plain", "safe content"),
            };
            let attachment = crate::attachment_transport::AttachmentMetadata::from_bytes(
                "file",
                mime,
                None,
                content.as_bytes(),
                None,
            );
            output.attachments = vec![attachment; if kind == "duplicate" { 2 } else { 1 }];
        }
        Ok(output)
    }
}
#[tokio::test]
async fn large_results_are_bounded_and_link_to_a_redacted_durable_artifact(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(LargeOutput));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("output", temp.path()).await?;
    for kind in ["credential", "invalid", "duplicate"] {
        assert!(
            coordinator
                .execute_agent_tool_call(
                    EventActor::new(ActorKind::User, None),
                    None,
                    "large",
                    json!({"attachment":kind})
                )
                .await
                .is_err(),
            "{kind} attachment must fail"
        );
    }
    assert!(
        crate::store::read_events(&run.events_path)?
            .iter()
            .all(|e| !matches!(e.payload, EventV1::ArtifactWritten(_))),
        "invalid media must be rejected before writing even the text artifact"
    );
    assert!(!run.artifacts_dir.exists() || std::fs::read_dir(&run.artifacts_dir)?.next().is_none());
    let result = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "large",
            json!({}),
        )
        .await?;
    assert!(result.display_text.len() < 52_000 && result.display_text.lines().count() < 2010);
    assert!(result.is_error());
    assert_eq!(result.artifacts.len(), 1);
    let artifact = &result.artifacts[0];
    let bytes = std::fs::read(run.run_dir.join(&artifact.path))?;
    assert_eq!(artifact.digest, blake3::hash(&bytes).to_hex().as_str());
    let text = std::str::from_utf8(&bytes)?;
    let _: Value = serde_json::from_slice(&bytes)?;
    assert!(!text.contains("private-quoted"));
    assert!(!text.contains("1234567890abcdef") && !text.contains("private-output"));
    assert!(text.contains("REDACTED") && text.len() > result.display_text.len());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(run.run_dir.join(&artifact.path))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    assert!(events.iter().any(|e| matches!(&e.payload, EventV1::ArtifactWritten(a) if a.path == artifact.path && a.digest == artifact.digest)));
    Ok(())
}

#[tokio::test]
async fn turn_waits_for_permission_then_feeds_tool_result_back_without_journaling_deltas(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CountTool(Arc::clone(&calls))));
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::Start,
            Stream::ReasoningDelta("private reasoning opaque-credential Bearer abc.def.ghi".into()),
            Stream::TextDelta("checking opaque-".into()),
            Stream::TextDelta("credential Bearer abc.".into()),
            Stream::TextDelta("def.ghi ".into()),
            Stream::ToolCallDelta {
                tool_call_id: "call-1".into(),
                function_name: Some("count".into()),
                arguments_delta: "{\"token\":\"opaque-".into(),
            },
            Stream::ToolCallDelta {
                tool_call_id: "call-1".into(),
                function_name: None,
                arguments_delta: "credential\"}".into(),
            },
            Stream::ToolCallComplete {
                tool_call_id: "call-1".into(),
                function_name: "count".into(),
                arguments_json: "{\"token\":\"opaque-credential\"}".into(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::Start,
            Stream::TextDelta("finished".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.secret_values = vec!["opaque-credential".into()];
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.provider_model_concurrency = 1;
    config.tool_concurrency = 1;
    config.permission_policy = PermissionPolicy::from_rules(vec![PermissionRule {
        permission: "*".into(),
        pattern: "*".into(),
        action: PermissionAction::Ask,
    }])?;
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["count".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("test", temp.path()).await?;
    let store = coordinator.event_store().await?;
    let observed = store.replay(1)?.next().await.ok_or("missing run event")??;
    let mut forbidden: crate::store::EventEnvelopeWithoutSeqV1 = observed.into();
    forbidden.event_id = "unauthorized-append".into();
    forbidden.payload = EventV1::RunFinished(RunFinishedEvent {
        summary: "must not be written".into(),
    });
    assert!(store.append(forbidden).is_err());
    let mut events = store.subscribe(1)?;
    let mut runtime = store.subscribe_runtime(1)?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let task = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "check")
        .await?;
    let permission = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if let EventV1::PermissionRequested(permission) = event?.payload {
                return Ok::<_, EventStoreError>(permission);
            }
        }
        Err(EventStoreError::Invalid("permission was not requested"))
    })
    .await??;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    coordinator
        .resolve_permission(permission.permission_id, PermissionDecision::Allow, None)
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCompleted(ref done) if done.task_id.as_str() == task) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("turn did not complete"))
    }).await??;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].context.initiator,
        harness_providers::ProviderRequestInitiator::User
    );
    assert_eq!(
        requests[1].context.initiator,
        harness_providers::ProviderRequestInitiator::Agent
    );
    assert!(requests[1]
        .messages
        .iter()
        .any(|m| m.role == MessageRole::Tool
            && m.content == "tool result"
            && m.tool_call_id.as_deref() == Some("call-1")));
    coordinator.stop_run().await?;
    let mut visible = [String::new(), String::new(), String::new()];
    let mut tool_names = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = runtime.next().await {
            match event? {
                RuntimeEvent::Live(event) => {
                    let (index, delta, tool_name) = match event.payload {
                        LiveEventV1::ProviderTextDelta { delta, .. } => (0, delta, None),
                        LiveEventV1::ProviderReasoningDelta { delta, .. } => (1, delta, None),
                        LiveEventV1::ProviderToolInputDelta {
                            tool_name, delta, ..
                        } => (2, delta, tool_name),
                        _ => continue,
                    };
                    visible[index].push_str(&delta);
                    tool_names.extend(tool_name);
                }
                RuntimeEvent::Durable(event)
                    if matches!(event.payload, EventV1::RunFinished(_)) =>
                {
                    return Ok::<_, EventStoreError>(())
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("missing run completion"))
    })
    .await??;
    assert_eq!(
        visible,
        [
            "checking [REDACTED] Bearer [REDACTED] finished",
            "private reasoning [REDACTED] Bearer [REDACTED]",
            "{\"token\":\"[REDACTED]\"}",
        ]
    );
    assert!(!tool_names.is_empty());
    assert!(tool_names.iter().all(|name| name == "count"));
    let journal = std::fs::read_to_string(run.events_path)?;
    assert!(!journal.contains("private reasoning"));
    assert!(!journal.contains("opaque-credential"));
    assert!(!journal.contains("provider_stream_delta"));
    assert!(journal.contains("finished"));
    Ok(())
}
