use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, PlanProfileConfig, RunInfo};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::PermissionDecision;
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolSurface;
use harness_tools::coordinator_registry;
use serde_json::json;
use tokio::time::{sleep, Duration, Instant};

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
        tool_surface: ToolSurface::Native,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn control_plane_toolset() -> Vec<&'static str> {
    vec![
        "todo.write",
        "todowrite",
        "todo.read",
        "todoread",
        "skill.load",
        "skill",
        "tool.invalid",
        "invalid",
        "plan.exit",
        "plan_exit",
    ]
}

async fn spawn_worker_run(
    workspace: &Path,
    worker_profile_name: &str,
    agent_profiles: BTreeMap<String, AgentProfile>,
    plan_profiles: BTreeMap<String, PlanProfileConfig>,
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
    config.plan_profiles = plan_profiles;

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
        .join("opencode-compat")
        .join("todos.json")
}

fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}

async fn wait_for_question_permission(path: &Path, previous: Option<&str>) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(permission_id) =
            read_events(path)
                .into_iter()
                .rev()
                .find_map(|event| match event.payload {
                    EventV1::PermissionRequested(data)
                        if data.kind == "question"
                            && previous.is_none_or(|previous| previous != data.permission_id) =>
                    {
                        Some(data.permission_id)
                    }
                    _ => None,
                })
        {
            return permission_id;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for question permission");
        }
        sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn native_control_plane_tools_cover_invalid_todo_skill_and_plan_exit() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let toolset = control_plane_toolset();
    let agent_profiles = BTreeMap::from([
        ("plan".to_string(), worker_profile("plan", &toolset)),
        ("build".to_string(), worker_profile("build", &[])),
    ]);
    let plan_profiles = BTreeMap::from([
        (
            "plan".to_string(),
            PlanProfileConfig {
                plan_mode: true,
                exit_target_profile: Some("build".to_string()),
            },
        ),
        ("build".to_string(), PlanProfileConfig::default()),
    ]);
    let (handle, run, worker_id) =
        spawn_worker_run(&workspace, "plan", agent_profiles, plan_profiles).await;

    let todos_payload = json!({
        "todos": [{"content": "task", "status": "pending", "priority": "high"}]
    });
    let native_todo_write = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "todo.write",
            todos_payload.clone(),
        )
        .await
        .expect("todo.write");
    let compat_todo_write = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "todowrite",
            todos_payload,
        )
        .await
        .expect("todowrite");
    assert_eq!(
        native_todo_write.display_text,
        compat_todo_write.display_text
    );
    assert_eq!(
        native_todo_write.structured_json,
        compat_todo_write.structured_json
    );
    let state_path = todo_state_file(&run);
    assert!(
        state_path.ends_with(Path::new("opencode-compat/todos.json")),
        "native control-plane state path should preserve the shared compat contract: {}",
        state_path.display()
    );
    assert!(state_path.exists(), "todo state file should be written");

    let native_todo_read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "todo.read",
            json!({}),
        )
        .await
        .expect("todo.read");
    let compat_todo_read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "todoread",
            json!({}),
        )
        .await
        .expect("todoread");
    assert_eq!(native_todo_read.display_text, compat_todo_read.display_text);
    assert_eq!(
        native_todo_read.structured_json,
        compat_todo_read.structured_json
    );

    let native_invalid = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "tool.invalid",
            json!({"tool": "todo.write", "error": "bad args"}),
        )
        .await
        .expect("tool.invalid");
    let compat_invalid = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "invalid",
            json!({"tool": "todo.write", "error": "bad args"}),
        )
        .await
        .expect("invalid");
    assert_eq!(native_invalid.display_text, compat_invalid.display_text);

    let native_skill = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "skill.load",
            json!({"name": "rust-best-practices"}),
        )
        .await
        .expect("skill.load");
    let compat_skill = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "skill",
            json!({"name": "rust-best-practices"}),
        )
        .await
        .expect("skill");
    assert_eq!(native_skill.display_text, compat_skill.display_text);
    assert_eq!(native_skill.structured_json, compat_skill.structured_json);
    assert!(native_skill
        .display_text
        .contains("# Skill: rust-best-practices"));

    let native_plan_exit_handle = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor(&worker_id),
                    Some("plan".to_string()),
                    "plan.exit",
                    json!({}),
                )
                .await
        })
    };
    let native_permission_id = wait_for_question_permission(&run.events_path, None).await;
    handle
        .resolve_permission(
            native_permission_id.clone(),
            PermissionDecision::Allow,
            Some("[[\"Yes\"]]".to_string()),
        )
        .await
        .expect("approve native plan.exit");
    let native_plan_exit = native_plan_exit_handle
        .await
        .expect("join native plan.exit")
        .expect("plan.exit");

    let compat_plan_exit_handle = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor(&worker_id),
                    Some("plan".to_string()),
                    "plan_exit",
                    json!({}),
                )
                .await
        })
    };
    let compat_permission_id =
        wait_for_question_permission(&run.events_path, Some(&native_permission_id)).await;
    handle
        .resolve_permission(
            compat_permission_id,
            PermissionDecision::Allow,
            Some("[[\"Yes\"]]".to_string()),
        )
        .await
        .expect("approve compat plan_exit");
    let compat_plan_exit = compat_plan_exit_handle
        .await
        .expect("join compat plan_exit")
        .expect("plan_exit");
    assert_eq!(native_plan_exit.display_text, compat_plan_exit.display_text);
    assert_eq!(
        native_plan_exit.structured_json,
        compat_plan_exit.structured_json
    );
    let handoff = native_plan_exit
        .structured_json
        .as_ref()
        .and_then(|value| value.get("plan_exit_handoff"))
        .expect("plan exit handoff");
    assert_eq!(handoff.get("source_profile"), Some(&json!("plan")));
    assert_eq!(handoff.get("target_profile"), Some(&json!("build")));
    assert_eq!(
        handoff.get("prompt"),
        Some(&json!(
            "The plan has been approved, you can now edit files. Execute the plan."
        ))
    );
}

#[tokio::test]
async fn plan_exit_rejects_non_plan_profile_or_missing_build_target() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let non_plan_workspace = temp_dir.path().join("workspace-non-plan");
    fs::create_dir_all(&non_plan_workspace).expect("non-plan workspace");
    let missing_build_workspace = temp_dir.path().join("workspace-missing-build");
    fs::create_dir_all(&missing_build_workspace).expect("missing-build workspace");

    let toolset = control_plane_toolset();
    let non_plan_profiles =
        BTreeMap::from([("deep".to_string(), worker_profile("deep", &toolset))]);
    let non_plan_metadata = BTreeMap::from([(
        "deep".to_string(),
        PlanProfileConfig {
            plan_mode: false,
            exit_target_profile: Some("build".to_string()),
        },
    )]);
    let (handle, _run, worker_id) = spawn_worker_run(
        &non_plan_workspace,
        "deep",
        non_plan_profiles,
        non_plan_metadata,
    )
    .await;
    let err = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "plan.exit",
            json!({}),
        )
        .await
        .expect_err("plan.exit should reject non-plan profile");
    assert!(err.contains("not plan-capable"), "unexpected error: {err}");

    let missing_build_profiles =
        BTreeMap::from([("plan".to_string(), worker_profile("plan", &toolset))]);
    let missing_build_metadata = BTreeMap::from([(
        "plan".to_string(),
        PlanProfileConfig {
            plan_mode: true,
            exit_target_profile: None,
        },
    )]);
    let (handle, _run, worker_id) = spawn_worker_run(
        &missing_build_workspace,
        "plan",
        missing_build_profiles,
        missing_build_metadata,
    )
    .await;
    let err = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("plan".to_string()),
            "plan.exit",
            json!({}),
        )
        .await
        .expect_err("plan.exit should reject missing build target");
    assert!(
        err.contains("configured exit target profile") && err.contains("build"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn native_todo_write_rejects_multiple_in_progress_items() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let agent_profiles = BTreeMap::from([(
        "deep".to_string(),
        worker_profile("deep", &["todo.write", "todo.read"]),
    )]);
    let plan_profiles = BTreeMap::from([("deep".to_string(), PlanProfileConfig::default())]);
    let (handle, run, worker_id) =
        spawn_worker_run(&workspace, "deep", agent_profiles, plan_profiles).await;

    handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "todo.write",
            json!({
                "todos": [
                    {"content": "keep", "status": "pending", "priority": "high"}
                ]
            }),
        )
        .await
        .expect("initial todo.write");
    let state_path = todo_state_file(&run);
    let before = fs::read_to_string(&state_path).expect("todo state before invalid write");

    let err = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "todo.write",
            json!({
                "todos": [
                    {"content": "first", "status": "in_progress", "priority": "high"},
                    {"content": "second", "status": "in_progress", "priority": "medium"}
                ]
            }),
        )
        .await
        .expect_err("todo.write should reject multiple in_progress items");
    assert!(err.contains("at most one item with status `in_progress`"));

    let after = fs::read_to_string(&state_path).expect("todo state after invalid write");
    assert_eq!(before, after, "invalid todo.write should not replace state");

    let todo_read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some("deep".to_string()),
            "todo.read",
            json!({}),
        )
        .await
        .expect("todo.read");
    assert_eq!(
        todo_read.structured_json,
        Some(json!({
            "todos": [
                {"content": "keep", "status": "pending", "priority": "high"}
            ]
        }))
    );
}
