use super::*;
use harness_providers::ProviderStreamEvent as Stream;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn background_registration_snapshot_timeout_and_explicit_kill_have_distinct_outcomes(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(Gate {
        inner: MockProvider::default(),
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    let (coordinator, actor) = setup(&temp, Arc::<Gate>::clone(&provider)).await?;
    let started = provider.started.notified();
    tokio::pin!(started);
    started.as_mut().enable();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        coordinator.execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"held child","description":"Hold child"}),
        ),
    )
    .await??;
    tokio::time::timeout(Duration::from_secs(3), started).await?;
    let history = coordinator.subagent_history().await?;
    assert_eq!(history.records.len(), 1);
    let child = history
        .records
        .keys()
        .next()
        .ok_or("registered child")?
        .clone();
    assert!(result.display_text.contains(&child));
    assert_eq!(
        provider.inner.call_count(),
        0,
        "registration is not completion"
    );
    let rejected = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"task_id":child,"prompt":"duplicate child","description":"Duplicate identity"}),
        )
        .await
        .err()
        .ok_or("duplicate spawn unexpectedly succeeded")?;
    assert!(
        format!("{rejected:?}").contains("spawn_rejected"),
        "eager logical rejection lost its native custom code: {rejected:?}"
    );
    for timeout in [0, 1] {
        let result = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "get_command_or_subagent_output",
                json!({"task_ids":[child],"timeout_ms":timeout}),
            )
            .await?;
        let data = result.structured_json.ok_or("live result")?;
        assert_eq!(data["Result"]["status"], "running");
        assert!(data["Result"]["exit_code"].is_null());
        assert!(data["Result"]["ended"].is_null());
        assert_eq!(data["Result"]["output_file"], "");
        assert_eq!(data["Result"]["truncated"], false);
    }
    let killed = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "kill_command_or_subagent",
            json!({"task_id":child}),
        )
        .await?;
    assert_eq!(
        killed.structured_json.ok_or("kill result")?["Result"]["outcome"],
        "killed"
    );
    let finished = tokio::time::timeout(
        Duration::from_secs(3),
        coordinator.execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[child],"timeout_ms":3000}),
        ),
    )
    .await??;
    assert_eq!(
        finished.structured_json.ok_or("cancelled result")?["Result"]["status"],
        "cancelled"
    );
    let repeated = coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "kill_command_or_subagent",
            json!({"task_id":child}),
        )
        .await?;
    assert_eq!(
        repeated.structured_json.ok_or("repeat kill")?["Result"]["outcome"],
        "already_exited"
    );
    coordinator.stop_run().await?;
    assert_eq!(
        provider.inner.call_count(),
        0,
        "cancelled bootstrap must not execute inference"
    );
    Ok(())
}
