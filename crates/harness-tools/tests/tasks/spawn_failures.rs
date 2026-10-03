use super::*;

#[tokio::test]
async fn spawn_decode_and_foreground_failures_never_become_successful_partial_reports(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("partial report not success".into()),
        Stream::error("deterministic child failure"),
    ]]));
    let coordinator = spawn_coordinator(
        config(temp.path(), Arc::<MockProvider>::clone(&provider)),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("spawn errors", temp.path()).await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    for input in [
        json!({"prompt":"missing description"}),
        json!({"prompt":"bad boolean","description":"","background":"invalid"}),
        json!({"prompt":"","description":"","task_id":"550e8400-e29b-41d4-a716-446655440000"}),
        json!({"prompt":"","description":"","subagent_type":"not-a-defined-type"}),
    ] {
        assert!(coordinator
            .execute_agent_tool_call(actor.clone(), None, "spawn_subagent", input)
            .await
            .is_err());
    }
    assert_eq!(provider.call_count(), 0);
    assert!(coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "spawn_subagent",
            json!({"prompt":"","description":"","background":false}),
        )
        .await
        .is_err());
    assert_eq!(provider.call_count(), 1);
    coordinator.stop_run().await?;
    Ok(())
}
