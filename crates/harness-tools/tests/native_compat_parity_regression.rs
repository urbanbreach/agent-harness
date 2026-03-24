use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision as EventPermissionDecision,
    TaskCompletedEvent, ToolCallFinishedEvent,
};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolSurface;
use harness_tools::coordinator_registry;
use regex::Regex;
use serde_json::{json, Value};
use tokio::time::{sleep, Instant};

const PNG_BYTES: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn worker_profile(toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: "deep".to_string(),
        category: "deep".to_string(),
        model_ref: "default:deep".to_string(),
        system_prompt: "deep prompt".to_string(),
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

async fn wait_for_question_permission(path: &Path, tool_call_id: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(permission_id) =
            read_events(path)
                .into_iter()
                .find_map(|event| match event.payload {
                    EventV1::PermissionRequested(data)
                        if data.kind == "question"
                            && data.tool_call_id.as_deref() == Some(tool_call_id) =>
                    {
                        Some(data.permission_id)
                    }
                    _ => None,
                })
        {
            return permission_id;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for question permission for {tool_call_id}"
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

fn find_task_completed(
    events: &[EventEnvelopeV1],
    parent_tool_call_id: &str,
) -> TaskCompletedEvent {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload)
                if payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.lineage.as_ref())
                    .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                    == Some(parent_tool_call_id) =>
            {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("task completed event")
}

fn permission_flow(events: &[EventEnvelopeV1], tool_call_id: &str) -> Vec<(String, String)> {
    let permission_ids = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut flow = Vec::new();
    for event in events {
        match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                flow.push(("requested".to_string(), data.kind.clone()));
            }
            EventV1::PermissionResolved(data)
                if permission_ids.iter().any(|id| id == &data.permission_id) =>
            {
                let decision = match data.decision {
                    EventPermissionDecision::Allow => "allow",
                    EventPermissionDecision::Deny => "deny",
                };
                flow.push(("resolved".to_string(), decision.to_string()));
            }
            _ => {}
        }
    }
    flow
}

fn artifact_bytes(run: &RunInfo, artifact_path: &str) -> Vec<u8> {
    let relative = artifact_path
        .strip_prefix("artifacts/")
        .expect("artifact path prefix");
    fs::read(run.artifacts_dir.join(relative)).expect("read artifact bytes")
}

fn normalize_string(value: &str, tool_call_id: &str) -> String {
    let tool_call_pattern = Regex::new(r"toolcall_\d+").expect("tool call regex");
    let agent_pattern = Regex::new(r"agent_\d+").expect("agent regex");
    let request_pattern = Regex::new(r"req_\d+").expect("request regex");

    let normalized = value.replace(tool_call_id, "<tool_call_id>");
    let normalized = tool_call_pattern.replace_all(&normalized, "<tool_call_id>");
    let normalized = agent_pattern.replace_all(&normalized, "<task_id>");
    request_pattern
        .replace_all(&normalized, "<request_id>")
        .into_owned()
}

fn normalize_value(value: &Value, tool_call_id: &str) -> Value {
    match value {
        Value::Object(map) => {
            let mut normalized = serde_json::Map::new();
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "timing"
                        | "output_summary"
                        | "alias_source_tool_id"
                        | "result_summary"
                        | "result_digest"
                ) {
                    continue;
                }
                if matches!(
                    key.as_str(),
                    "child_session_id"
                        | "child_request_id"
                        | "session_id"
                        | "request_id"
                        | "task_id"
                ) {
                    normalized.insert(key.clone(), json!(format!("<{key}>")));
                    continue;
                }
                normalized.insert(key.clone(), normalize_value(child, tool_call_id));
            }
            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| normalize_value(item, tool_call_id))
                .collect(),
        ),
        Value::String(text) => Value::String(normalize_string(text, tool_call_id)),
        other => other.clone(),
    }
}

fn spawn_binary_http_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind binary test server");
    let addr = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                PNG_BYTES.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(PNG_BYTES);
        }
    });
    format!("http://{addr}")
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
            "fs.write",
            "write",
            "web.fetch",
            "webfetch",
            "user.question",
            "question",
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
        .start_run("native_compat_parity_regression", workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent(EventActor::new(ActorKind::Supervisor, None), "deep", None)
        .await
        .expect("spawn worker");

    (handle, run, worker_id)
}

#[tokio::test]
async fn native_and_compat_aliases_match_output_json_artifacts_and_permissions() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("edit.txt"), "alpha\nbeta\n").expect("edit fixture file");
    fs::write(workspace.join("batch.txt"), "alpha\nbeta\n").expect("batch fixture file");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let native_write = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "fs.write",
            json!({
                "path": "edit.txt",
                "content": "native compat parity\n",
            }),
        )
        .await
        .expect("request fs.write");
    wait_for_tool_call_finish(&run.events_path, &native_write).await;
    fs::write(workspace.join("edit.txt"), "alpha\nbeta\n").expect("reset edit fixture");

    let compat_write = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "write",
            json!({
                "filePath": "edit.txt",
                "content": "native compat parity\n",
            }),
        )
        .await
        .expect("request write");
    wait_for_tool_call_finish(&run.events_path, &compat_write).await;

    let fetch_base_url = spawn_binary_http_server();
    let native_fetch = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "web.fetch",
            json!({
                "url": format!("{fetch_base_url}/image"),
                "format": "markdown",
            }),
        )
        .await
        .expect("request web.fetch");
    wait_for_tool_call_finish(&run.events_path, &native_fetch).await;

    let compat_fetch = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "webfetch",
            json!({
                "url": format!("{fetch_base_url}/image"),
                "format": "markdown",
            }),
        )
        .await
        .expect("request webfetch");
    wait_for_tool_call_finish(&run.events_path, &compat_fetch).await;

    let native_question = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "user.question",
            json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}]
                }]
            }),
        )
        .await
        .expect("request user.question");
    let native_question_permission =
        wait_for_question_permission(&run.events_path, &native_question).await;
    handle
        .resolve_permission(
            native_question_permission,
            PermissionDecision::Allow,
            Some("[[\"A\"]]".to_string()),
        )
        .await
        .expect("resolve native question");
    wait_for_tool_call_finish(&run.events_path, &native_question).await;

    let compat_question = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "question",
            json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}]
                }]
            }),
        )
        .await
        .expect("request question");
    let compat_question_permission =
        wait_for_question_permission(&run.events_path, &compat_question).await;
    handle
        .resolve_permission(
            compat_question_permission,
            PermissionDecision::Allow,
            Some("[[\"A\"]]".to_string()),
        )
        .await
        .expect("resolve compat question");
    wait_for_tool_call_finish(&run.events_path, &compat_question).await;

    let native_spawn = handle
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
        .expect("request agent.spawn");
    wait_for_tool_call_finish(&run.events_path, &native_spawn).await;

    let compat_task = handle
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
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &compat_task).await;

    let native_batch = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "tool.batch",
            json!({
                "calls": [
                    {"tool_id": "fs.read", "args": {"path": "batch.txt", "limit": 1}},
                    {"tool_id": "shell.run", "args": {"cmd": "ls", "args": []}},
                    {"tool_id": "fs.read", "args": {"path": "batch.txt", "offset": 2, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request tool.batch");
    wait_for_tool_call_finish(&run.events_path, &native_batch).await;

    let compat_batch = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "fs.read", "parameters": {"path": "batch.txt", "limit": 1}},
                    {"tool": "shell.run", "parameters": {"cmd": "ls", "args": []}},
                    {"tool": "fs.read", "parameters": {"path": "batch.txt", "offset": 2, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch");
    wait_for_tool_call_finish(&run.events_path, &compat_batch).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);

    let native_write_finished = find_finished(&events, &native_write);
    let compat_write_finished = find_finished(&events, &compat_write);
    assert_eq!(
        normalize_value(
            native_write_finished
                .output_json
                .as_ref()
                .expect("native write output json"),
            &native_write,
        ),
        normalize_value(
            compat_write_finished
                .output_json
                .as_ref()
                .expect("compat write output json"),
            &compat_write,
        )
    );
    let native_write_artifact = native_write_finished
        .metadata
        .as_ref()
        .expect("native write metadata")
        .artifact_refs
        .first()
        .expect("native write artifact");
    let compat_write_artifact = compat_write_finished
        .metadata
        .as_ref()
        .expect("compat write metadata")
        .artifact_refs
        .first()
        .expect("compat write artifact");
    assert_eq!(native_write_artifact.digest, compat_write_artifact.digest);
    assert_eq!(
        artifact_bytes(&run, &native_write_artifact.path),
        artifact_bytes(&run, &compat_write_artifact.path)
    );

    let native_fetch_finished = find_finished(&events, &native_fetch);
    let compat_fetch_finished = find_finished(&events, &compat_fetch);
    assert_eq!(
        normalize_value(
            native_fetch_finished
                .output_json
                .as_ref()
                .expect("native fetch output json"),
            &native_fetch,
        ),
        normalize_value(
            compat_fetch_finished
                .output_json
                .as_ref()
                .expect("compat fetch output json"),
            &compat_fetch,
        )
    );
    let native_fetch_artifact = native_fetch_finished
        .metadata
        .as_ref()
        .expect("native fetch metadata")
        .artifact_refs
        .first()
        .expect("native fetch artifact");
    let compat_fetch_artifact = compat_fetch_finished
        .metadata
        .as_ref()
        .expect("compat fetch metadata")
        .artifact_refs
        .first()
        .expect("compat fetch artifact");
    assert_eq!(native_fetch_artifact.digest, compat_fetch_artifact.digest);
    assert_eq!(artifact_bytes(&run, &native_fetch_artifact.path), PNG_BYTES);
    assert_eq!(
        artifact_bytes(&run, &native_fetch_artifact.path),
        artifact_bytes(&run, &compat_fetch_artifact.path)
    );

    let native_question_finished = find_finished(&events, &native_question);
    let compat_question_finished = find_finished(&events, &compat_question);
    assert_eq!(
        normalize_value(
            native_question_finished
                .output_json
                .as_ref()
                .expect("native question output json"),
            &native_question,
        ),
        normalize_value(
            compat_question_finished
                .output_json
                .as_ref()
                .expect("compat question output json"),
            &compat_question,
        )
    );
    assert_eq!(
        permission_flow(&events, &native_question),
        vec![
            ("requested".to_string(), "question".to_string()),
            ("resolved".to_string(), "allow".to_string()),
        ]
    );
    assert_eq!(
        permission_flow(&events, &native_question),
        permission_flow(&events, &compat_question)
    );

    let native_spawn_finished = find_finished(&events, &native_spawn);
    let compat_task_finished = find_finished(&events, &compat_task);
    assert_eq!(
        normalize_value(
            native_spawn_finished
                .output_json
                .as_ref()
                .expect("native spawn output json"),
            &native_spawn,
        ),
        normalize_value(
            compat_task_finished
                .output_json
                .as_ref()
                .expect("compat task output json"),
            &compat_task,
        )
    );
    assert_eq!(
        normalize_value(
            &serde_json::to_value(
                native_spawn_finished
                    .metadata
                    .as_ref()
                    .expect("native spawn metadata"),
            )
            .expect("native spawn metadata json"),
            &native_spawn,
        ),
        normalize_value(
            &serde_json::to_value(
                compat_task_finished
                    .metadata
                    .as_ref()
                    .expect("compat task metadata"),
            )
            .expect("compat task metadata json"),
            &compat_task,
        )
    );

    let native_task_completed = find_task_completed(&events, &native_spawn);
    let compat_task_completed = find_task_completed(&events, &compat_task);
    assert_eq!(
        normalize_value(
            &serde_json::to_value(native_task_completed).expect("native task completed json"),
            &native_spawn,
        ),
        normalize_value(
            &serde_json::to_value(compat_task_completed).expect("compat task completed json"),
            &compat_task,
        )
    );

    let native_batch_finished = find_finished(&events, &native_batch);
    let compat_batch_finished = find_finished(&events, &compat_batch);
    assert_eq!(
        normalize_value(
            native_batch_finished
                .output_json
                .as_ref()
                .expect("native batch output json"),
            &native_batch,
        ),
        normalize_value(
            compat_batch_finished
                .output_json
                .as_ref()
                .expect("compat batch output json"),
            &compat_batch,
        )
    );
}
