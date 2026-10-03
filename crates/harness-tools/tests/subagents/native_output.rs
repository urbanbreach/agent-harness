use super::*;
use harness_providers::ProviderStreamEvent as Stream;
use serde_json::json;

#[cfg(unix)]
#[tokio::test]
async fn mixed_native_command_and_subagent_queries_retain_command_output_and_order(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("child report".into()),
        Stream::Done { usage: None },
    ]]));
    let (coordinator, actor) = setup(&temp, provider).await?;
    let child = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"child work","description":"Child report","background":false}),
        )
        .await?
        .structured_json
        .ok_or("child output")?["subagent_id"]
        .as_str()
        .ok_or("child id")?
        .to_owned();
    let command = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "bash",
            json!({"command":"printf 'native command output'","block_until_ms":0}),
        )
        .await?
        .structured_json
        .ok_or("command output")?["task_id"]
        .as_str()
        .ok_or("command id")?
        .to_owned();
    let output = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[command,child],"timeout_ms":3000}),
        )
        .await?
        .structured_json
        .ok_or("mixed output")?;
    let results = output["MultiResult"]["results"]
        .as_array()
        .ok_or("mixed results")?;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["task_id"], command);
    assert_eq!(results[0]["command"], "printf 'native command output'");
    assert_eq!(results[0]["status"], "completed");
    assert_eq!(results[0]["exit_code"], 0);
    assert_eq!(results[0]["output"], "native command output");
    assert!(!results[0]["output_file"]
        .as_str()
        .ok_or("command output path")?
        .is_empty());
    assert_eq!(
        results[0]["raw_output_bytes"],
        "native command output".len()
    );
    assert_eq!(results[1]["task_id"], child);
    assert_eq!(results[1]["status"], "completed");
    assert_eq!(results[1]["output_file"], "");
    let killed = coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "kill_command_or_subagent",
            json!({"task_id":command}),
        )
        .await?
        .structured_json
        .ok_or("command kill")?;
    assert_eq!(killed["Result"]["outcome"], "already_exited");
    coordinator.stop_run().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn native_output_bounds_large_command_display_without_losing_total_or_raw_artifact(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::default());
    let (coordinator, actor) = setup(&temp, provider).await?;
    let raw = "x".repeat(80_000);
    let command = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "bash",
            json!({"command":format!("printf '{raw}'"),"block_until_ms":0}),
        )
        .await?
        .structured_json
        .ok_or("command handle")?["task_id"]
        .as_str()
        .ok_or("command id")?
        .to_owned();
    let result = coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[command],"timeout_ms":3000}),
        )
        .await?;
    let data = result.structured_json.ok_or("bounded command output")?;
    let data = &data["Result"];
    assert_eq!(data["status"], "completed");
    assert_eq!(data["raw_output_bytes"], raw.len());
    assert_eq!(data["truncated"], true);
    assert!(data["output"].as_str().ok_or("display body")?.len() < raw.len());
    let hint = data["truncation_hint"].as_str().ok_or("truncation hint")?;
    assert!(!hint.is_empty());
    assert!(result.display_text.contains(hint));
    let output_file = data["output_file"].as_str().ok_or("raw output file")?;
    assert_eq!(std::fs::read_to_string(output_file)?, raw);
    coordinator.stop_run().await?;
    Ok(())
}
