use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallFinishedEvent, ToolCallStatus,
};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolSurface;
use harness_tools::coordinator_registry;
use serde_json::json;
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

fn coordinator_config(session_dir: &Path) -> CoordinatorConfig {
    let allowlist = ShellAllowlist {
        executables: vec!["pwd".to_string()],
        cwd_roots: vec![".".to_string()],
    };

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(allowlist));
    config.agent_profiles = BTreeMap::from([(
        "parent".to_string(),
        profile("parent", "parent", &["agent.spawn", "task"]),
    )]);
    config
}

async fn spawn_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let handle = spawn_coordinator(
        coordinator_config(&session_dir),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_orchestration_toggle", workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent(EventActor::new(ActorKind::Supervisor, None), "parent", None)
        .await
        .expect("spawn worker");

    (handle, run, worker_id)
}

#[tokio::test]
async fn paused_orchestration_blocks_native_and_compat_spawn_tools() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    handle
        .set_orchestration_enabled(EventActor::new(ActorKind::User, None), false)
        .await
        .expect("pause orchestration");

    let native_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "agent.spawn",
            json!({
                "profile": "parent",
                "description": "paused native spawn",
                "prompt": "should not spawn"
            }),
        )
        .await
        .expect("request native agent.spawn");
    let compat_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "category": "parent",
                "description": "paused compat spawn",
                "prompt": "should not spawn"
            }),
        )
        .await
        .expect("request compat task");

    wait_for_tool_call_finish(&run.events_path, &native_tool_call_id).await;
    wait_for_tool_call_finish(&run.events_path, &compat_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    for tool_call_id in [&native_tool_call_id, &compat_tool_call_id] {
        let finished = find_finished(&events, tool_call_id);
        assert_eq!(finished.status, ToolCallStatus::Failed);
        assert!(
            finished
                .output_summary
                .as_deref()
                .is_some_and(|summary| summary.contains("paused")),
            "expected paused summary for {tool_call_id:?}, got {:?}",
            finished.output_summary
        );
    }
}

#[tokio::test]
async fn paused_orchestration_is_restored_when_resuming_a_run() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let session_dir = workspace.join("sessions");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    handle
        .set_orchestration_enabled(EventActor::new(ActorKind::User, None), false)
        .await
        .expect("pause orchestration");
    handle.stop_run().await.expect("stop first run");

    let resumed = spawn_coordinator(
        coordinator_config(&session_dir),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.clone(), "native_orchestration_toggle")
        .await
        .expect("resume run");

    let tool_call_id = resumed
        .request_tool_call(
            worker_actor(&worker_id),
            Some("parent".to_string()),
            "agent.spawn",
            json!({
                "profile": "parent",
                "description": "resume should stay paused",
                "prompt": "should not spawn"
            }),
        )
        .await
        .expect("request agent.spawn after resume");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    resumed.stop_run().await.expect("stop resumed run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("paused")));
}
