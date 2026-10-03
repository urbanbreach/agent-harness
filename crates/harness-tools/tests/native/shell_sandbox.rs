#[cfg(target_os = "linux")]
#[test]
fn shell_filters_environment_and_enforces_filesystem_policy_only_in_children(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for native shell confinement checks".into());
    }
    if std::env::var("HARNESS_SHELL_ENV_PROBE").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "shell_filters_environment_and_enforces_filesystem_policy_only_in_children",
                "--nocapture",
            ])
            .env("HARNESS_SHELL_ENV_PROBE", "1")
            .env("GITHUB_TOKEN", "synthetic-shell-environment-secret")
            .env("PYTHONSTARTUP", "/must-not-be-inherited")
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("workspace");
        std::fs::create_dir(&root)?;
        let outside = temp.path().join("outside");
        std::fs::write(&outside, "private")?;
        for policy in ["off", "workspace_write", "read_only", "strict", "unknown"] {
            std::fs::write(root.join("source"), "original")?;
            let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
            harness_tools::register_shell_tool(&mut registry, ShellAllowlist::default(), &|key| {
                (key == "HARNESS_OS_SANDBOX_POLICY").then(|| policy.to_owned())
            });
            let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
            config.tool_registry = Arc::new(registry);
            config.permission_policy = PermissionPolicy::allow_all();
            let handle = spawn_coordinator(config, Arc::new(FakeClock::new()), Arc::new(DefaultRedactor::default()));
            handle.start_run(policy, &root).await?;
            let script = format!(r#"import json, os, pathlib, tempfile
assert 'GITHUB_TOKEN' not in os.environ and 'PYTHONSTARTUP' not in os.environ
assert pathlib.Path('source').read_text() == 'original'
try:
    pathlib.Path('/proc/{}/environ').read_bytes()
    raise AssertionError('parent environment is readable')
except PermissionError: pass
def permitted(operation):
    try: operation(); return True
    except PermissionError: return False
written = permitted(lambda: pathlib.Path('source').write_text('changed'))
outside = permitted(lambda: pathlib.Path({}).read_text())
with tempfile.TemporaryFile() as file: file.write(b'scratch')
print(json.dumps(dict(written=written, outside=outside, scratch=tempfile.gettempdir())))
"#, std::process::id(), serde_json::to_string(&outside.to_string_lossy())?);
            let result = handle.execute_agent_tool_call(EventActor::new(ActorKind::User, None), None, "bash", json!({"command":format!("/usr/bin/python3 -c {}", shell_words::quote(&script))})).await;
            handle.stop_run().await?;
            if policy == "unknown" {
                assert!(result.is_err_and(|e| e.contains("sandbox policy")));
                assert_eq!(std::fs::read_to_string(root.join("source"))?, "original");
                continue;
            }
            let result = result?;
            assert!(!result.is_error(), "{policy}: {}", result.display_text);
            let metadata = result.structured_json.ok_or("missing shell output")?;
            let report: serde_json::Value = serde_json::from_str(metadata["stdout"].as_str().ok_or("missing stdout")?)?;
            assert_eq!(report["written"], matches!(policy, "off" | "workspace_write"));
            assert_eq!(report["outside"], policy == "off");
            if policy != "off" {
                assert!(!std::path::Path::new(report["scratch"].as_str().ok_or("missing scratch path")?).exists());
            }
            std::fs::write(&outside, "parent remains unrestricted")?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[tokio::test]
async fn cancelling_shell_reaps_its_child_and_terminates_the_process_group(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root)?;
    let listener = tokio::net::UnixListener::bind(root.join("ready.sock"))?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        harness_core::perm::PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: harness_core::perm::PermissionAction::Allow,
        },
        harness_core::perm::PermissionRule {
            permission: "bash".into(),
            pattern: "*".into(),
            action: harness_core::perm::PermissionAction::Ask,
        },
    ])?;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = coordinator.start_run("processes", &root).await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let worker = coordinator.clone();
    let failed = tokio::spawn(async move {
        worker
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "bash",
                json!({"command":"printf out; printf err >&2; exit 7"}),
            )
            .await
    });
    let permission = ast_permission(&mut events).await?;
    coordinator
        .resolve_permission_with_grant_scope(
            permission.permission_id,
            harness_core::perm::PermissionDecision::Allow,
            None,
            Some(harness_core::perm::PermissionGrantScope::Run),
        )
        .await?;
    let failed = failed.await??;
    assert!(failed.display_text.contains("outerr"));
    assert_eq!(
        failed
            .structured_json
            .as_ref()
            .and_then(|v| v.get("exit_code")),
        Some(&json!(7))
    );
    assert!(harness_core::store::read_events(&info.events_path)?.iter().any(|e| {
        matches!(&e.payload, EventV1::ToolCallFinished(result) if result.status == ToolCallStatus::Failed)
    }), "a nonzero exit must be recorded as a failed tool call");
    let repeated = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        coordinator.execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "bash",
            json!({"command":"printf again"}),
        ),
    )
    .await??;
    assert_eq!(
        repeated
            .structured_json
            .as_ref()
            .and_then(|v| v["stdout"].as_str()),
        Some("again")
    );
    // The parent reaps its child on TERM but does not signal it: killing only the parent cannot pass.
    let script = r#"import os, signal, socket, subprocess, sys, time
child = subprocess.Popen(["sleep", "60"])
def terminate(*unused):
    child.wait()
    sys.exit(0)
signal.signal(signal.SIGTERM, terminate)
sock = socket.socket(socket.AF_UNIX)
sock.connect("ready.sock")
sock.sendall(str(child.pid).encode())
sock.close()
time.sleep(60)
"#;
    let command = format!("printf ready; python3 -c {}", shell_words::quote(script));
    let task = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "bash",
            json!({"command":command}),
        )
        .await?;
    let permission = ast_permission(&mut events).await?;
    coordinator
        .resolve_permission(
            permission.permission_id,
            harness_core::perm::PermissionDecision::Allow,
            None,
        )
        .await?;
    let pid = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let (mut socket, _) = listener.accept().await?;
        let mut pid = String::new();
        socket.read_to_string(&mut pid).await?;
        Ok::<_, std::io::Error>(pid)
    })
    .await??;
    let pid = rustix::process::Pid::from_raw(pid.parse()?).ok_or("invalid child PID")?;
    assert!(rustix::process::test_kill_process(pid).is_ok());
    coordinator.cancel_task(task, "test cancellation").await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run()).await??;
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    Ok(())
}

#[tokio::test]
async fn background_shell_kill_and_root_shutdown_reap_owned_process_groups(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    for cleanup in ["kill", "stop", "owner_drop"] {
        let root = tempfile::tempdir()?;
        let listener = tokio::net::UnixListener::bind(root.path().join("ready.sock"))?;
        let mut config = CoordinatorConfig::new(root.path().join("sessions"));
        config.tool_registry = Arc::new(harness_tools::coordinator_registry(
            ShellAllowlist::default(),
        ));
        config.permission_policy = PermissionPolicy::allow_all();
        let handle = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        handle
            .start_run("background group cleanup", root.path())
            .await?;
        let actor = EventActor::new(ActorKind::User, None);
        let script = r#"import signal, socket, subprocess, sys
child = subprocess.Popen([sys.executable, "-c", "import signal; signal.pause()"])
def stop(*unused):
    child.wait()
    sys.exit(0)
signal.signal(signal.SIGTERM, stop)
sock = socket.socket(socket.AF_UNIX)
sock.connect("ready.sock")
sock.sendall(str(child.pid).encode())
sock.shutdown(socket.SHUT_WR)
sock.recv(1)
"#;
        let output = handle
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "bash",
                json!({"command":format!("python3 -c {}", shell_words::quote(script)), "is_background":"yes", "block_until_ms":"0"}),
            )
            .await?;
        let id = output.structured_json.ok_or("missing handle")?["task_id"]
            .as_str()
            .ok_or("missing task ID")?
            .to_owned();
        let mut subscription = handle
            .subscribe_command(actor.clone(), id.clone())
            .await?
            .ok_or("missing command")?;
        let pid = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let (mut socket, _) = listener.accept().await?;
            let mut pid = String::new();
            socket.read_to_string(&mut pid).await?;
            Ok::<_, std::io::Error>((pid, socket))
        })
        .await??;
        let _socket = pid.1;
        let pid = rustix::process::Pid::from_raw(pid.0.parse()?).ok_or("invalid descendant PID")?;
        assert!(rustix::process::test_kill_process(pid).is_ok());
        if cleanup == "kill" {
            assert_eq!(
                handle
                    .kill_command(actor.clone(), id.clone())
                    .await?
                    .ok_or("missing kill")?
                    .outcome,
                "killed"
            );
            let terminal = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                subscription
                    .updates
                    .wait_for(harness_core::coord::CommandSnapshot::is_terminal),
            )
            .await??
            .clone();
            assert_eq!(terminal.result.status, "cancelled");
        }
        if cleanup == "owner_drop" {
            drop(handle);
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                subscription
                    .updates
                    .wait_for(harness_core::coord::CommandSnapshot::is_terminal),
            )
            .await??;
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(3), handle.stop_run()).await??;
        }
        assert_eq!(
            rustix::process::test_kill_process(pid),
            Err(rustix::io::Errno::SRCH)
        );
    }
    Ok(())
}
