use super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use tokio_stream::StreamExt;

struct OrderedTool(Arc<tokio::sync::Notify>);
#[async_trait::async_trait]
impl Tool for OrderedTool {
    fn id(&self) -> &'static str {
        "ordered"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object","properties":{"wait":{"type":"boolean"}},"required":["wait"]})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(&self, _: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        if args["wait"] == true {
            self.0.notified().await;
        }
        Ok(ToolResult::structured("tool completed", args))
    }
}

#[tokio::test]
async fn replay_preserves_provider_call_order_when_tools_finish_in_reverse(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::compaction_tests::answer;
    let temp = tempfile::tempdir()?;
    let release = Arc::new(tokio::sync::Notify::new());
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(OrderedTool(Arc::clone(&release))));
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "first".into(),
                function_name: "ordered".into(),
                arguments_json: r#"{"wait":true}"#.into(),
            },
            Stream::ToolCallComplete {
                tool_call_id: "second".into(),
                function_name: "ordered".into(),
                arguments_json: r#"{"wait":false}"#.into(),
            },
            Stream::Done { usage: None },
        ],
        answer("done"),
        answer("before restart"),
        answer("after restart"),
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(tools);
    config.permission_policy = PermissionPolicy::allow_all();
    if let Some(profile) = config.agent_profiles.get_mut("default") {
        profile.toolset = vec!["ordered".into()];
    }
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("ordered results", temp.path())
        .await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let first = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "tools",
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::ToolCallFinished(e) if e.tool_call_id.as_str().ends_with("-tool-1")) { return Ok::<_, EventStoreError>(()); }
        }
        Err(EventStoreError::Invalid("second tool did not finish"))
    }).await??;
    release.notify_one();
    super::history_tests::settled(&coordinator, &first).await?;
    let next = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "before",
        )
        .await?;
    super::history_tests::settled(&coordinator, &next).await?;
    let before = provider.captured_requests().await[2].messages.clone();
    assert_eq!(before[2].tool_call_id.as_deref(), Some("first"));
    assert_eq!(before[3].tool_call_id.as_deref(), Some("second"));
    assert_eq!(
        serde_json::from_str::<Value>(&before[2].content)?["data"]["wait"],
        true
    );
    assert_eq!(
        serde_json::from_str::<Value>(&before[3].content)?["data"]["wait"],
        false
    );
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "resumed")
        .await?;
    let next = resumed
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "after")
        .await?;
    super::history_tests::settled(&resumed, &next).await?;
    let after = provider.captured_requests().await[3].messages.clone();
    assert_eq!(&after[..before.len()], &before);
    resumed.stop_run().await?;
    Ok(())
}
