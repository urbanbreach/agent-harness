#[tokio::test]
async fn rustfmt_stays_on_the_target_and_cancelled_formatters_leave_it_unchanged(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real formatter checks".into());
    }
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let child_source = "pub fn child()->u32{7}\n";
    std::fs::write(workspace.join("child.rs"), child_source)?;
    let listener = tokio::net::UnixListener::bind(workspace.join("ready.sock"))?;
    let script = r#"import os, signal, socket, subprocess, sys, time
child = subprocess.Popen(["sleep", "60"])
def stop(*unused):
    child.wait()
    sys.exit(0)
signal.signal(signal.SIGTERM, stop)
sock = socket.socket(socket.AF_UNIX)
sock.connect("ready.sock")
sock.sendall(str(child.pid).encode())
sock.close()
time.sleep(60)
"#;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.formatter = Arc::new(serde_json::from_value(json!({"fixture":{
        "command":["python3","-c",script],"extensions":["demo"]
    }}))?);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = coordinator.start_run("formatters", &workspace).await?;
    let result = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "write",
            json!({"path":"lib.rs","content":"mod child;\npub fn answer()->u32{42}\n"}),
        )
        .await?;
    assert!(result.structured_json.ok_or("missing result")?["format_warning"].is_null());
    assert_eq!(
        std::fs::read_to_string(workspace.join("lib.rs"))?,
        "mod child;\npub fn answer() -> u32 {\n    42\n}\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("child.rs"))?,
        child_source
    );
    let task = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "write",
            json!({"path":"cancel.demo","content":"must not be committed"}),
        )
        .await?;
    let pid = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let (mut socket, _) = listener.accept().await?;
        let mut pid = String::new();
        socket.read_to_string(&mut pid).await?;
        Ok::<_, std::io::Error>(pid)
    })
    .await??;
    coordinator.cancel_task(task, "cancel formatter").await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run()).await??;
    assert_eq!(
        rustix::process::test_kill_process(
            rustix::process::Pid::from_raw(pid.parse()?).ok_or("invalid PID")?
        ),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(!workspace.join("cancel.demo").exists());
    assert!(
        !std::fs::read_dir(&workspace)?.any(|entry| entry.is_ok_and(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with(".harness-format-")))
    );
    assert_eq!(
        harness_core::store::read_events(&info.events_path)?
            .iter()
            .filter(|e| matches!(e.payload, EventV1::EditApplied(_)))
            .count(),
        1
    );
    Ok(())
}
