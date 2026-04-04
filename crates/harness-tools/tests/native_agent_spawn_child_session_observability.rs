use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ProfilePermissions, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallFinishedEvent, ToolCallStatus,
};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolSurface;
use harness_tools::coordinator_registry;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration, Instant};

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn profile(name: &str, category: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: category.to_string(),
        model_ref: "default:default".to_string(),
        system_prompt: format!("{name} prompt"),
        max_iters: 12,
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        tool_surface: ToolSurface::Native,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}

async fn wait_for_tool_call_finish(path: &Path, tool_call_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if read_events(path).iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id
            )
        }) {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for tool call {tool_call_id}"
        );
        sleep(Duration::from_millis(20)).await;
    }
}

fn find_finished(events: &[EventEnvelopeV1], tool_call_id: &str) -> ToolCallFinishedEvent {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("tool call finished event")
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
            profile("parent", "parent", &["agent.spawn", "shell.run"]),
        ),
        (
            "restricted".to_string(),
            profile("restricted", "restricted", &["shell.run"]),
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
            "agent.spawn",
            json!({
                "profile": "parent",
                "description": "Observe child failure metadata",
                "prompt": "This child has no provider configured"
            }),
        )
        .await
        .expect("request agent.spawn");
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
            "agent.spawn",
            json!({
                "profile": "parent",
                "description": "Inherited child scope",
                "prompt": "Background child"
            }),
        )
        .await
        .expect("request inherited spawn");
    wait_for_tool_call_finish(&run.events_path, &inherited_spawn).await;

    let restricted_spawn = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "agent.spawn",
            json!({
                "profile": "restricted",
                "description": "Restricted child scope",
                "prompt": "Background child",
                "background": true
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
            "shell.run",
            json!({
                "cmd": "pwd",
                "args": []
            }),
        )
        .await
        .expect("request inherited child shell.run");
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
            "shell.run",
            json!({
                "cmd": "pwd",
                "args": []
            }),
        )
        .await;
    assert!(
        restricted_shell.is_err(),
        "restricted child shell.run should be denied"
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
