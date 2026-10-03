use super::*;

#[tokio::test]
async fn unrelated_output_and_wait_queries_preserve_parent_completion_notifications(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::event::EventV1;

    for query in [
        "get_command_or_subagent_output",
        "wait_commands_or_subagents",
    ] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(NotificationGate {
            parent: tokio::sync::Semaphore::new(0),
            child: tokio::sync::Semaphore::new(0),
            parent_started: tokio::sync::Notify::new(),
            child_started: tokio::sync::Notify::new(),
            collect_reports: 0,
            continue_parent: false,
            wakeups: AtomicUsize::new(0),
            collected: tokio::sync::Mutex::new(Vec::new()),
        });
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.tool_registry = Arc::new(harness_tools::coordinator_registry(
            ShellAllowlist::default(),
        ));
        config.permission_policy = PermissionPolicy::allow_all();
        config.provider_model_concurrency = 2;
        let mut parent_profile = AgentProfile::fallback("default");
        parent_profile.toolset = vec![
            "spawn_subagent".into(),
            "get_command_or_subagent_output".into(),
            "wait_commands_or_subagents".into(),
            "kill_command_or_subagent".into(),
        ];
        config
            .agent_profiles
            .insert("default".into(), parent_profile);
        config
            .agent_profiles
            .insert("child".into(), AgentProfile::fallback("child"));
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator
            .start_run("unauthorized completion query", temp.path())
            .await?;
        let parent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let unrelated = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let mut events = coordinator.subscribe_new_events().await?;
        let parent_started = provider.parent_started.notified();
        let child_started = provider.child_started.notified();
        tokio::pin!(parent_started, child_started);
        parent_started.as_mut().enable();
        child_started.as_mut().enable();
        coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                parent.clone(),
                "launch",
            )
            .await?;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(parent_started, child_started)
        })
        .await?;
        let child = provider
            .collected
            .lock()
            .await
            .first()
            .cloned()
            .ok_or("parent did not register child identity")?;
        provider.child.add_permits(1);
        let notification = wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::BackgroundTaskNotification(e)
                    if e.child_session_id.as_str() == child)
            },
            "child completion did not reserve its parent notification",
        )
        .await?;
        let followup = match notification.payload {
            EventV1::BackgroundTaskNotification(event) => event
                .delivered_turn_request_id
                .ok_or("notification did not schedule parent turn")?,
            _ => return Err("unexpected notification event".into()),
        };
        wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::NativeSubagentReceipt(e)
                    if e.child_id == child && e.kind == "terminal_published")
            },
            "child terminal publication did not finish",
        )
        .await?;

        let input = if query == "get_command_or_subagent_output" {
            json!({"task_ids":[child],"timeout_ms":0})
        } else {
            json!({"task_ids":[child],"mode":"wait_any","timeout_ms":0})
        };
        let unauthorized = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            coordinator.execute_agent_tool_call(
                EventActor::new(ActorKind::Worker, Some(unrelated)),
                None,
                query,
                input,
            ),
        )
        .await??;
        let data = unauthorized
            .structured_json
            .as_ref()
            .ok_or("query response")?;
        if query == "get_command_or_subagent_output" {
            assert!(unauthorized.is_error());
            assert!(data.get("TaskNotFound").is_some());
        } else {
            assert_eq!(data["MultiResult"]["results"][0]["status"], "not_found");
        }

        provider.parent.add_permits(1);
        wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::NativeSubagentReceipt(e)
                    if e.child_id == child && e.kind == "completion_reminder_delivered")
            },
            "unrelated query consumed the rightful parent's buffered completion",
        )
        .await?;
        wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::TaskCompleted(e)
                    if e.task_id.as_str() == followup && e.result_summary == "notification delivered")
            },
            "rightful parent did not observe buffered child result",
        )
        .await?;
        assert_eq!(provider.wakeups.load(Ordering::SeqCst), 1);
        coordinator.stop_run().await?;
    }
    Ok(())
}
