use super::*;
use harness_core::config::{SubagentDefinition, SubagentDefinitionSnapshot};
use harness_providers::ProviderStreamEvent as Stream;
use serde_json::json;

#[tokio::test]
async fn child_provider_receives_actor_resolved_type_schema_instead_of_global_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::TextDelta("resolved-schema-fixture".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("nested-schema-fixture".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.subagents.max_depth = 2;
    let mut definitions = SubagentDefinitionSnapshot::default();
    for (name, tools) in [
        (
            "fixture-reviewer",
            vec!["Agent(fixture-specialist)".into(), "Bash".into()],
        ),
        (
            "fixture-specialist",
            vec!["Agent(fixture-specialist)".into()],
        ),
    ] {
        definitions.user.insert(
            name.into(),
            SubagentDefinition {
                name: name.into(),
                description: name.into(),
                tools,
                inject_default_tools: false,
                ..Default::default()
            },
        );
    }
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_subagent_tools(&mut registry, &config.subagents, &definitions, None);
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = registry.tool_ids();
    config.agent_profiles.insert("default".into(), parent);
    config.tool_registry = Arc::new(registry);
    config.subagent_definitions = Some(definitions);
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::<MockProvider>::clone(&provider);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("actor schema", temp.path()).await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let first = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(parent)),
            None,
            "spawn_subagent",
            json!({"prompt":"inspect schema","description":"Schema child","subagent_type":"fixture-reviewer","background":false}),
        )
        .await?
        .structured_json
        .ok_or("first child result")?["subagent_id"]
        .as_str()
        .ok_or("first child ID")?
        .to_owned();
    let first_history = coordinator.subagent_history().await?;
    assert_eq!(
        first_history
            .records
            .get(&first)
            .and_then(|record| record.metadata.as_ref())
            .map(|metadata| metadata.injected_depth.0),
        Some(1)
    );
    let second = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(first.clone())),
            None,
            "spawn_subagent",
            json!({"prompt":"inspect nested schema","description":"Nested schema child","subagent_type":"fixture-specialist","background":false}),
        )
        .await?
        .structured_json
        .ok_or("second child result")?["subagent_id"]
        .as_str()
        .ok_or("second child ID")?
        .to_owned();
    let third_attempt = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(second.clone())),
            None,
            "spawn_subagent",
            json!({"prompt":"must not run","description":"Over-depth child","subagent_type":"fixture-specialist","background":true}),
        )
        .await;
    let history = coordinator.subagent_history().await?;
    let requests = provider.captured_requests().await;
    coordinator.stop_run().await?;
    assert!(
        third_attempt.is_err(),
        "depth-two actor unexpectedly registered a third child"
    );
    assert_eq!(
        history
            .records
            .get(&second)
            .and_then(|record| record.metadata.as_ref())
            .map(|metadata| metadata.injected_depth.0),
        Some(2)
    );
    let tools = requests
        .first()
        .and_then(|request| request.tools.as_ref())
        .ok_or("first child tool definitions")?;
    let spawn = tools
        .iter()
        .find(|tool| tool.tool_id == "spawn_subagent")
        .ok_or("child spawn tool")?;
    assert_eq!(
        spawn.parameters["properties"]["subagent_type"]["enum"],
        json!(["fixture-specialist"])
    );
    for alias in ["task", "get_task_output", "wait_tasks", "kill_task"] {
        assert!(!tools.iter().any(|tool| tool.tool_id == alias));
    }
    let nested_request = requests.get(1).ok_or("second child provider request")?;
    assert!(
        nested_request
            .tools
            .as_ref()
            .is_none_or(|tools| !tools.iter().any(|tool| tool.tool_id == "spawn_subagent")),
        "depth-two child provider request exposed spawn_subagent"
    );
    Ok(())
}
