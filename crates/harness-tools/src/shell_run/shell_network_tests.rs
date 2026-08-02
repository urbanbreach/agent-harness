//! Integration tests for child-only TCP network confinement via Landlock.
//!
//! These tests bind real loopback listeners because the feature under test is
//! OS-level network enforcement; the listeners are never exposed externally.

use super::{ShellCommandInvocation, ShellCommandRunner, TokioShellCommandRunner};
use crate::UnwrapOrAbort;
use harness_core::sandbox::{
    build_sandbox_child_plan, detect_landlock, evaluate_network_confinement_with_landlock,
    NetworkConfinementStatus, SandboxNetworkPolicy, SandboxPathRoots, SandboxPlatform,
    SandboxPolicy,
};
use std::collections::BTreeSet;
use std::net::TcpListener;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn shell_child_network_policy_denies_unlisted_tcp_port_and_allows_listed_port() {
    // arrange
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

    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("workspace");
    let state = workspace.join(".agent-harness");
    let working_directory = temp.path().join("working");
    std::fs::create_dir_all(&state).unwrap_or_abort();
    std::fs::create_dir_all(&working_directory).unwrap_or_abort();
    let roots = SandboxPathRoots {
        workspace_root: workspace,
        harness_state_dir: state,
        temp_dir: working_directory.clone(),
    };
    let denied_listener = TcpListener::bind("127.0.0.1:0").unwrap_or_abort();
    let allowed_listener = TcpListener::bind("127.0.0.1:0").unwrap_or_abort();
    let denied_port = denied_listener.local_addr().unwrap_or_abort().port();
    let allowed_port = allowed_listener.local_addr().unwrap_or_abort().port();
    let denied_plan = build_sandbox_child_plan(
        SandboxPolicy::WorkspaceWrite,
        &roots,
        SandboxNetworkPolicy::DenyAll,
    )
    .unwrap_or_abort();
    let allowed_plan = build_sandbox_child_plan(
        SandboxPolicy::WorkspaceWrite,
        &roots,
        SandboxNetworkPolicy::AllowTcpPorts {
            allowed_ports: BTreeSet::from([allowed_port]),
        },
    )
    .unwrap_or_abort();
    let runner = TokioShellCommandRunner;

    // act
    let denied = runner
        .run(
            ShellCommandInvocation::new("/bin/bash", working_directory.clone())
                .args(vec![
                    "-c".to_string(),
                    shell_tcp_connect_command(denied_port),
                ])
                .with_sandbox_plan(Some(denied_plan)),
            5_000,
        )
        .await
        .unwrap_or_abort();
    let allowed = runner
        .run(
            ShellCommandInvocation::new("/bin/bash", working_directory)
                .args(vec![
                    "-c".to_string(),
                    shell_tcp_connect_command(allowed_port),
                ])
                .with_sandbox_plan(Some(allowed_plan)),
            5_000,
        )
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(denied.stdout, "denied");
    assert_eq!(allowed.stdout, "connected");
    let _connection = allowed_listener.accept().unwrap_or_abort();
}

#[cfg(target_os = "linux")]
fn shell_tcp_connect_command(port: u16) -> String {
    format!("if : >/dev/tcp/127.0.0.1/{port}; then printf connected; else printf denied; fi")
}
