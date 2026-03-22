use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision as EventPermissionDecision,
    ToolCallFinishedEvent, ToolCallStatus,
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

fn worker_profile(toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: "deep".to_string(),
        category: "deep".to_string(),
        model_ref: "default:deep".to_string(),
        system_prompt: "deep prompt".to_string(),
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

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([(
        "deep".to_string(),
        worker_profile(&[
            "agent.spawn",
            "task",
            "tool.batch",
            "batch",
            "fs.read",
            "shell.run",
        ]),
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
async fn native_batch_and_agent_spawn_preserve_child_lineage_permissions_and_order() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("fixture.txt"), "alpha\nbeta\n").expect("fixture file");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let native_spawn_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "agent.spawn",
            json!({
                "profile": "deep",
                "description": "Native background child",
                "prompt": "Say hello from native child",
                "background": true,
                "skills": ["rust-best-practices"],
                "command": "delegate-native",
            }),
        )
        .await
        .expect("request native agent.spawn");
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
            "tool.batch",
            json!({
                "calls": [
                    {"tool_id": "fs.read", "args": {"path": "fixture.txt"}},
                    {"tool_id": "shell.run", "args": {"cmd": "ls", "args": []}},
                    {
                        "tool_id": "batch",
                        "args": {
                            "tool_calls": [{"tool": "fs.read", "parameters": {"path": "fixture.txt"}}]
                        }
                    },
                    {"tool_id": "fs.read", "args": {"path": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request native tool.batch");
    wait_for_tool_call_finish(&run.events_path, &native_batch_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "fs.read", "parameters": {"path": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "fs.read", "parameters": {"path": "fixture.txt", "offset": 1, "limit": 1}}
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
        Some("agent.spawn")
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
        Some("agent.spawn")
    );
    assert_eq!(
        compat_task_metadata.alias_source_tool_id.as_deref(),
        Some("task")
    );
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
        Some("tool.batch")
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
    let native_details = native_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("native batch details array");
    assert_eq!(native_details.len(), 4);
    assert_eq!(native_details[0].get("index"), Some(&json!(0)));
    assert_eq!(native_details[0].get("tool_id"), Some(&json!("fs.read")));
    assert_eq!(native_details[0].get("success"), Some(&json!(true)));

    assert_eq!(native_details[1].get("index"), Some(&json!(1)));
    assert_eq!(native_details[1].get("tool_id"), Some(&json!("shell.run")));
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
    assert_eq!(native_details[3].get("tool_id"), Some(&json!("fs.read")));
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
        Some("tool.batch")
    );
    assert_eq!(
        compat_batch_metadata.alias_source_tool_id.as_deref(),
        Some("batch")
    );

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
        .contains("2: beta"));
    assert!(compat_details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second compat summary")
        .contains("1: alpha"));
}

#[tokio::test]
async fn compat_task_and_batch_delegate_to_native_orchestration() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("fixture.txt"), "alpha\nbeta\n").expect("fixture file");

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
                    {"tool": "fs.read", "parameters": {"path": "fixture.txt", "offset": 2, "limit": 1}},
                    {
                        "tool": "tool.batch",
                        "parameters": {
                            "calls": [
                                {"tool_id": "fs.read", "args": {"path": "fixture.txt"}}
                            ]
                        }
                    },
                    {"tool": "fs.read", "parameters": {"path": "fixture.txt", "offset": 1, "limit": 1}}
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
        Some("agent.spawn")
    );
    assert_eq!(
        compat_task_metadata.alias_source_tool_id.as_deref(),
        Some("task")
    );
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
        Some("tool.batch")
    );
    assert_eq!(
        compat_batch_metadata.alias_source_tool_id.as_deref(),
        Some("batch")
    );
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
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("compat batch details");
    assert_eq!(compat_details.len(), 3);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[0].get("tool_id"), Some(&json!("fs.read")));
    assert_eq!(
        compat_details[0].get("canonical_tool_id"),
        Some(&json!("fs.read"))
    );
    assert_eq!(compat_details[0].get("success"), Some(&json!(true)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first compat summary")
        .contains("2: beta"));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert_eq!(compat_details[1].get("tool_id"), Some(&json!("tool.batch")));
    assert_eq!(
        compat_details[1].get("canonical_tool_id"),
        Some(&json!("tool.batch"))
    );
    assert_eq!(compat_details[1].get("success"), Some(&json!(false)));
    assert!(compat_details[1]
        .get("error")
        .and_then(Value::as_str)
        .expect("nested compat batch error")
        .contains("cannot be nested"));
    assert_eq!(compat_details[2].get("index"), Some(&json!(2)));
    assert_eq!(compat_details[2].get("tool_id"), Some(&json!("fs.read")));
    assert_eq!(
        compat_details[2].get("canonical_tool_id"),
        Some(&json!("fs.read"))
    );
    assert_eq!(compat_details[2].get("success"), Some(&json!(true)));
    assert!(compat_details[2]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second compat summary")
        .contains("1: alpha"));
}
