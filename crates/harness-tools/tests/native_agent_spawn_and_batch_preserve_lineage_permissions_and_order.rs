use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use event_log::{find_finished, read_events, wait_for_request_terminal, wait_for_tool_call_finish};
use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventV1, PermissionDecision as EventPermissionDecision, ToolCallStatus,
};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[path = "common/env_guard.rs"]
mod env_guard;
#[allow(dead_code)]
#[path = "common/event_log.rs"]
mod event_log;

use env_guard::EnvGuard;

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn worker_profile(toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: "deep".to_string(),
        category: "deep".to_string(),
        model_ref: "default:deep".to_string(),
        system_prompt: "deep prompt".to_string(),
        max_iters: 12,
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn named_worker_profile(name: &str, toolset: &[&str]) -> AgentProfile {
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

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn write_fixture(workspace: &Path) {
    fs::write(workspace.join("fixture.txt"), "alpha\nbeta\n").expect("fixture file");
}

fn write_numbered_fixture(workspace: &Path) {
    let fixture_body = (1..=30)
        .map(|index| format!("line-{index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace.join("fixture.txt"), format!("{fixture_body}\n")).expect("fixture file");
}

async fn spawn_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([(
        "deep".to_string(),
        worker_profile(&["task", "batch", "read", "bash"]),
    )]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_agent_spawn_batch", workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent(EventActor::new(ActorKind::Supervisor, None), "deep", None)
        .await
        .expect("spawn worker");

    (handle, run, worker_id)
}

#[tokio::test]
async fn native_plan_exit_switches_to_build_agent_after_approval() {
    let _guard = env_lock().lock().await;
    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["Yes"]]"#))]);
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
        ("build".to_string(), named_worker_profile("build", &[])),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_exit", &workspace)
        .await
        .expect("start run");
    let plan_agent_id = handle
        .spawn_agent_idle(EventActor::new(ActorKind::Supervisor, None), "plan", None)
        .await
        .expect("spawn plan");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "plan_exit",
            json!({}),
        )
        .await
        .expect("request plan_exit");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("plan_exit structured output");
    assert_eq!(output["agent"], "build");
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    let build_agent_id = output["build_agent_id"]
        .as_str()
        .expect("build agent id")
        .to_string();

    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == build_agent_id
                && payload.profile == "build"
                && payload.parent_agent_id.as_deref() == Some(plan_agent_id.as_str())
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("has been approved")
                && payload.text.contains(".agent-harness/plans/")
    )));
}

#[tokio::test]
async fn native_batch_and_agent_spawn_preserve_child_lineage_permissions_and_order() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    write_numbered_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let native_spawn_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Native background child",
                "prompt": "Say hello from native child",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
                "command": "delegate-native",
            }),
        )
        .await
        .expect("request native task");
    wait_for_tool_call_finish(&run.events_path, &native_spawn_tool_call_id).await;

    let compat_task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Compat background child",
                "prompt": "Say hello from compat child",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
            }),
        )
        .await
        .expect("request compat task");
    wait_for_tool_call_finish(&run.events_path, &compat_task_tool_call_id).await;

    let native_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt"}},
                    {"tool": "bash", "parameters": {"command": "ls", "workdir": ".", "description": "List workspace"}},
                    {
                        "tool": "batch",
                        "parameters": {
                            "tool_calls": [{"tool": "read", "parameters": {"filePath": "fixture.txt"}}]
                        }
                    },
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request native batch");
    wait_for_tool_call_finish(&run.events_path, &native_batch_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request compat batch");
    wait_for_tool_call_finish(&run.events_path, &compat_batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);

    let native_spawn_finished = find_finished(&events, &native_spawn_tool_call_id);
    assert_eq!(native_spawn_finished.status, ToolCallStatus::Succeeded);
    let native_spawn_metadata = native_spawn_finished
        .metadata
        .as_ref()
        .expect("native spawn metadata");
    assert_eq!(
        native_spawn_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(native_spawn_metadata.alias_source_tool_id.as_deref(), None);
    let native_spawn_output = native_spawn_finished
        .output_json
        .as_ref()
        .expect("native spawn output json");
    assert_eq!(native_spawn_output.get("mode"), Some(&json!("background")));
    assert_eq!(native_spawn_output.get("status"), Some(&json!("scheduled")));
    let native_child_session = native_spawn_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("native child session id");
    let native_child_request = native_spawn_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("native child request id");
    assert_eq!(
        native_spawn_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some(native_child_session)
    );
    assert_eq!(
        native_spawn_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some(native_child_request)
    );

    let native_completed = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload)
                if payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.lineage.as_ref())
                    .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                    == Some(native_spawn_tool_call_id.as_str()) =>
            {
                Some(payload)
            }
            _ => None,
        })
        .expect("native spawn task completion");
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some(native_child_session)
    );
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some(native_child_request)
    );

    let compat_task_finished = find_finished(&events, &compat_task_tool_call_id);
    let compat_task_metadata = compat_task_finished
        .metadata
        .as_ref()
        .expect("compat task metadata");
    assert_eq!(
        compat_task_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(compat_task_metadata.alias_source_tool_id.as_deref(), None);
    let compat_task_output = compat_task_finished
        .output_json
        .as_ref()
        .expect("compat task output json");
    assert_eq!(compat_task_output.get("mode"), Some(&json!("background")));
    assert_eq!(
        compat_task_output
            .get("child_session_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
    );

    let native_batch_finished = find_finished(&events, &native_batch_tool_call_id);
    let native_batch_metadata = native_batch_finished
        .metadata
        .as_ref()
        .expect("native batch metadata");
    assert_eq!(
        native_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(native_batch_metadata.alias_source_tool_id.as_deref(), None);
    let native_batch_output = native_batch_finished
        .output_json
        .as_ref()
        .expect("native batch output json");
    assert_eq!(
        native_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/nested_batch_disallowed"),
        Some(&json!(true))
    );
    assert_eq!(
        native_batch_output.pointer("/audit/successful"),
        Some(&json!(2))
    );
    assert_eq!(
        native_batch_output.pointer("/audit/failed"),
        Some(&json!(2))
    );
    let native_details = native_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("native batch details array");
    assert_eq!(native_details.len(), 4);
    assert_eq!(native_details[0].get("index"), Some(&json!(0)));
    assert_eq!(native_details[0].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        native_details[0].pointer("/request/parameters/filePath"),
        Some(&json!("fixture.txt"))
    );
    assert_eq!(native_details[0].get("success"), Some(&json!(true)));

    assert_eq!(native_details[1].get("index"), Some(&json!(1)));
    assert_eq!(native_details[1].get("tool_id"), Some(&json!("bash")));
    assert_eq!(native_details[1].get("success"), Some(&json!(false)));
    assert!(native_details[1]
        .get("error")
        .and_then(Value::as_str)
        .expect("shell error")
        .contains("tool call denied"));

    assert_eq!(native_details[2].get("index"), Some(&json!(2)));
    assert_eq!(native_details[2].get("tool_id"), Some(&json!("batch")));
    assert_eq!(native_details[2].get("success"), Some(&json!(false)));
    assert!(native_details[2]
        .get("error")
        .and_then(Value::as_str)
        .expect("nested batch error")
        .contains("cannot be nested"));

    assert_eq!(native_details[3].get("index"), Some(&json!(3)));
    assert_eq!(native_details[3].get("tool_id"), Some(&json!("read")));
    assert_eq!(native_details[3].get("success"), Some(&json!(true)));

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == EventPermissionDecision::Deny
                    && data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));

    let compat_batch_finished = find_finished(&events, &compat_batch_tool_call_id);
    let compat_batch_metadata = compat_batch_finished
        .metadata
        .as_ref()
        .expect("compat batch metadata");
    assert_eq!(
        compat_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(compat_batch_metadata.alias_source_tool_id.as_deref(), None);

    let compat_batch_output = compat_batch_finished
        .output_json
        .as_ref()
        .expect("compat batch output json");
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("compat batch details");
    assert_eq!(compat_details.len(), 2);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first compat summary")
        .contains("|line-02"));
    assert!(compat_details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second compat summary")
        .contains("|line-01"));
}

#[tokio::test]
async fn compat_task_and_batch_delegate_to_native_orchestration() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let fixture_body = (1..=30)
        .map(|index| format!("line-{index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace.join("fixture.txt"), format!("{fixture_body}\n")).expect("fixture file");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let compat_task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Compat child",
                "prompt": "Say hello from compat child",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
            }),
        )
        .await
        .expect("request compat task");
    wait_for_tool_call_finish(&run.events_path, &compat_task_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {
                        "tool": "batch",
                        "parameters": {
                            "tool_calls": [
                                {"tool": "read", "parameters": {"filePath": "fixture.txt"}}
                            ]
                        }
                    },
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request compat batch");
    wait_for_tool_call_finish(&run.events_path, &compat_batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);

    let compat_task_finished = find_finished(&events, &compat_task_tool_call_id);
    assert_eq!(compat_task_finished.status, ToolCallStatus::Succeeded);
    let compat_task_metadata = compat_task_finished
        .metadata
        .as_ref()
        .expect("compat task metadata");
    assert_eq!(
        compat_task_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(compat_task_metadata.alias_source_tool_id.as_deref(), None);
    let compat_task_output = compat_task_finished
        .output_json
        .as_ref()
        .expect("compat task output json");
    assert_eq!(compat_task_output.get("mode"), Some(&json!("background")));
    assert_eq!(compat_task_output.get("status"), Some(&json!("scheduled")));
    assert_eq!(
        compat_task_output
            .get("child_session_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
    );
    assert_eq!(
        compat_task_output
            .get("child_request_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref())
    );

    let compat_batch_finished = find_finished(&events, &compat_batch_tool_call_id);
    assert_eq!(compat_batch_finished.status, ToolCallStatus::Succeeded);
    let compat_batch_metadata = compat_batch_finished
        .metadata
        .as_ref()
        .expect("compat batch metadata");
    assert_eq!(
        compat_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(compat_batch_metadata.alias_source_tool_id.as_deref(), None);
    let compat_batch_output = compat_batch_finished
        .output_json
        .as_ref()
        .expect("compat batch output json");
    assert_eq!(
        compat_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        compat_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        compat_batch_output.pointer("/execution/nested_batch_disallowed"),
        Some(&json!(true))
    );
    assert_eq!(
        compat_batch_output.pointer("/audit/successful"),
        Some(&json!(2))
    );
    assert_eq!(
        compat_batch_output.pointer("/audit/failed"),
        Some(&json!(1))
    );
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("compat batch details");
    assert_eq!(compat_details.len(), 3);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[0].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        compat_details[0].pointer("/request/parameters/filePath"),
        Some(&json!("fixture.txt"))
    );
    assert_eq!(
        compat_details[0].get("canonical_tool_id"),
        Some(&json!("read"))
    );
    assert_eq!(compat_details[0].get("success"), Some(&json!(true)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first compat summary")
        .contains("|line-02"));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert_eq!(compat_details[1].get("tool_id"), Some(&json!("batch")));
    assert_eq!(
        compat_details[1].get("canonical_tool_id"),
        Some(&json!("batch"))
    );
    assert_eq!(compat_details[1].get("success"), Some(&json!(false)));
    assert!(compat_details[1]
        .get("error")
        .and_then(Value::as_str)
        .expect("nested compat batch error")
        .contains("cannot be nested"));
    assert_eq!(compat_details[2].get("index"), Some(&json!(2)));
    assert_eq!(compat_details[2].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        compat_details[2].get("canonical_tool_id"),
        Some(&json!("read"))
    );
    assert_eq!(compat_details[2].get("success"), Some(&json!(true)));
    assert!(compat_details[2]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second compat summary")
        .contains("|line-01"));
}

#[tokio::test]
async fn task_tool_rejects_unknown_child_profile_before_spawning_fallback_model() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Missing child profile",
                "prompt": "Try to inspect the repo",
                "subagent_type": "missing_profile",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .expect("request task tool");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .expect("output summary")
        .contains("Unknown child profile `missing_profile`"));
}

#[tokio::test]
async fn batch_tool_accepts_args_alias_on_real_tool_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    write_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "args": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "read", "args": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch tool");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(2)));
    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(
        details[0].pointer("/request/parameters/filePath"),
        Some(&json!("fixture.txt"))
    );
    assert!(details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first summary")
        .contains("|beta"));
    assert!(details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second summary")
        .contains("|alpha"));
}

#[tokio::test]
async fn batch_tool_accepts_wrapper_calls_inside_tool_calls_on_real_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    write_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"recipient_name": "functions.read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"recipient_name": "functions.read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch tool");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(2)));
    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(details[0].get("tool_id"), Some(&json!("read")));
    assert!(details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first summary")
        .contains("|beta"));
    assert!(details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second summary")
        .contains("|alpha"));
}

#[tokio::test]
async fn task_tool_reenters_existing_child_session_by_session_id() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let first_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Initial child",
                "prompt": "First child turn",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request initial task");
    wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let first_events = read_events(&run.events_path);
    let first_finished = find_finished(&first_events, &first_tool_call_id);
    let first_output = first_finished
        .output_json
        .as_ref()
        .expect("initial task output json");
    let child_session_id = first_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("child session id")
        .to_string();
    let first_request_id = first_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &first_request_id).await;

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Resume child by session id",
                "prompt": "Second child turn by session_id",
                "session_id": child_session_id,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task reentry by session_id");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let reentry_events = read_events(&run.events_path);
    let reentry_finished = find_finished(&reentry_events, &reentry_tool_call_id);
    assert_eq!(reentry_finished.status, ToolCallStatus::Succeeded);
    let reentry_output = reentry_finished
        .output_json
        .as_ref()
        .expect("reentry task output json");
    assert_eq!(
        reentry_output
            .get("child_session_id")
            .and_then(Value::as_str),
        Some(child_session_id.as_str())
    );
    assert_eq!(
        reentry_output.get("resumed_existing_session"),
        Some(&json!(true))
    );
    assert_eq!(
        reentry_output.pointer("/child_session/resumed_existing_session"),
        Some(&json!(true))
    );
    let second_request_id = reentry_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("second child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &second_request_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let child_spawn_count = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::AgentSpawned(payload) if payload.agent_id == child_session_id
            )
        })
        .count();
    assert_eq!(child_spawn_count, 1, "reentry must not spawn a new child");
}

#[tokio::test]
async fn task_tool_reenters_existing_child_session_by_task_id() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let first_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Initial child",
                "prompt": "First child turn",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request initial task");
    wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let first_events = read_events(&run.events_path);
    let first_finished = find_finished(&first_events, &first_tool_call_id);
    let first_output = first_finished
        .output_json
        .as_ref()
        .expect("initial task output json");
    let child_task_id = first_output
        .get("task_id")
        .and_then(Value::as_str)
        .expect("child task id")
        .to_string();
    let first_request_id = first_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &first_request_id).await;

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Resume child by task id",
                "prompt": "Second child turn by task_id",
                "task_id": child_task_id,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task reentry by task_id");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let reentry_events = read_events(&run.events_path);
    let reentry_finished = find_finished(&reentry_events, &reentry_tool_call_id);
    assert_eq!(reentry_finished.status, ToolCallStatus::Succeeded);
    let reentry_output = reentry_finished
        .output_json
        .as_ref()
        .expect("reentry task output json");
    assert_eq!(
        reentry_output
            .get("child_session_id")
            .and_then(Value::as_str),
        Some(child_task_id.as_str())
    );
    assert_eq!(
        reentry_output.get("resumed_existing_session"),
        Some(&json!(true))
    );
    assert_eq!(
        reentry_output.pointer("/child_session/resumed_existing_session"),
        Some(&json!(true))
    );
    let second_request_id = reentry_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("second child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &second_request_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let child_spawn_count = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::AgentSpawned(payload) if payload.agent_id == child_task_id
            )
        })
        .count();
    assert_eq!(child_spawn_count, 1, "reentry must not spawn a new child");
}

#[tokio::test]
async fn batch_rejects_more_than_25_calls_preserving_input_order() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("fixture.txt"), "alpha\nbeta\n").expect("fixture file");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let tool_calls = (0..26)
        .map(|index| {
            json!({
                "tool": "read",
                "parameters": {
                    "filePath": "fixture.txt",
                    "offset": index + 1,
                    "limit": 1
                }
            })
        })
        .collect::<Vec<_>>();

    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({ "tool_calls": tool_calls }),
        )
        .await
        .expect("request over-limit batch");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.get("requested_call_count"), Some(&json!(26)));
    assert_eq!(output.get("max_calls"), Some(&json!(25)));
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(25)));
    assert_eq!(output.pointer("/audit/failed"), Some(&json!(1)));
    assert_eq!(
        output.pointer("/audit/discarded_call_count"),
        Some(&json!(1))
    );

    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(details.len(), 26);
    for (index, detail) in details.iter().enumerate() {
        assert_eq!(detail.get("index"), Some(&json!(index)));
        assert_eq!(
            detail.pointer("/request/parameters/offset"),
            Some(&json!(index + 1))
        );
    }
    for detail in &details[..25] {
        assert_eq!(detail.get("success"), Some(&json!(true)));
    }
    assert_eq!(details[25].get("success"), Some(&json!(false)));
    assert!(details[25]
        .get("error")
        .and_then(Value::as_str)
        .expect("over-limit batch error")
        .contains("Maximum of 25 tools allowed in batch"));
}
