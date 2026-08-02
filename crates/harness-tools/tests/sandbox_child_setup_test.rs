#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use harness_core::sandbox::{
    build_sandbox_child_plan, detect_landlock, evaluate_network_confinement_with_landlock,
    NetworkConfinementStatus, SandboxNetworkPolicy, SandboxPathRoots, SandboxPlatform,
    SandboxPolicy,
};
use harness_providers::UnwrapOrAbort;
use rustix::io::{fcntl_setfd, FdFlags};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SetupFrame {
    Ready,
    Error { code: String, message: String },
}

#[test]
fn sandbox_child_setup_static_gate_has_no_pre_exec_or_landlock_call_target() {
    // Given: the parent shell-spawn source
    let shell_run = include_str!("../src/shell_run.rs");
    let helper = include_str!("../src/shell_run/sandbox_helper.rs");

    // When: its child-setup path is inspected

    // Then: allocation-capable Landlock setup is reachable only through the exec helper.
    assert!(!shell_run.contains(".pre_exec("));
    assert!(!shell_run.contains("CommandExt"));
    assert!(!shell_run.contains("apply_landlock_sandbox_plan"));
    assert!(helper.contains("harness-sandbox-helper"));
}

#[test]
fn sandbox_helper_reports_typed_invalid_request_before_ready() {
    // Given: a helper with an inherited control socket
    let (mut parent, child) = UnixStream::pair().expect("control socket");
    fcntl_setfd(&child, FdFlags::empty()).expect("make control socket inheritable");
    let mut command = helper_command(&child, "/bin/true", Vec::new());

    // When: the parent sends malformed setup JSON
    let process = command.spawn().expect("spawn helper");
    drop(child);
    parent.write_all(b"not-json\n").expect("write request");
    parent
        .shutdown(Shutdown::Write)
        .expect("close request stream");
    let frame = read_frame(&mut parent);
    let output = process.wait_with_output().expect("helper exit");

    // Then: the parent-observable protocol preserves the typed setup failure.
    match frame {
        SetupFrame::Error { code, message } => {
            assert_eq!(code, "invalid_request");
            assert!(!message.is_empty());
        }
        SetupFrame::Ready => panic!("malformed request must not reach READY"),
    }
    assert!(!output.status.success());
}

#[test]
fn sandbox_helper_denies_network_after_ready_when_landlock_is_available() {
    // Given: a real deny-network child plan and loopback TCP listener
    let landlock = detect_landlock();
    if !matches!(
        evaluate_network_confinement_with_landlock(
            &SandboxNetworkPolicy::DenyAll,
            SandboxPlatform::Linux,
            &landlock,
        ),
        NetworkConfinementStatus::Available { .. }
    ) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    let state = workspace.join(".agent-harness");
    let working = temp.path().join("working");
    std::fs::create_dir_all(&state).expect("state directory");
    std::fs::create_dir_all(&working).expect("working directory");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let port = listener.local_addr().expect("listener address").port();
    let plan = build_sandbox_child_plan(
        SandboxPolicy::WorkspaceWrite,
        &SandboxPathRoots {
            workspace_root: workspace,
            harness_state_dir: state,
            temp_dir: working,
        },
        SandboxNetworkPolicy::DenyAll,
    )
    .expect("sandbox plan");
    let (mut parent, child) = UnixStream::pair().expect("control socket");
    fcntl_setfd(&child, FdFlags::empty()).expect("make control socket inheritable");
    let mut command = helper_command(
        &child,
        "/bin/bash",
        vec![
            "-c".to_string(),
            format!(
                "if : >/dev/tcp/127.0.0.1/{port}; then printf connected; else printf denied; fi"
            ),
        ],
    );

    // When: the helper closes descriptors, installs restrictions, and starts the child.
    let process = command.spawn().expect("spawn helper");
    drop(child);
    serde_json::to_writer(&mut parent, &serde_json::json!({ "plan": plan })).expect("write plan");
    parent.write_all(b"\n").expect("terminate plan");
    parent
        .shutdown(Shutdown::Write)
        .expect("close request stream");
    let raw_frame = read_raw_frame(&mut parent);
    let output = process.wait_with_output().expect("child result");

    // Then: READY precedes execution and the real loopback connection is denied.
    assert!(
        !raw_frame.is_empty(),
        "helper closed control before READY; status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let frame: SetupFrame = serde_json::from_str(&raw_frame).expect("valid READY frame");
    assert!(matches!(frame, SetupFrame::Ready));
    assert_ne!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "connected",
        "deny-all policy must not allow the loopback TCP connection"
    );
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

fn helper_command(control: &UnixStream, program: &str, args: Vec<String>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harness-sandbox-helper"));
    command
        .arg("--control-fd")
        .arg(control.as_raw_fd().to_string())
        .arg("--")
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn read_frame(control: &mut UnixStream) -> SetupFrame {
    let line = read_raw_frame(control);
    serde_json::from_str(&line).unwrap_or_abort()
}

fn read_raw_frame(control: &mut UnixStream) -> String {
    let mut line = String::new();
    BufReader::new(control)
        .read_line(&mut line)
        .unwrap_or_abort();
    line
}
