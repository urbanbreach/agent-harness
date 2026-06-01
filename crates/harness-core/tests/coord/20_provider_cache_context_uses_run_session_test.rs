#[tokio::test]
async fn provider_prompt_cache_context_uses_run_id_not_reused_agent_id() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = CapturingProvider::new(vec!["first answer", "second answer"]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let first_run = coordinator
        .start_run("first cache-key run", PathBuf::from("/workspace/project"))
        .await
        .expect("start first run");
    let first_agent = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn first agent");
    assert_eq!(first_agent, "agent_000001");
    let first_request = coordinator
        .request_agent_turn(supervisor_actor(), first_agent.clone(), "first prompt")
        .await
        .expect("request first turn");
    wait_for_events(&first_run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request.as_str())
                        && data.result_summary == "first answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop first run");

    let second_run = coordinator
        .start_run("second cache-key run", PathBuf::from("/workspace/project"))
        .await
        .expect("start second run");
    let second_agent = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn second agent");
    assert_eq!(second_agent, "agent_000001");
    let second_request = coordinator
        .request_agent_turn(supervisor_actor(), second_agent, "second prompt")
        .await
        .expect("request second turn");
    wait_for_events(&second_run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(second_request.as_str())
                        && data.result_summary == "second answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop second run");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2, "expected one provider request per run");
    assert_eq!(
        requests[0].context.session_id.as_deref(),
        Some(first_run.run_id.as_str())
    );
    assert_eq!(
        requests[1].context.session_id.as_deref(),
        Some(second_run.run_id.as_str())
    );
    assert_ne!(
        requests[0].context.session_id,
        requests[1].context.session_id,
        "prompt cache keys must not collide when agent ids repeat across sessions"
    );
    assert_ne!(
        requests[0].context.session_id.as_deref(),
        Some(first_agent.as_str()),
        "provider cache context must not use the reused agent id"
    );
}
