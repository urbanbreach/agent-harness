use super::*;
use harness_providers::ProviderStreamEvent as Stream;
use serde_json::json;

#[tokio::test]
async fn native_durable_projection_redacts_child_description_and_completed_output(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let secret = "sk-durable-projection-fixture-1234";
    let provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta(format!("Completed with {secret}")),
        Stream::Done { usage: None },
    ]]));
    let (coordinator, actor) = setup(&temp, provider).await?;
    let result = coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "spawn_subagent",
            json!({
                "prompt": "Return the fixture result",
                "description": format!("Inspect {secret}"),
                "background": false
            }),
        )
        .await?
        .structured_json
        .ok_or("foreground child result")?;
    let child = result["subagent_id"].as_str().ok_or("child identity")?;
    let run = coordinator.run_info().await?;
    coordinator.stop_run().await?;
    let directory = run.run_dir.join("subagents").join(child);
    for path in [
        directory.join("meta.json"),
        directory.join("output.json"),
        run.run_dir.join("events.jsonl"),
    ] {
        let bytes = std::fs::read_to_string(&path)?;
        assert!(
            !bytes.contains(secret),
            "secret persisted in {}",
            path.display()
        );
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("meta.json"))?)?;
    assert_eq!(metadata["status"], "completed");
    let output: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("output.json"))?)?;
    assert!(!output["output"]
        .as_str()
        .ok_or("durable completed output")?
        .is_empty());
    Ok(())
}
