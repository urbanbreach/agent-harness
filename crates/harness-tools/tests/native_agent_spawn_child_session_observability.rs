use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use event_log::{find_finished, read_events, wait_for_tool_call_finish};
use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ProfilePermissions, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{ActorKind, EventActor, EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

#[allow(dead_code)]
#[path = "common/event_log.rs"]
mod event_log;

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn profile(name: &str, category: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: category.to_string(),
        model_ref: "default:default".to_string(),
        system_prompt: format!("{name} prompt"),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

async fn spawn_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
    .with_category_override(
        "restricted",
        ProfilePermissions {
            shell: Some(PermissionMode::Deny),
            ..ProfilePermissions::default()
        },
    );

    let allowlist = ShellAllowlist {
        executables: vec!["pwd".to_string()],
        cwd_roots: vec![".".to_string()],
    };

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = permission_policy;
    config.tool_registry = Arc::new(coordinator_registry(allowlist));
    config.agent_profiles = BTreeMap::from([
        (
            "parent".to_string(),
            profile("parent", "parent", &["task", "bash"]),
        ),
        (
            "restricted".to_string(),
            profile("restricted", "restricted", &["bash"]),
        ),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_agent_spawn_child_session_observability", workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent(EventActor::new(ActorKind::Supervisor, None), "parent", None)
        .await
        .expect("spawn worker");

    (handle, run, worker_id)
}

#[tokio::test]
async fn agent_spawn_returns_child_session_status_duration_and_counts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "category": "parent",
                "description": "Observe child failure metadata",
                "prompt": "This child has no provider configured"
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);

    let output = finished.output_json.as_ref().expect("output json");
    assert_eq!(output.get("status"), Some(&json!("failed")));
    assert!(output.get("duration_ms").and_then(Value::as_u64).is_some());
    assert!(output
        .get("failure_summary")
        .and_then(Value::as_str)
        .expect("failure summary")
        .contains("no provider configured"));
    assert_eq!(
        output.pointer("/child_tool_call_counts/requested"),
        Some(&json!(0))
    );
    assert_eq!(
        output.pointer("/child_tool_call_counts/succeeded"),
        Some(&json!(0))
    );
    assert_eq!(
        output.pointer("/child_tool_call_counts/failed"),
        Some(&json!(0))
    );
    assert_eq!(
        output.pointer("/permissions/scope_relation"),
        Some(&json!("inherits_parent_scope"))
    );
    assert_eq!(
        output.pointer("/child_session/status"),
        Some(&json!("failed"))
    );
    assert_eq!(
        output.pointer("/child_session/mode"),
        Some(&json!("foreground"))
    );
    assert_eq!(
        output.pointer("/child_session/session_id"),
        output.get("child_session_id")
    );
    assert_eq!(
        output.pointer("/child_session/request_id"),
        output.get("child_request_id")
    );

    let child_session_id = output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("child session id");
    let child_request_id = output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("child request id");
    let session_dir = run.run_dir.parent().expect("parent session dir");
    let child_run_dir = session_dir.join(child_session_id);
    let child_events_path = child_run_dir.join("events.jsonl");
    assert!(
        child_events_path.exists(),
        "task should materialize a durable child events.jsonl at {}",
        child_events_path.display()
    );
    let child_events = read_events(&child_events_path);
    assert!(
        child_events.len() >= 4,
        "child log should include its own lifecycle and prompt"
    );
    assert!(child_events
        .iter()
        .enumerate()
        .all(|(index, event)| event.seq == index as u64 + 1 && event.run_id == child_session_id));
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::RunStarted(data)
            if data.run_name == "Observe child failure metadata (@parent subagent)"
    )));
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(data)
            if data.request_id == child_request_id && data.text.contains("no provider configured")
    )));

    let child_meta: Value = serde_json::from_str(
        &fs::read_to_string(child_run_dir.join("meta.json")).expect("read child meta"),
    )
    .expect("parse child meta");
    assert_eq!(child_meta["run_id"], json!(child_session_id));
    assert_eq!(
        child_meta["harness_lineage"]["parent_run_id"],
        json!(run.run_id)
    );
    assert_eq!(
        child_meta["harness_lineage"]["relationship"],
        json!("task_child_session")
    );

    let child_event_count_before_resume = child_events.len();
    let mut resume_config = CoordinatorConfig::new(session_dir.to_path_buf());
    resume_config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );
    resume_config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist {
        executables: vec!["pwd".to_string()],
        cwd_roots: vec![".".to_string()],
    }));
    resume_config.agent_profiles = BTreeMap::from([
        (
            "parent".to_string(),
            profile("parent", "parent", &["task", "bash"]),
        ),
        (
            "restricted".to_string(),
            profile("restricted", "restricted", &["bash"]),
        ),
    ]);
    let resumed_handle = spawn_coordinator(
        resume_config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed_handle
        .resume_run(&run.run_id, "resumed parent")
        .await
        .expect("resume parent run");
    resumed_handle
        .stop_run()
        .await
        .expect("stop resumed parent run");
    let child_events_after_resume = read_events(&child_events_path);
    assert_eq!(
        child_events_after_resume.len(),
        child_event_count_before_resume,
        "resuming the parent must attach child mirrors without appending lifecycle events"
    );
}

#[tokio::test]
async fn child_session_permission_inheritance_isolated_by_task() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let inherited_spawn = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "category": "parent",
                "description": "Inherited child scope",
                "prompt": "Background child",
                "run_in_background": true
            }),
        )
        .await
        .expect("request inherited spawn");
    wait_for_tool_call_finish(&run.events_path, &inherited_spawn).await;

    let restricted_spawn = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "category": "restricted",
                "description": "Restricted child scope",
                "prompt": "Background child",
                "run_in_background": true
            }),
        )
        .await
        .expect("request restricted spawn");
    wait_for_tool_call_finish(&run.events_path, &restricted_spawn).await;

    let inherited_spawn_finished = find_finished(&read_events(&run.events_path), &inherited_spawn);
    let inherited_output = inherited_spawn_finished
        .output_json
        .as_ref()
        .expect("inherited output");
    let inherited_child_session = inherited_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("inherited child session");
    assert_eq!(
        inherited_output.pointer("/permissions/scope_relation"),
        Some(&json!("inherits_parent_scope"))
    );

    let inherited_shell = handle
        .request_tool_call(
            worker_actor(inherited_child_session),
            Some("ignored-by-worker".to_string()),
            "bash",
            json!({
                "command": "pwd",
                "workdir": ".",
                "description": "Print workspace"
            }),
        )
        .await
        .expect("request inherited child bash");
    wait_for_tool_call_finish(&run.events_path, &inherited_shell).await;

    let restricted_spawn_finished =
        find_finished(&read_events(&run.events_path), &restricted_spawn);
    let restricted_output = restricted_spawn_finished
        .output_json
        .as_ref()
        .expect("restricted output");
    let restricted_child_session = restricted_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("restricted child session");
    assert_eq!(
        restricted_output.pointer("/permissions/parent_scope"),
        Some(&json!("parent"))
    );
    assert_eq!(
        restricted_output.pointer("/permissions/child_scope"),
        Some(&json!("restricted"))
    );
    assert_eq!(
        restricted_output.pointer("/permissions/scope_relation"),
        Some(&json!("isolated_by_requested_profile"))
    );
    assert_eq!(restricted_output.get("status"), Some(&json!("scheduled")));

    let restricted_shell = handle
        .request_tool_call(
            worker_actor(restricted_child_session),
            Some("ignored-by-worker".to_string()),
            "bash",
            json!({
                "command": "pwd",
                "workdir": ".",
                "description": "Print workspace"
            }),
        )
        .await;
    assert!(
        restricted_shell.is_err(),
        "restricted child bash should be denied"
    );

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);

    let inherited_shell_finished = find_finished(&events, &inherited_shell);
    assert_eq!(inherited_shell_finished.status, ToolCallStatus::Succeeded);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));
}
