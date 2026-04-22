use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, RunInfo};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;
use serde_json::json;

fn actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn worker_profile(name: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: format!("default:{name}"),
        system_prompt: format!("{name} prompt"),
        max_iters: 12,
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn control_plane_toolset() -> Vec<&'static str> {
    vec!["todowrite", "todoread", "skill", "invalid"]
}

async fn spawn_worker_run(
    workspace: &Path,
    worker_profile_name: &str,
    agent_profiles: BTreeMap<String, AgentProfile>,
) -> (harness_core::coord::CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("session-dir");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = agent_profiles;

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_control_plane", workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent(
            EventActor::new(ActorKind::Supervisor, None),
            worker_profile_name,
            None,
        )
        .await
        .expect("spawn worker");
    (handle, run, worker_id)
}

fn todo_state_file(run: &RunInfo) -> PathBuf {
    run.artifacts_dir
        .parent()
        .expect("run root")
        .join("control-plane")
        .join("todos.json")
}

fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}

#[tokio::test]
async fn native_control_plane_tools_cover_invalid_todo_and_skill() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let toolset = control_plane_toolset();
    let agent_profiles = BTreeMap::from([("build".to_string(), worker_profile("build", &toolset))]);
    let (handle, run, worker_id) = spawn_worker_run(&workspace, "build", agent_profiles).await;

    let todos_payload = json!({
        "todos": [{"content": "task", "status": "pending", "priority": "high"}]
    });
    let todo_write = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("build".to_string()),
            "todowrite",
            todos_payload,
        )
        .await
        .expect("todowrite");
    assert_eq!(
        todo_write.structured_json,
        Some(json!({
            "todos": [{"content": "task", "status": "pending", "priority": "high"}]
        }))
    );
    let state_path = todo_state_file(&run);
    assert!(
        state_path.ends_with(Path::new("control-plane/todos.json")),
        "todo state path should use the harness control-plane contract: {}",
        state_path.display()
    );
    assert!(state_path.exists(), "todo state file should be written");

    let todo_read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("build".to_string()),
            "todoread",
            json!({}),
        )
        .await
        .expect("todoread");
    assert!(todo_read.display_text.contains("task"));

    let invalid = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("build".to_string()),
            "invalid",
            json!({"tool": "todowrite", "error": "bad args"}),
        )
        .await
        .expect("invalid");
    assert!(invalid.display_text.contains("bad args"));

    let skill = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("build".to_string()),
            "skill",
            json!({"name": "rust-best-practices"}),
        )
        .await
        .expect("skill");
    assert!(skill.display_text.contains("# Skill: rust-best-practices"));
}

#[tokio::test]
async fn native_todo_write_rejects_multiple_in_progress_items() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let agent_profiles = BTreeMap::from([(
        "deep".to_string(),
        worker_profile("deep", &["todowrite", "todoread"]),
    )]);
    let (handle, run, worker_id) = spawn_worker_run(&workspace, "deep", agent_profiles).await;

    handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "todowrite",
            json!({
                "todos": [
                    {"content": "keep", "status": "pending", "priority": "high"}
                ]
            }),
        )
        .await
        .expect("initial todowrite");
    let state_path = todo_state_file(&run);
    let before = fs::read_to_string(&state_path).expect("todo state before invalid write");

    let err = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "todowrite",
            json!({
                "todos": [
                    {"content": "first", "status": "in_progress", "priority": "high"},
                    {"content": "second", "status": "in_progress", "priority": "medium"}
                ]
            }),
        )
        .await
        .expect_err("todowrite should reject multiple in_progress items");
    assert!(err.contains("at most one item with status `in_progress`"));

    let after = fs::read_to_string(&state_path).expect("todo state after invalid write");
    assert_eq!(before, after, "invalid todowrite should not replace state");

    let todo_read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "todoread",
            json!({}),
        )
        .await
        .expect("todoread");
    assert_eq!(
        todo_read.structured_json,
        Some(json!({
            "todos": [
                {"content": "keep", "status": "pending", "priority": "high"}
            ]
        }))
    );

    let events = read_events(&run.events_path);
    assert!(
        events.iter().any(|event| matches!(
            &event.payload,
            harness_core::event::EventV1::ToolCallFinished(data)
                if data.status == harness_core::event::ToolCallStatus::Failed
        )),
        "expected a failed tool call event for the rejected todo write"
    );
}
