use super::*;
use harness_core::event::RuntimeReminderKind;

/// Waits longer than `wait_for`: delivery starts after a two-second collection window.
async fn next_monitor_event(
    events: &mut harness_core::store::EventStream,
) -> Result<String, Box<dyn std::error::Error>> {
    let event = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = events.next().await {
            if let EventV1::RuntimeReminder(reminder) = event?.payload
                && reminder.kind == RuntimeReminderKind::MonitorEvent
            {
                return Ok(reminder.text);
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "events ended",
        ))
    })
    .await
    .map_err(|_| "no monitor event")??;
    Ok(event)
}

#[tokio::test]
async fn monitors_wake_an_idle_agent_with_matching_lines_and_their_exit(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::default());
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config.behavior.command_notifications.enabled = true;
    config.behavior.command_notifications.wake_idle = true;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["monitor".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("monitor", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent));
    let mut events = coordinator.subscribe_new_events().await?;
    let monitor = |args: serde_json::Value| {
        coordinator.execute_agent_tool_call(actor.clone(), None, "monitor", args)
    };

    // Lines arrive while the command keeps running; the unfinished last line is no event.
    let started = monitor(json!({"description": "build", "filter": "OK|error",
        "command": "printf 'noise\\n\\033[32mBUILD OK\\033[0m\\nerror: one\\r\\n'; printf partial; sleep 30"}))
    .await?;
    assert!(!started.is_error(), "{}", started.display_text);
    let text = next_monitor_event(&mut events).await?;
    assert!(
        text.contains("Monitor event(build)") && text.contains("BUILD OK\nerror: one\n"),
        "{text}"
    );
    assert!(
        !text.contains("noise") && !text.contains("partial") && !text.contains("completed"),
        "{text}"
    );
    let id = started.structured_json.ok_or("missing monitor result")?["task_id"]
        .as_str()
        .ok_or("missing task id")?
        .to_owned();
    coordinator.kill_command(actor.clone(), id).await?;

    let path = temp.path().join("ready.txt");
    let watched = monitor(json!({"description": "ready file", "path": path})).await?;
    assert!(!watched.is_error(), "{}", watched.display_text);
    std::fs::write(&path, "done")?;
    let text = next_monitor_event(&mut events).await?;
    assert!(
        text.contains(&format!("create {}\nmonitor completed", path.display())),
        "{text}"
    );

    for (args, error) in [
        (
            json!({"description": "x", "command": "true", "path": "a"}),
            "not both",
        ),
        (
            json!({"description": "x", "path": "a", "filter": "y"}),
            "only to command",
        ),
        (
            json!({"description": "x", "command": "true", "filter": "("}),
            "regular expression",
        ),
        (
            json!({"description": "x", "path": "missing", "event": "modify"}),
            "existing",
        ),
    ] {
        let failed = monitor(args).await;
        let message = failed.map_or_else(|error| error, |result| result.display_text);
        assert!(message.contains(error), "{message}");
    }
    let rearm = monitor(json!({"action": "rearm"})).await?;
    assert_eq!(rearm.display_text, "No paused monitor to resume.");
    Ok(())
}
