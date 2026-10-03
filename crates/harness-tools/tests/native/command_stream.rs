#[tokio::test]
async fn background_shell_streams_owned_redacted_output_and_retains_terminal_status(
) -> Result<(), Box<dyn std::error::Error>> {
    struct CommandClock(FakeClock);
    impl harness_core::clock::Clock for CommandClock {
        fn mono_ms(&self) -> u64 {
            harness_core::clock::Clock::mono_ms(&self.0)
        }

        fn system_time_rfc3339(&self) -> Option<String> {
            Some(format!("1970-01-01T00:00:{:02}Z", self.mono_ms() / 1000))
        }
    }
    use tokio::io::AsyncWriteExt;
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let root = tempfile::tempdir()?;
    let listener = tokio::net::UnixListener::bind(root.path().join("command.sock"))?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_concurrency = 1;
    config.secret_values.push("split-command-credential".into());
    let clock = Arc::new(CommandClock(FakeClock::new()));
    let mut profile = harness_core::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["bash".into(), "read".into()];
    config.agent_profiles.insert("default".into(), profile);
    let handle = spawn_coordinator(
        config,
        Arc::<CommandClock>::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("background commands", root.path()).await?;
    let owner = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(owner.clone()));
    let script = r#"import socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect("command.sock")
sys.stdout.write("ready ")
sys.stdout.flush()
s.recv(1)
sys.stdout.write("split-command-cred")
sys.stdout.flush()
s.recv(1)
sys.stdout.write("ential\n")
sys.stdout.write("captured line\n" * 5000)
sys.stdout.flush()
sys.stderr.write("error stream\n")
sys.exit(7)
"#;
    let output = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "bash",
            json!({"command":format!("python3 -c {}", shell_words::quote(script)), "run_in_background":true}),
        )
        .await?;
    let data = output.structured_json.ok_or("missing command handle")?;
    let id = data["task_id"]
        .as_str()
        .ok_or("missing task ID")?
        .to_owned();
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "7", "native command IDs are UUIDv7");
    assert_eq!(data["task_type"], "bash");
    let mut subscription = handle
        .subscribe_command(actor.clone(), id.clone())
        .await?
        .ok_or("missing admitted command")?;
    let (mut socket, _) =
        tokio::time::timeout(std::time::Duration::from_secs(3), listener.accept()).await??;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        subscription
            .updates
            .wait_for(|snapshot| snapshot.result.output == "ready"),
    )
    .await??;
    clock.0.advance(1250);
    let running = handle
        .subscribe_command(actor.clone(), id.clone())
        .await?
        .ok_or("missing running command")?
        .snapshot;
    assert_eq!(running.result.duration_secs.to_bits(), 1.25_f64.to_bits());
    assert_eq!(running.result.started, "1970-01-01T00:00:00Z");
    assert_eq!(
        handle
            .list_commands(actor.clone())
            .await?
            .into_iter()
            .find(|command| command.result.task_id == id)
            .ok_or("missing listed command")?
            .result
            .duration_secs
            .to_bits(),
        1.25_f64.to_bits()
    );
    // Returning the bash handle releases the one tool slot, not the command.
    let foreground = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        handle.execute_agent_tool_call(
            actor.clone(),
            None,
            "bash",
            json!({"command":"printf foreground"}),
        ),
    )
    .await??;
    assert_eq!(
        foreground.structured_json.ok_or("missing foreground")?["stdout"],
        "foreground"
    );
    let foreign = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let foreign = EventActor::new(ActorKind::Worker, Some(foreign));
    assert!(handle
        .subscribe_command(foreign.clone(), id.clone())
        .await
        .is_err());
    assert!(handle.kill_command(foreign, id.clone()).await.is_err());
    clock.0.advance(750);
    socket.write_all(b"1").await?;
    let partial = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        subscription
            .updates
            .wait_for(|snapshot| snapshot.result.raw_output_bytes >= 24),
    )
    .await??
    .clone();
    assert_eq!(partial.result.duration_secs.to_bits(), 2.0_f64.to_bits());
    assert!(!partial.result.output.contains("split-command"));
    clock.0.advance(1000);
    socket.write_all(b"2").await?;
    let terminal = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        subscription
            .updates
            .wait_for(harness_core::coord::CommandSnapshot::is_terminal),
    )
    .await??
    .clone();
    assert_eq!(terminal.owner_agent_id.as_deref(), Some(owner.as_str()));
    assert_eq!(terminal.result.status, "failed");
    assert_eq!(terminal.result.exit_code, Some(7));
    assert!(terminal.result.ended.is_some());
    assert_eq!(
        terminal.result.ended.as_deref(),
        Some("1970-01-01T00:00:03Z")
    );
    assert!(terminal.finished_mono_ms.is_some());
    assert_eq!(terminal.result.duration_secs.to_bits(), 3.0_f64.to_bits());
    assert!(terminal.result.truncated);
    assert!(terminal.result.output.len() <= 40_000);
    assert!(terminal.result.raw_output_bytes > terminal.result.output.len());
    let captured = std::fs::read_to_string(&terminal.result.output_file)?;
    assert!(captured.contains("[REDACTED]"));
    assert!(captured.contains("captured line"));
    assert!(captured.contains("error stream"));
    assert!(!captured.contains("split-command-cred"));
    let read = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "read",
            json!({"filePath":terminal.result.output_file}),
        )
        .await?;
    assert!(read.display_text.contains("[REDACTED]"));
    assert!(!read.display_text.contains("split-command-cred"));
    assert_eq!(
        handle
            .kill_command(actor.clone(), id.clone())
            .await?
            .ok_or("missing kill result")?
            .outcome,
        "already_exited"
    );
    for command in ["echo $(pwd)", "echo unsafe &", "bash -c 'echo unsafe'"] {
        assert!(handle
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "bash",
                json!({"command":command,"run_in_background":true}),
            )
            .await
            .is_err());
    }
    assert!(handle
        .subscribe_command(actor.clone(), "unknown".into())
        .await?
        .is_none());
    assert!(handle
        .kill_command(actor.clone(), "unknown".into())
        .await?
        .is_none());
    assert_eq!(handle.list_commands(actor).await?.len(), 1);
    handle.stop_run().await?;
    Ok(())
}
