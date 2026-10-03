use super::*;
use harness_core::{
    config::{
        ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget, SubagentDefinitionSnapshot,
    },
    event::EventV1,
    subagent::{FinalizedStateResult, SendSubagentMessageDelivery},
};
use harness_providers::{
    CompletionUsage, ProviderStreamEvent as Stream, ProviderStreamFinishedMetadata,
};
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

#[tokio::test]
async fn public_messages_enforce_limits_and_ownership_wake_the_same_child_and_respect_kill(
) -> Result<(), Box<dyn std::error::Error>> {
    let settled = || Stream::DoneWithMetadata {
        usage: Some(CompletionUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
        }),
        metadata: Some(ProviderStreamFinishedMetadata {
            settled_reasoning: Some(Vec::new()),
            usage_complete: Some(true),
            ..Default::default()
        }),
    };
    let provider = Arc::new(Gate {
        inner: MockProvider::script([
            vec![Stream::TextDelta("initial-wake-fixture".into()), settled()],
            vec![
                Stream::TextDelta("continued-wake-fixture".into()),
                settled(),
            ],
        ]),
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(1),
    });
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.subagents.messaging_enabled = true;
    let definitions = SubagentDefinitionSnapshot::default();
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_subagent_tools(
        &mut registry,
        &config.subagents,
        &definitions,
        config.subagent_model_catalog.as_ref(),
    );
    let schema = registry
        .get("send_subagent_message")
        .ok_or("enabled message tool missing")?
        .parameters_json_schema();
    assert!(schema["properties"].get("queue").is_none());
    assert!(schema["properties"].get("delivery").is_some());
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = registry.tool_ids();
    config.tool_registry = Arc::new(registry);
    config.subagent_definitions = Some(definitions);
    config.provider = Arc::<Gate>::clone(&provider);
    config.permission_policy = PermissionPolicy::allow_all();
    config.agent_profiles.insert("default".into(), parent);
    config.agent_model_targets.insert(
        "default".into(),
        ResolvedModelTarget {
            model_ref: "mock:default".into(),
            provider: "mock".into(),
            model: "default".into(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            limits: ResolvedModelLimits::from_values(
                Some(100_000),
                Some(98_000),
                Some(4096),
                ModelLimitProvenance::explicit("message wake fixture"),
            ),
            resolution: harness_core::model_resolution::resolve_model(
                harness_core::model_resolution::ModelResolutionInput {
                    provider: "mock",
                    model: "default",
                    metadata_family: None,
                    input_modalities: &[],
                    supports_tool_calls: Some(true),
                    supports_reasoning_summaries: Some(true),
                },
            ),
            catalog_entry: None,
        },
    );
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("public message wake", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let initial_started = provider.started.notified();
    tokio::pin!(initial_started);
    initial_started.as_mut().enable();
    let child = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"initial-message-fixture","description":"Message wake child","background":false}),
        )
        .await?
        .structured_json
        .ok_or("initial completion")?["subagent_id"]
        .as_str()
        .ok_or("initial child identity")?
        .to_owned();
    tokio::time::timeout(Duration::from_secs(3), initial_started).await?;
    assert!(matches!(
        coordinator.raw_finalized_state(child.clone()).await?,
        FinalizedStateResult::Available { .. }
    ));
    let before = coordinator.subagent_history().await?;
    let before = before.records.get(&child).ok_or("initial history")?;
    let initial_generation = before.generation;
    let initial_attempt = before.lifecycle.current_attempt_id().map(str::to_owned);
    for text in [String::new(), "\u{00e9}".repeat(16_385)] {
        let result = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "send_subagent_message",
                json!({"subagent_id":child,"text":text}),
            )
            .await?;
        assert!(!result.is_error());
        let data = result.structured_json.ok_or("limit result")?;
        assert_eq!(data["outcome"], "limit");
        assert_eq!(data["max_bytes"], 32_768);
        assert_eq!(data["observed_bytes"], text.len());
    }
    for input in [
        json!({"subagent_id":child,"text":"invalid delivery","delivery":"unknown"}),
        json!({"subagent_id":child,"text":"invalid legacy flag","queue":"true"}),
    ] {
        assert!(coordinator
            .execute_agent_tool_call(actor.clone(), None, "send_subagent_message", input)
            .await
            .is_err());
    }
    let unrelated = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let foreign = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(unrelated)),
            None,
            "send_subagent_message",
            json!({"subagent_id":child,"text":"foreign-message-fixture","sender":actor.agent_id,"generation":initial_generation}),
        )
        .await?;
    assert!(!foreign.is_error());
    assert_eq!(
        foreign.structured_json.ok_or("foreign result")?["outcome"],
        "not_found_or_not_owned"
    );
    assert_eq!(provider.inner.call_count(), 1);
    let wake_started = provider.started.notified();
    tokio::pin!(wake_started);
    wake_started.as_mut().enable();
    let mut events = coordinator.subscribe_new_events().await?;
    let accepted = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "send_subagent_message",
            json!({"subagent_id":child,"text":"accepted-wake-fixture","delivery":"queue","queue":false}),
        )
        .await?;
    assert!(!accepted.is_error());
    let accepted = accepted.structured_json.ok_or("accepted result")?;
    assert_eq!(accepted["outcome"], "accepted");
    assert!(!accepted["message_id"]
        .as_str()
        .ok_or("message receipt identity")?
        .is_empty());
    let receipt = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            let EventV1::NativeSubagentMessage(receipt) = event?.payload else {
                continue;
            };
            if Some(receipt.message_id.as_str()) == accepted["message_id"].as_str() {
                return Ok::<_, Box<dyn std::error::Error>>(receipt);
            }
        }
        Err("message receipt event stream closed".into())
    })
    .await??;
    assert_eq!(receipt.delivery, SendSubagentMessageDelivery::Queue);
    assert_eq!(Some(receipt.sender), actor.agent_id);
    assert_eq!(receipt.recipient, child);
    tokio::time::timeout(Duration::from_secs(3), wake_started).await?;
    let history = coordinator.subagent_history().await?;
    assert_eq!(
        history.records.len(),
        1,
        "wake must not create a new identity"
    );
    let woke = history.records.get(&child).ok_or("woken child history")?;
    assert!(woke.generation > initial_generation);
    assert_ne!(
        woke.lifecycle.current_attempt_id().map(str::to_owned),
        initial_attempt
    );
    provider.release.add_permits(1);
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        coordinator.execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[child],"timeout_ms":3000}),
        ),
    )
    .await??
    .structured_json
    .ok_or("wake output")?;
    assert_eq!(output["Result"]["task_id"], child);
    assert_eq!(output["Result"]["status"], "completed");
    let requests = provider.inner.captured_requests().await;
    assert_eq!(requests.len(), 2);
    let retained = serde_json::to_string(&requests[1].messages)?;
    assert!(retained.contains("initial-wake-fixture"));
    assert!(retained.contains("accepted-wake-fixture"));
    assert!(!retained.contains("foreign-message-fixture"));
    let killed = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "kill_command_or_subagent",
            json!({"task_id":child}),
        )
        .await?
        .structured_json
        .ok_or("kill completed child")?;
    assert_eq!(killed["Result"]["outcome"], "already_exited");
    let wake_started = provider.started.notified();
    tokio::pin!(wake_started);
    wake_started.as_mut().enable();
    let accepted = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "send_subagent_message",
            json!({"subagent_id":child,"text":"completed-kill-wake-fixture"}),
        )
        .await?;
    assert_eq!(
        accepted.structured_json.ok_or("completed kill wake")?["outcome"],
        "accepted"
    );
    tokio::time::timeout(Duration::from_secs(3), wake_started).await?;
    let killed = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "kill_command_or_subagent",
            json!({"task_id":child}),
        )
        .await?
        .structured_json
        .ok_or("kill active child")?;
    assert_eq!(killed["Result"]["outcome"], "killed");
    let cancelled = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[child],"timeout_ms":3000}),
        )
        .await?
        .structured_json
        .ok_or("killed active output")?;
    assert_eq!(cancelled["Result"]["status"], "cancelled");
    let rejected = coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "send_subagent_message",
            json!({"subagent_id":child,"text":"killed-wake-fixture"}),
        )
        .await?;
    assert!(!rejected.is_error());
    assert_eq!(
        rejected.structured_json.ok_or("killed wake rejection")?["outcome"],
        "not_active_or_finalizing"
    );
    coordinator.stop_run().await?;
    assert_eq!(
        provider.inner.call_count(),
        2,
        "explicit kill must prevent wake"
    );
    Ok(())
}
