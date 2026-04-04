use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolSurface;
use harness_tools::coordinator_registry;

#[tokio::test]
async fn native_fs_write_routes_through_hashline_and_emits_edit_events() {
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
        vec!["fs.write".to_string()],
    );

    let run = handle
        .start_run("native_fs_write_routes", &workspace)
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
            "fs.write",
            serde_json::json!({
                "path": "demo.txt",
                "content": "alpha\nBETA\n",
            }),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(80)).await;
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
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data)
                if data.edit_id == edit_id
                    && data.path == "demo.txt"
                    && data.diff_rel_path.as_deref().is_some_and(|path| path.ends_with(".diff"))
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
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
async fn compat_apply_patch_translates_to_hashline_without_behavior_drift() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("kept.txt"), "alpha\nbeta\n").expect("seed kept");
    fs::write(workspace.join("moved_from.txt"), "old line\n").expect("seed moved");
    fs::write(workspace.join("deleted.txt"), "remove me\n").expect("seed deleted");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_translation", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let patch_text = "*** Begin Patch\n*** Add File: added.txt\n+hello from add\n*** Update File: kept.txt\n@@\n-alpha\n+ALPHA\n*** Update File: moved_from.txt\n*** Move to: moved_to.txt\n@@\n-old line\n+new line\n*** Delete File: deleted.txt\n*** End Patch";

    let result = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": patch_text }),
        )
        .await
        .expect("apply_patch tool");
    handle.stop_run().await.expect("stop run");

    assert!(result
        .display_text
        .contains("Success. Updated the following files"));
    assert!(!result.artifacts.is_empty(), "expected diff artifacts");
    assert!(
        result
            .artifacts
            .iter()
            .all(|artifact| artifact.path.ends_with(".diff")),
        "expected only diff artifacts from translated hashline ops"
    );

    assert_eq!(
        fs::read_to_string(workspace.join("kept.txt")).expect("read kept"),
        "ALPHA\nbeta\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("moved_to.txt")).expect("read moved_to"),
        "new line\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("added.txt")).expect("read added"),
        "hello from add"
    );
    assert!(
        !workspace.join("moved_from.txt").exists(),
        "move source should be removed"
    );
    assert!(
        !workspace.join("deleted.txt").exists(),
        "deleted file should be removed"
    );
}

#[tokio::test]
async fn hashline_translation_rejects_invalid_patch_without_file_mutation() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let original = "alpha\nbeta\n";
    fs::write(workspace.join("anchor.txt"), original).expect("seed anchor");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_invalid", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let invalid_patch = "*** Begin Patch\n*** Add File: should-not-exist.txt\n+temporary\n*** Update File: anchor.txt\n@@\n-missing-line\n+changed\n*** End Patch";

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": invalid_patch }),
        )
        .await
        .expect_err("invalid patch should fail");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("hunk context not found"),
        "expected translated verification error, got: {error}"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("anchor.txt")).expect("read anchor"),
        original
    );
    assert!(
        !workspace.join("should-not-exist.txt").exists(),
        "failed translation must not mutate files"
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
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles.insert(
        "worker".to_string(),
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "worker-prompt".to_string(),
            max_iters: 12,
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
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
