use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod common;

use common::{
    edit_only_permission_policy, read_events, setup_workspace_fixture, supervisor_actor,
    wait_for_succeeded_tool_call_finish, worker_actor,
};
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{McpConfig, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};

#[tokio::test]
async fn native_edit_create_routes_through_hashline_and_emits_edit_events() {
    let workspace = setup_workspace_fixture();

    let handle = test_coordinator(
        workspace.temp_dir(),
        edit_only_permission_policy(),
        vec!["edit".to_string()],
    );

    let run = handle
        .start_run("native_edit_create_routes", workspace.workspace())
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "demo.txt",
                "editId": "create-demo",
                "edits": [
                    {
                        "op": "append",
                        "lines": ["alpha", "BETA"],
                    }
                ],
            }),
        )
        .await
        .expect("request tool call");

    wait_for_succeeded_tool_call_finish(&run.events_path, &tool_call_id, Duration::from_secs(2))
        .await;
    handle.stop_run().await.expect("stop run");

    assert_eq!(
        fs::read_to_string(workspace.workspace().join("demo.txt")).expect("read updated file"),
        "alpha\nBETA\n"
    );

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditProposed(data)
                if data.edit_id == "create-demo"
                    && data.path == "demo.txt"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data)
                if data.edit_id == "create-demo"
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

#[cfg(unix)]
#[tokio::test]
async fn native_edit_create_rejects_symlink_parent_escape() {
    let workspace = setup_workspace_fixture();
    let outside = workspace.temp_dir().join("outside");
    fs::create_dir_all(&outside).expect("outside");
    symlink(&outside, workspace.workspace().join("escape-link"))
        .expect("symlink outside dir into workspace");

    let handle = test_coordinator(
        workspace.temp_dir(),
        edit_only_permission_policy(),
        vec!["edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_create_symlink_escape", workspace.workspace())
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
                "filePath": "escape-link/blocked.txt",
                "edits": [
                    {
                        "op": "append",
                        "lines": ["blocked"],
                    }
                ],
            }),
        )
        .await
        .expect_err("symlink escape should fail edit create");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("path escapes workspace root"),
        "unexpected error: {error}"
    );
    assert!(
        !outside.join("blocked.txt").exists(),
        "edit must not create files outside the workspace"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_edit_rejects_symlink_file_escape() {
    let workspace = setup_workspace_fixture();
    let outside = workspace.temp_dir().join("outside");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(outside.join("secret.txt"), "outside\n").expect("seed outside file");
    symlink(
        outside.join("secret.txt"),
        workspace.workspace().join("escape-file.txt"),
    )
    .expect("symlink outside file into workspace");

    let handle = test_coordinator(
        workspace.temp_dir(),
        edit_only_permission_policy(),
        vec!["edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_symlink_escape", workspace.workspace())
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

#[tokio::test]
async fn native_edit_delete_with_absolute_path_emits_applied_event_without_rejection() {
    let workspace = setup_workspace_fixture();
    let file_path = workspace.workspace().join("demo.txt");
    fs::write(&file_path, "alpha\n").expect("seed file");

    let handle = test_coordinator(
        workspace.temp_dir(),
        edit_only_permission_policy(),
        vec!["edit".to_string()],
    );

    let run = handle
        .start_run("native_edit_delete_compat_absolute", workspace.workspace())
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": file_path.display().to_string(),
                "delete": true,
                "editId": "delete-path-absolute"
            }),
        )
        .await
        .expect("path delete should succeed");
    handle.stop_run().await.expect("stop run");

    assert!(!file_path.exists(), "path delete should remove the file");

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data)
                if data.edit_id == "delete-path-absolute" && data.path == "demo.txt"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditRejected(data) if data.edit_id == "delete-path-absolute"
        )
    }));
}

#[tokio::test]
async fn native_edit_rename_only_moves_existing_file() {
    let workspace = setup_workspace_fixture();
    fs::write(workspace.workspace().join("old.txt"), "alpha\n").expect("seed file");

    let handle = test_coordinator(
        workspace.temp_dir(),
        edit_only_permission_policy(),
        vec!["edit".to_string()],
    );

    let run = handle
        .start_run("native_edit_rename_only", workspace.workspace())
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "old.txt",
                "editId": "rename-only",
                "rename": "new.txt",
            }),
        )
        .await
        .expect("rename-only edit should succeed");
    handle.stop_run().await.expect("stop run");

    assert!(!workspace.workspace().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(workspace.workspace().join("new.txt")).expect("read moved file"),
        "alpha\n"
    );

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data) if data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn native_edit_rename_failure_does_not_apply_content_edit() {
    let workspace = setup_workspace_fixture();
    fs::write(workspace.workspace().join("old.txt"), "alpha\n").expect("seed source file");
    fs::write(workspace.workspace().join("new.txt"), "occupied\n").expect("seed destination file");

    let handle = test_coordinator(
        workspace.temp_dir(),
        edit_only_permission_policy(),
        vec!["edit".to_string()],
    );

    let run = handle
        .start_run("native_edit_rename_failure_atomic", workspace.workspace())
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
                "filePath": "old.txt",
                "editId": "rename-fails",
                "rename": "new.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", harness_core::edit::hashline::compute_line_hash("alpha")),
                        "lines": ["changed"],
                    }
                ],
            }),
        )
        .await
        .expect_err("rename to existing destination should fail before editing");
    handle.stop_run().await.expect("stop run");

    assert!(error.contains("destination already exists"));
    assert_eq!(
        fs::read_to_string(workspace.workspace().join("old.txt")).expect("read source file"),
        "alpha\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.workspace().join("new.txt")).expect("read destination file"),
        "occupied\n"
    );

    let events = read_events(&run.events_path);
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data) if data.edit_id == "rename-fails"
        )
    }));
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
            max_iters: Some(12),
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
