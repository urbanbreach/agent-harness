use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{McpConfig, PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};

#[tokio::test]
async fn native_write_routes_through_hashline_and_emits_edit_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("demo.txt"), "alpha\nbeta\n").expect("seed file");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "write".to_string()],
    );

    let run = handle
        .start_run("native_write_routes", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "demo.txt").await;

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "demo.txt",
                "content": "alpha\nBETA\n",
            }),
        )
        .await
        .expect("request tool call");

    wait_for_tool_call_finished(&run.events_path, &tool_call_id, Duration::from_secs(2)).await;
    handle.stop_run().await.expect("stop run");

    assert_eq!(
        fs::read_to_string(workspace.join("demo.txt")).expect("read updated file"),
        "alpha\nBETA\n"
    );

    let edit_id = format!("fs-write-{tool_call_id}");
    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditProposed(data)
                if data.edit_id == edit_id
                    && data.path == "demo.txt"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data)
                if data.edit_id == edit_id
                    && data.path == "demo.txt"
                    && data.diff_rel_path.as_deref().is_some_and(|path| path.ends_with(".diff"))
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn native_write_requires_prior_read_for_existing_files() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("demo.txt"), "alpha\nbeta\n").expect("seed file");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["write".to_string()],
    );

    let _run = handle
        .start_run("native_write_requires_read", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "demo.txt",
                "content": "alpha\nBETA\n",
            }),
        )
        .await
        .expect_err("write without prior read should fail");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("You must read file"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("read(hashlineAnchors=true)") || error.contains("edit.hashline_scan"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn native_write_rejects_stale_reads_after_external_modification() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("demo.txt"), "alpha\nbeta\n").expect("seed file");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "write".to_string()],
    );

    let _run = handle
        .start_run("native_write_stale_read", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "demo.txt").await;
    fs::write(workspace.join("demo.txt"), "alpha\nBETA\n").expect("external mutation");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "demo.txt",
                "content": "alpha\nOMEGA\n",
            }),
        )
        .await
        .expect_err("stale read should fail write");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("modified since it was last read"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("Please read the file again"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("read(hashlineAnchors=true)") || error.contains("edit.hashline_scan"),
        "unexpected error: {error}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_write_rejects_symlink_parent_escape() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let outside = temp_dir.path().join("outside");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&outside).expect("outside");
    symlink(&outside, workspace.join("escape-link")).expect("symlink outside dir into workspace");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["write".to_string()],
    );

    let _run = handle
        .start_run("native_write_symlink_escape", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "escape-link/blocked.txt",
                "content": "blocked\n",
            }),
        )
        .await
        .expect_err("symlink escape should fail write");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("path escapes workspace root"),
        "unexpected error: {error}"
    );
    assert!(
        !outside.join("blocked.txt").exists(),
        "write must not create files outside the workspace"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_edit_rejects_symlink_file_escape() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let outside = temp_dir.path().join("outside");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(outside.join("secret.txt"), "outside\n").expect("seed outside file");
    symlink(
        outside.join("secret.txt"),
        workspace.join("escape-file.txt"),
    )
    .expect("symlink outside file into workspace");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_symlink_escape", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "escape-file.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", harness_core::edit::hashline::compute_line_hash("outside")),
                        "lines": ["inside"],
                    }
                ],
            }),
        )
        .await
        .expect_err("symlink escape should fail edit");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("path escapes workspace root"),
        "unexpected error: {error}"
    );
    assert_eq!(
        fs::read_to_string(outside.join("secret.txt")).expect("read outside file"),
        "outside\n"
    );
}

fn test_coordinator(
    session_dir: &Path,
    permission_policy: PermissionPolicy,
    toolset: Vec<String>,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.permission_policy = permission_policy;
    config.tool_registry = Arc::new(coordinator_registry_with_mcp_and_editing(
        ShellAllowlist::default(),
        McpConfig::default(),
        EditingToolSurfaceConfig {
            hashline_edit: true,
        },
    ));
    config.agent_profiles.insert(
        "worker".to_string(),
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "worker-prompt".to_string(),
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset,
        },
    );

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

fn read_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event"))
        .collect()
}

async fn wait_for_tool_call_finished(events_path: &Path, tool_call_id: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if events_path.exists()
            && read_events(events_path).iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(data)
                        if data.tool_call_id == tool_call_id
                            && data.status == ToolCallStatus::Succeeded
                )
            })
        {
            return;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for ToolCallFinished for {tool_call_id} in {}",
                events_path.display()
            );
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn read_file_tool(handle: &CoordinatorHandle, worker_agent_id: &str, path: &str) {
    handle
        .execute_agent_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "read",
            serde_json::json!({
                "filePath": path,
                "offset": 1,
                "limit": 2000,
            }),
        )
        .await
        .expect("read tool");
}
