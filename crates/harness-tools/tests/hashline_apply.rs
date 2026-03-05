use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::edit::hashline::{compute_line_hash, HashlineOp, HashlinePatch, LineAnchor};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;

#[tokio::test]
async fn hashline_apply_success_writes_file_and_emits_applied_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let file_path = workspace.join("demo.txt");
    let original = "alpha\nbeta\ngamma\n";
    fs::write(&file_path, original).expect("seed file");

    let patch = replace_line_patch("edit-success", "demo.txt", original, 2, "BETA");
    let expected_content = "alpha\nBETA\ngamma\n";
    let expected_digest = blake3::hash(expected_content.as_bytes())
        .to_hex()
        .to_string();

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
    );

    let run = handle
        .start_run("hashline_apply_success", &workspace)
        .await
        .expect("start run");

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).expect("patch json"),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let updated = fs::read_to_string(&file_path).expect("read updated file");
    assert_eq!(updated, expected_content);

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditProposed(data)
                if data.edit_id == "edit-success"
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));

    let applied_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::EditApplied(data)
                if data.edit_id == "edit-success"
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("edit applied event");
    assert_eq!(applied_event.new_file_digest, expected_digest);

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn hashline_apply_mismatch_leaves_file_unchanged() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let file_path = workspace.join("demo.txt");
    let original = "alpha\nbeta\ngamma\n";
    fs::write(&file_path, original).expect("seed file");

    let mut patch = replace_line_patch("edit-mismatch", "demo.txt", original, 2, "BETA");
    if let HashlineOp::Replace { expected, .. } = &mut patch.ops[0] {
        expected[0].hash = "000000000000".to_string();
    }

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
    );

    let run = handle
        .start_run("hashline_apply_mismatch", &workspace)
        .await
        .expect("start run");

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).expect("patch json"),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let unchanged = fs::read_to_string(&file_path).expect("read unchanged file");
    assert_eq!(unchanged, original);

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditRejected(data)
                if data.edit_id == "edit-mismatch"
                    && data.reason.contains("ANCHOR_MISMATCH")
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data) if data.edit_id == "edit-mismatch"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
}

#[tokio::test]
async fn hashline_apply_permission_ask_blocks_until_resolved() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let file_path = workspace.join("demo.txt");
    let original = "alpha\nbeta\ngamma\n";
    fs::write(&file_path, original).expect("seed file");

    let patch = replace_line_patch("edit-ask", "demo.txt", original, 2, "BETA");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Ask,
            PermissionMode::Deny,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(1_000),
    );

    let run = handle
        .start_run("hashline_apply_ask", &workspace)
        .await
        .expect("start run");

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).expect("patch json"),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(40)).await;
    let before_resolve = read_events(&run.events_path);

    assert_eq!(
        fs::read_to_string(&file_path).expect("read before resolve"),
        original,
        "file must not change before permission resolution"
    );
    assert!(before_resolve.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(!before_resolve.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
        )
    }));
    assert!(!before_resolve.iter().any(|event| {
        matches!(&event.payload, EventV1::EditProposed(data) if data.edit_id == "edit-ask")
    }));

    let permission_id = before_resolve
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission id");

    handle
        .resolve_permission(permission_id, PermissionDecision::Allow)
        .await
        .expect("resolve permission");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    assert_eq!(
        fs::read_to_string(&file_path).expect("read updated file"),
        "alpha\nBETA\ngamma\n"
    );

    let events = read_events(&run.events_path);
    let permission_requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
            )
        })
        .expect("permission requested event");
    let permission_resolved_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.decision == harness_core::event::PermissionDecision::Allow
            )
        })
        .expect("permission resolved event");
    let tool_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
            )
        })
        .expect("tool started event");
    let edit_proposed_idx = events
        .iter()
        .position(|event| {
            matches!(&event.payload, EventV1::EditProposed(data) if data.edit_id == "edit-ask")
        })
        .expect("edit proposed event");

    assert!(permission_requested_idx < permission_resolved_idx);
    assert!(permission_resolved_idx < tool_started_idx);
    assert!(tool_started_idx < edit_proposed_idx);
}

fn test_coordinator(session_dir: &Path, permission_policy: PermissionPolicy) -> CoordinatorHandle {
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
            toolset: vec!["edit.hashline_apply".to_string()],
        },
    );

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn worker_actor(agent_id: String) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id))
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

fn replace_line_patch(
    edit_id: &str,
    path: &str,
    source: &str,
    line_number: u32,
    replacement: &str,
) -> HashlinePatch {
    let source_lines = source
        .trim_end_matches('\n')
        .split('\n')
        .collect::<Vec<_>>();
    let anchor = LineAnchor {
        line: line_number,
        hash: compute_line_hash(source_lines[(line_number - 1) as usize]),
    };

    HashlinePatch {
        edit_id: edit_id.to_string(),
        path: path.to_string(),
        ops: vec![HashlineOp::Replace {
            expected: vec![anchor],
            lines: vec![replacement.to_string()],
        }],
    }
}

fn read_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event"))
        .collect()
}
