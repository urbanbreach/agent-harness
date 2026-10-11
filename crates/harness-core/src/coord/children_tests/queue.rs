use super::*;

async fn check_crash_recovery(
    config: CoordinatorConfig,
    run: &RunInfo,
    crashed: &[EventEnvelopeV1],
    parent: &str,
    children: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let queued = children.last().ok_or("queued child absent")?;
    let registration = crashed
        .iter()
        .position(|event| {
            matches!(&event.payload,
                EventV1::NativeSubagentRegistered(child) if &child.child_id == queued
            )
        })
        .ok_or("registration absent")?;
    let mut prefixes = vec![crashed[..=registration].to_vec(), crashed.to_vec()];
    let mut check_interrupted_recovery = true;
    while let Some(prefix) = prefixes.pop() {
        let saved = tempfile::tempdir()?;
        let path = saved.path().join(run.run_id.as_str());
        std::fs::create_dir(&path)?;
        let mut bytes = Vec::new();
        for event in &prefix {
            serde_json::to_writer(&mut bytes, event)?;
            bytes.push(b'\n');
        }
        std::fs::write(path.join("events.jsonl"), bytes)?;
        let provider = Arc::new(MockProvider::default());
        let mut resumed = config.clone();
        resumed.session_dir = saved.path().into();
        resumed.provider = Arc::<MockProvider>::clone(&provider);
        for _ in 0..2 {
            let handle = spawn_coordinator(
                resumed.clone(),
                Arc::new(FakeClock::new()),
                Arc::new(DefaultRedactor::default()),
            );
            handle
                .resume_run(run.run_id.as_str(), "recovered queue")
                .await?;
            for child in children {
                let output = join(launch(
                    &handle,
                    parent,
                    "get_command_or_subagent_output",
                    json!({"task_ids":[child], "timeout_ms":10}),
                ))
                .await?;
                assert_eq!(
                    output
                        .structured_json
                        .as_ref()
                        .and_then(|v| v["Result"]["status"].as_str()),
                    Some("failed"),
                    "{output:?}"
                );
                assert!(handle.subagent_history().await?.records[child]
                    .lifecycle
                    .is_finished());
            }
            let history = crate::store::read_events(&handle.run_info().await?.events_path)?;
            if check_interrupted_recovery {
                let admitted = history
                    .iter()
                    .position(|event| {
                        matches!(&event.payload,
                            EventV1::SubagentTransition(t) if t.child_id.0 == *queued
                                && t.transition == SubagentTransitionKind::Spawned
                        )
                    })
                    .ok_or("recovered admission missing")?;
                prefixes.push(history[..=admitted].to_vec());
                check_interrupted_recovery = false;
            }
            for child in children {
                assert_eq!(history.iter().filter(|event| matches!(&event.payload,
                    EventV1::SubagentTransition(t) if t.child_id.0 == *child && t.transition == SubagentTransitionKind::Finished
                )).count(), 1);
            }
            assert_eq!(
                provider.call_count(),
                0,
                "recovery must not rerun child providers"
            );
            handle.stop_run().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn native_registration_queue_demotion_and_foreground_cancel_keep_distinct_scopes(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (requests, mut received) = mpsc::unbounded_channel();
    let provider = Arc::new(Gate {
        requests,
        permits: tokio::sync::Semaphore::new(0),
        responses: MockProvider::script([done("first"), done("second"), done("cancelled")]),
    });
    let mut config = configuration(temp.path(), Arc::<Gate>::clone(&provider));
    config.subagents.max_concurrent = 1;
    let (handle, parent) = start(config.clone(), temp.path()).await?;
    let mut events = handle.subscribe_new_events().await?;
    let first = launch(&handle, &parent, "spawn_subagent", spawn_args(false));
    let first_id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let first_request = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("first provider request absent")?;
    assert_eq!(
        first_request.context.session_id.as_deref(),
        Some(first_id.as_str())
    );
    let second = launch(&handle, &parent, "spawn_subagent", spawn_args(false));
    let second_id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let queued = join(launch(
        &handle,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[second_id]}),
    ))
    .await?;
    assert_eq!(
        queued
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["status"].as_str()),
        Some("initializing")
    );
    let run = handle.run_info().await?;
    let crashed = crate::store::read_events(&run.events_path)?;
    assert_eq!(provider.responses.call_count(), 0);
    let demoted = handle.demote_all_foreground_child_tasks().await?;
    assert_eq!(demoted.len(), 2);
    assert!(demoted
        .iter()
        .all(crate::foreground_demote::DemoteToBackgroundResult::is_demoted));
    let first_handle = join(first).await?;
    let second_handle = join(second).await?;
    assert!(first_handle.display_text.contains(&first_id));
    assert!(second_handle.display_text.contains(&second_id));
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r)
            if r.child_id == first_id && r.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let second_request = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("second provider request absent")?;
    assert_eq!(
        second_request.context.session_id.as_deref(),
        Some(second_id.as_str())
    );
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r)
            if r.child_id == second_id && r.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let mut events = handle.subscribe_new_events().await?;
    let third = launch(&handle, &parent, "spawn_subagent", spawn_args(false));
    let tool = event(&mut events, |event| match &event.payload {
        EventV1::ToolCallRequested(r) if r.tool_id == "spawn_subagent" => {
            Some(r.tool_call_id.to_string())
        }
        _ => None,
    })
    .await?;
    let third_id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await?;
    let mut terminal = handle.subscribe_new_events().await?;
    handle.cancel_task(&tool, "foreground cancelled").await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), third)
            .await??
            .is_err()
    );
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r)
            if r.child_id == third_id && r.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let cancelled = join(launch(
        &handle,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[third_id]}),
    ))
    .await?;
    assert_eq!(
        cancelled
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["status"].as_str()),
        Some("cancelled")
    );
    handle.stop_run().await?;
    check_crash_recovery(config, &run, &crashed, &parent, &[first_id, second_id]).await?;
    Ok(())
}

#[tokio::test]
async fn native_background_waiter_cancel_does_not_cancel_work_or_replay_it(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (requests, mut received) = mpsc::unbounded_channel();
    let provider = Arc::new(Gate {
        requests,
        permits: tokio::sync::Semaphore::new(0),
        responses: MockProvider::script([done("survived")]),
    });
    let config = configuration(temp.path(), Arc::<Gate>::clone(&provider));
    let (handle, parent) = start(config.clone(), temp.path()).await?;
    let run = handle.run_info().await?;
    let mut events = handle.subscribe_new_events().await?;
    let background = launch(&handle, &parent, "spawn_subagent", spawn_args(true));
    let id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let _ = join(background).await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await?;
    let mut events = handle.subscribe_new_events().await?;
    let waiter = launch(
        &handle,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[id],"timeout_ms":60_000}),
    );
    let tool = event(&mut events, |event| match &event.payload {
        EventV1::ToolCallRequested(r) if r.tool_id == "get_command_or_subagent_output" => {
            Some(r.tool_call_id.to_string())
        }
        _ => None,
    })
    .await?;
    handle
        .request_subagent_cancel(
            EventActor::new(ActorKind::System, None),
            SubagentCommandRequest::WaiterCancel { waiter_id: tool },
        )
        .await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await??
            .is_err()
    );
    assert!(matches!(
        handle.raw_finalized_state(id.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r) if r.child_id == id && r.kind == "terminal_published" => {
            Some(())
        }
        _ => None,
    })
    .await?;
    handle.stop_run().await?;
    let bytes = std::fs::read(&run.events_path)?;
    let calls = provider.responses.call_count();
    let restored = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    restored
        .resume_run(run.run_id.to_string(), "native reload")
        .await?;
    let output = join(launch(
        &restored,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[id]}),
    ))
    .await?;
    assert_eq!(
        output
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["status"].as_str()),
        Some("completed")
    );
    assert_eq!(provider.responses.call_count(), calls);
    assert!(matches!(
        restored.raw_finalized_state(id).await?,
        FinalizedStateResult::Available { .. }
    ));
    restored.stop_run().await?;
    assert!(std::fs::read(&run.events_path)?.starts_with(&bytes));
    Ok(())
}
