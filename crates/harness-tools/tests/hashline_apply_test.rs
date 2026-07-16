use harness_tools::UnwrapOrAbort;
use std::fs;
use std::path::Path;
use std::sync::Arc;

mod common;

use common::{
    ask_edit_permission_policy, edit_only_permission_policy, read_events, setup_workspace_fixture,
    supervisor_actor, worker_actor,
};
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::ShellAllowlist;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::edit::hashline::{compute_line_hash, HashlineOp, HashlinePatch, LineAnchor};
use harness_core::event::{EventV1, ToolCallStatus};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry_with_internal_hashline_tools;

#[tokio::test]
async fn hashline_apply_success_writes_file_and_emits_applied_event() {
    let workspace = setup_workspace_fixture();

    let file_path = workspace.workspace().join("demo.txt");
    let original = "alpha\nbeta\ngamma\n";
    fs::write(&file_path, original).unwrap_or_abort();

    let patch = replace_line_patch("edit-success", "demo.txt", original, 2, "BETA");
    let expected_content = "alpha\nBETA\ngamma\n";
    let expected_digest = blake3::hash(expected_content.as_bytes())
        .to_hex()
        .to_string();

    let handle = test_coordinator(workspace.temp_dir(), edit_only_permission_policy());

    let run = handle
        .start_run("hashline_apply_success", workspace.workspace())
        .await
        .unwrap_or_abort();

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).unwrap_or_abort(),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    handle.stop_run().await.unwrap_or_abort();

    let updated = fs::read_to_string(&file_path).unwrap_or_abort();
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
        .unwrap_or_abort();
    assert_eq!(applied_event.new_file_digest, expected_digest);

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn hashline_apply_mismatch_leaves_file_unchanged() {
    let workspace = setup_workspace_fixture();

    let file_path = workspace.workspace().join("demo.txt");
    let original = "alpha\nbeta\ngamma\n";
    fs::write(&file_path, original).unwrap_or_abort();

    let mut patch = replace_line_patch("edit-mismatch", "demo.txt", original, 2, "BETA");
    if let HashlineOp::Replace { expected, .. } = &mut patch.ops[0] {
        expected[0].hash = "000000000000".to_string();
    }

    let handle = test_coordinator(workspace.temp_dir(), edit_only_permission_policy());

    let run = handle
        .start_run("hashline_apply_mismatch", workspace.workspace())
        .await
        .unwrap_or_abort();

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).unwrap_or_abort(),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    handle.stop_run().await.unwrap_or_abort();

    let unchanged = fs::read_to_string(&file_path).unwrap_or_abort();
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
                if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
}

#[tokio::test]
async fn hashline_apply_overlap_rejection_explains_recovery() {
    let workspace = setup_workspace_fixture();

    let file_path = workspace.workspace().join("demo.txt");
    let original = "alpha\nbeta\ngamma\ndelta\n";
    fs::write(&file_path, original).unwrap_or_abort();

    let source_lines = original
        .trim_end_matches('\n')
        .split('\n')
        .collect::<Vec<_>>();
    let anchor = |line_number: u32| LineAnchor {
        line: line_number,
        hash: compute_line_hash(source_lines[(line_number - 1) as usize]),
    };
    let patch = HashlinePatch {
        edit_id: "edit-overlap".to_string(),
        path: "demo.txt".to_string(),
        ops: vec![
            HashlineOp::Replace {
                expected: vec![anchor(2), anchor(3)],
                lines: vec!["BETA".to_string(), "GAMMA".to_string()],
            },
            HashlineOp::Delete {
                expected: vec![anchor(3)],
            },
        ],
    };

    let handle = test_coordinator(workspace.temp_dir(), edit_only_permission_policy());

    let run = handle
        .start_run("hashline_apply_overlap", workspace.workspace())
        .await
        .unwrap_or_abort();

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).unwrap_or_abort(),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    handle.stop_run().await.unwrap_or_abort();

    let unchanged = fs::read_to_string(&file_path).unwrap_or_abort();
    assert_eq!(unchanged, original);

    let events = read_events(&run.events_path);
    let rejection_reason = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::EditRejected(data)
                if data.edit_id == "edit-overlap"
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.reason.as_str())
            }
            _ => None,
        })
        .unwrap_or_abort();

    assert!(rejection_reason.contains("OVERLAP"));
    assert!(rejection_reason.contains("Recovery:"));
    assert!(rejection_reason.contains("original file snapshot"));
    assert!(rejection_reason.contains("merge touching changes into one replace"));
    assert!(rejection_reason.contains("re-read and apply conflicting changes in a second patch"));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
}

#[tokio::test]
async fn hashline_apply_permission_ask_blocks_until_resolved() {
    let workspace = setup_workspace_fixture();

    let file_path = workspace.workspace().join("demo.txt");
    let original = "alpha\nbeta\ngamma\n";
    fs::write(&file_path, original).unwrap_or_abort();

    let patch = replace_line_patch("edit-ask", "demo.txt", original, 2, "BETA");

    let handle = test_coordinator(workspace.temp_dir(), ask_edit_permission_policy());

    let run = handle
        .start_run("hashline_apply_ask", workspace.workspace())
        .await
        .unwrap_or_abort();

    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(&patch).unwrap_or_abort(),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    let before_resolve = read_events(&run.events_path);

    assert_eq!(
        fs::read_to_string(&file_path).unwrap_or_abort(),
        original,
        "file must not change before permission resolution"
    );
    assert!(before_resolve.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
        )
    }));
    assert!(!before_resolve.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == tool_call_id
        )
    }));
    assert!(!before_resolve.iter().any(|event| {
        matches!(&event.payload, EventV1::EditProposed(data) if data.edit_id == "edit-ask")
    }));

    let permission_id = before_resolve
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    handle
        .resolve_permission(permission_id, PermissionDecision::Allow, None)
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    handle.stop_run().await.unwrap_or_abort();

    assert_eq!(
        fs::read_to_string(&file_path).unwrap_or_abort(),
        "alpha\nBETA\ngamma\n"
    );

    let events = read_events(&run.events_path);
    let permission_requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            )
        })
        .unwrap_or_abort();
    let permission_resolved_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.decision == harness_core::event::PermissionDecision::Allow
            )
        })
        .unwrap_or_abort();
    let tool_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == tool_call_id
            )
        })
        .unwrap_or_abort();
    let edit_proposed_idx = events
        .iter()
        .position(|event| {
            matches!(&event.payload, EventV1::EditProposed(data) if data.edit_id == "edit-ask")
        })
        .unwrap_or_abort();

    assert!(permission_requested_idx < permission_resolved_idx);
    assert!(permission_resolved_idx < tool_started_idx);
    assert!(tool_started_idx < edit_proposed_idx);
}

fn test_coordinator(session_dir: &Path, permission_policy: PermissionPolicy) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.permission_policy = permission_policy;
    config.tool_registry = Arc::new(coordinator_registry_with_internal_hashline_tools(
        ShellAllowlist::default(),
    ));
    config.agent_profiles.insert(
        "worker".to_string(),
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "worker-prompt".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: vec!["edit.hashline_apply".to_string()],
            permission_ruleset: Vec::new(),
        },
    );

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
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
