use super::*;
use harness_core::config::{
    SubagentCatalogAuthority, SubagentCatalogModel, SubagentDefinition, SubagentDefinitionSnapshot,
    SubagentModelCatalog, SubagentRuntimeConfig,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn native_schema_and_lenient_output_dispatch_keep_aliases_and_cardinality_distinct(
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    let spawn = registry
        .get("spawn_subagent")
        .ok_or("spawn missing")?
        .parameters_json_schema();
    let required = spawn["required"].as_array().ok_or("spawn required")?;
    assert!(required.contains(&json!("prompt")));
    assert!(required.contains(&json!("description")));
    assert!(!required.contains(&json!("background")));
    assert!(!required.contains(&json!("subagent_type")));
    assert_eq!(spawn["properties"]["background"]["default"], true);
    for hidden in [
        "task_id",
        "workspace",
        "capability_mode",
        "fork_context",
        "persona",
        "effort",
        "load_skills",
        "session_id",
        "run_in_background",
    ] {
        assert!(spawn["properties"].get(hidden).is_none(), "{hidden}");
    }
    let output = registry
        .get("get_command_or_subagent_output")
        .ok_or("output missing")?
        .parameters_json_schema();
    for hidden in [
        "task_id",
        "full_session",
        "include_thinking",
        "request_ids",
        "block",
        "all",
    ] {
        assert!(output["properties"].get(hidden).is_none(), "{hidden}");
    }
    assert_eq!(output["properties"]["timeout_ms"]["maximum"], 3_600_000);
    assert!(registry.get("background_output").is_none());
    assert!(registry.get("background_cancel").is_none());
    assert!(registry.get("send_subagent_message").is_none());

    let mut configured = harness_tools::coordinator_registry(ShellAllowlist::default());
    let mut settings = SubagentRuntimeConfig::default();
    let mut definitions = SubagentDefinitionSnapshot::default();
    definitions.user.insert(
        "fixture-reviewer".into(),
        SubagentDefinition {
            name: "fixture-reviewer".into(),
            description: "Custom reviewer fixture".into(),
            ..Default::default()
        },
    );
    let catalog = SubagentModelCatalog {
        authority: SubagentCatalogAuthority::Complete,
        models: vec![SubagentCatalogModel {
            id: "fixture-first-party-model".into(),
            family: Some("xai".into()),
            picker_eligible: true,
        }],
    };
    settings.model_inheritance = true;
    harness_tools::register_subagent_tools(
        &mut configured,
        &settings,
        &definitions,
        Some(&catalog),
    );
    let configured_schema = configured
        .get("spawn_subagent")
        .ok_or("configured spawn")?
        .parameters_json_schema();
    assert!(configured_schema["properties"]["subagent_type"]["enum"]
        .as_array()
        .ok_or("agent names missing")?
        .contains(&json!("fixture-reviewer")));
    assert!(configured_schema["properties"].get("model").is_none());
    settings.enabled = false;
    harness_tools::register_subagent_tools(
        &mut configured,
        &settings,
        &definitions,
        Some(&catalog),
    );
    assert!(configured.get("spawn_subagent").is_none());
    assert!(configured.get("task").is_none());
    for retained in [
        "get_command_or_subagent_output",
        "wait_commands_or_subagents",
        "kill_command_or_subagent",
        "get_task_output",
        "wait_tasks",
        "kill_task",
    ] {
        assert!(configured.get(retained).is_some(), "{retained}");
    }

    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::default());
    let (coordinator, actor) = setup(&temp, Arc::<MockProvider>::clone(&provider)).await?;
    for input in [
        json!({"task_id":17}),
        json!({"task_ids":[" unknown ","unknown"," "]}),
        json!({"task_ids":vec!["unknown";21]}),
    ] {
        let result = coordinator
            .execute_agent_tool_call(actor.clone(), None, "get_command_or_subagent_output", input)
            .await?;
        assert!(result.is_error());
        let data = result.structured_json.ok_or("single missing")?;
        assert!(data.get("TaskNotFound").is_some());
        assert!(data.get("MultiResult").is_none());
    }
    let result = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":["second","first","second"]}),
        )
        .await?;
    let data = result.structured_json.ok_or("multi missing")?;
    assert_eq!(data["MultiResult"]["mode"], "poll");
    assert_eq!(data["MultiResult"]["results"][0]["task_id"], "second");
    assert_eq!(data["MultiResult"]["results"][1]["task_id"], "first");
    assert_eq!(data["MultiResult"]["results"][0]["status"], "not_found");
    for (tool, input) in [
        ("get_command_or_subagent_output", json!({})),
        (
            "get_command_or_subagent_output",
            json!({"task_ids":[false]}),
        ),
        (
            "get_command_or_subagent_output",
            json!({"task_ids":(0..21).map(|index| index.to_string()).collect::<Vec<_>>()}),
        ),
        (
            "wait_commands_or_subagents",
            json!({"task_ids":[],"mode":"wait_all"}),
        ),
        (
            "wait_commands_or_subagents",
            json!({"task_ids":vec!["unknown";21],"mode":"wait_all"}),
        ),
        (
            "wait_commands_or_subagents",
            json!({"task_ids":"unknown","mode":"wait_all"}),
        ),
        (
            "wait_commands_or_subagents",
            json!({"task_ids":["unknown"],"mode":"all"}),
        ),
    ] {
        assert!(coordinator
            .execute_agent_tool_call(actor.clone(), None, tool, input)
            .await
            .is_err());
    }
    for mode in ["wait_all", "wait_any"] {
        let result = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "wait_commands_or_subagents",
                json!({"task_ids":["unknown","unknown"],"mode":mode,"timeout_ms":0}),
            )
            .await?;
        let data = result.structured_json.ok_or("wait data")?;
        assert_eq!(data["MultiResult"]["mode"], mode);
        assert_eq!(
            data["MultiResult"]["results"]
                .as_array()
                .ok_or("wait results")?
                .len(),
            2
        );
    }
    let missing = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "kill_command_or_subagent",
            json!({"task_id":"unknown"}),
        )
        .await?;
    assert!(missing.is_error());
    assert!(missing
        .structured_json
        .ok_or("kill missing")?
        .get("TaskNotFound")
        .is_some());
    let failure = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"","description":"","model":"unvalidated-explicit-model"}),
        )
        .await
        .err()
        .ok_or("model validation unexpectedly succeeded")?;
    assert!(
        format!("{failure:?}").contains("validation_unavailable"),
        "native custom error code was lost: {failure:?}"
    );
    assert_eq!(provider.call_count(), 0);
    let alias = registry
        .get("task")
        .ok_or("native task alias missing")?
        .parameters_json_schema();
    assert_eq!(alias["properties"]["run_in_background"]["default"], true);
    assert!(alias["properties"].get("background").is_none());
    for obsolete in ["session_id", "load_skills", "command"] {
        assert!(alias["properties"].get(obsolete).is_none());
    }
    let spawned = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "task",
            json!({"prompt":"native alias child","description":"Alias child","run_in_background":false}),
        )
        .await?
        .structured_json
        .ok_or("alias completion")?;
    let child = spawned["subagent_id"]
        .as_str()
        .ok_or("alias child identity")?;
    let output = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "get_task_output",
            json!({"task_id":child}),
        )
        .await?
        .structured_json
        .ok_or("alias output")?;
    assert_eq!(output["Result"]["task_id"], child);
    assert_eq!(output["Result"]["status"], "completed");
    let waited = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "wait_tasks",
            json!({"task_ids":[child],"mode":"wait_all","timeout_ms":0}),
        )
        .await?
        .structured_json
        .ok_or("alias wait")?;
    assert_eq!(waited["MultiResult"]["results"][0]["task_id"], child);
    let killed = coordinator
        .execute_agent_tool_call(actor, None, "kill_task", json!({"task_id":child}))
        .await?
        .structured_json
        .ok_or("alias kill")?;
    assert_eq!(killed["Result"]["outcome"], "already_exited");
    assert_eq!(provider.call_count(), 1, "aliases cannot rerun the child");
    coordinator.stop_run().await?;
    Ok(())
}
