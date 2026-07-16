use harness_tools::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

mod common;

use common::{
    allow_all_permission_policy, anonymous_supervisor_actor, find_finished, read_events,
    setup_workspace_fixture, wait_for_tool_call_finish, worker_actor,
};
use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ProfilePermissions, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

fn profile(name: &str, category: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: category.to_string(),
        model_ref: "default:default".to_string(),
        model_ref_explicit: true,
        system_prompt: format!("{name} prompt"),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
        permission_ruleset: Vec::new(),
    }
}

fn child_observability_permission_policy() -> PermissionPolicy {
    allow_all_permission_policy().with_category_override(
        "restricted",
        ProfilePermissions {
            shell: Some(PermissionMode::Deny),
            ..ProfilePermissions::default()
        },
    )
}

fn pwd_allowlist() -> ShellAllowlist {
    ShellAllowlist {
        executables: vec!["pwd".to_string()],
        cwd_roots: vec![".".to_string()],
        ..ShellAllowlist::default()
    }
}

fn child_observability_profiles() -> BTreeMap<String, AgentProfile> {
    BTreeMap::from([
        (
            "parent".to_string(),
            profile("parent", "parent", &["task", "bash"]),
        ),
        (
            "restricted".to_string(),
            profile("restricted", "restricted", &["bash"]),
        ),
    ])
}

async fn spawn_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = child_observability_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(pwd_allowlist()));
    config.agent_profiles = child_observability_profiles();

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_agent_spawn_child_session_observability", workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), "parent", None)
        .await
        .unwrap_or_abort();

    (handle, run, worker_id)
}

#[tokio::test]
async fn agent_spawn_returns_child_session_status_duration_and_counts() {
    let workspace = setup_workspace_fixture();

    let (handle, run, worker_id) = spawn_run(workspace.workspace()).await;
    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "category": "parent",
                "description": "Observe child failure metadata",
                "prompt": "This child has no provider configured",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);

    let output = finished.output_json.as_ref().unwrap_or_abort();
    assert_eq!(output.get("status"), Some(&json!("failed")));
    assert!(output.get("duration_ms").and_then(Value::as_u64).is_some());
    assert!(output
        .get("failure_summary")
        .and_then(Value::as_str)
        .unwrap_or_abort()
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
        .unwrap_or_abort();
    let child_request_id = output
        .get("child_request_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    let session_dir = run.run_dir.parent().unwrap_or_abort();
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
        .all(|(index, event)| event.seq == index as u64 + 1
            && event.run_id.as_str() == child_session_id));
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::RunStarted(data)
            if data.run_name.as_str() == "Observe child failure metadata (@parent subagent)"
    )));
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(data)
            if data.request_id.as_str() == child_request_id && data.text.contains("no provider configured")
    )));

    let child_meta: Value = serde_json::from_str(
        &fs::read_to_string(child_run_dir.join("meta.json")).unwrap_or_abort(),
    )
    .unwrap_or_abort();
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
    resume_config.permission_policy = allow_all_permission_policy();
    resume_config.tool_registry = Arc::new(coordinator_registry(pwd_allowlist()));
    resume_config.agent_profiles = child_observability_profiles();
    let resumed_handle = spawn_coordinator(
        resume_config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed_handle
        .resume_run(run.run_id.as_str(), "resumed parent")
        .await
        .unwrap_or_abort();
    resumed_handle.stop_run().await.unwrap_or_abort();
    let child_events_after_resume = read_events(&child_events_path);
    assert_eq!(
        child_events_after_resume.len(),
        child_event_count_before_resume,
        "resuming the parent must attach child mirrors without appending lifecycle events"
    );
}

#[tokio::test]
async fn child_session_permission_inheritance_isolated_by_task() {
    let workspace = setup_workspace_fixture();

    let (handle, run, worker_id) = spawn_run(workspace.workspace()).await;

    let inherited_spawn = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "category": "parent",
                "description": "Inherited child scope",
                "prompt": "Background child",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
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
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &restricted_spawn).await;

    let inherited_spawn_finished = find_finished(&read_events(&run.events_path), &inherited_spawn);
    let inherited_output = inherited_spawn_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    let inherited_child_session = inherited_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &inherited_shell).await;

    let restricted_spawn_finished =
        find_finished(&read_events(&run.events_path), &restricted_spawn);
    let restricted_output = restricted_spawn_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    let restricted_child_session = restricted_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
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

    handle.stop_run().await.unwrap_or_abort();
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
