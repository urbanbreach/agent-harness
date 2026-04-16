use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;

#[tokio::test]
async fn native_write_routes_through_hashline_and_emits_edit_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("demo.txt"), "alpha\nbeta\n").expect("seed file");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "write".to_string()],
    );

    let run = handle
        .start_run("native_write_routes", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "demo.txt").await;

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "demo.txt",
                "content": "alpha\nBETA\n",
            }),
        )
        .await
        .expect("request tool call");

    wait_for_tool_call_finished(&run.events_path, &tool_call_id, Duration::from_secs(2)).await;
    handle.stop_run().await.expect("stop run");

    assert_eq!(
        fs::read_to_string(workspace.join("demo.txt")).expect("read updated file"),
        "alpha\nBETA\n"
    );

    let edit_id = format!("fs-write-{tool_call_id}");
    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditProposed(data)
                if data.edit_id == edit_id
                    && data.path == "demo.txt"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::EditApplied(data)
                if data.edit_id == edit_id
                    && data.path == "demo.txt"
                    && data.diff_rel_path.as_deref().is_some_and(|path| path.ends_with(".diff"))
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn compat_apply_patch_translates_to_hashline_without_behavior_drift() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("kept.txt"), "alpha\nbeta\n").expect("seed kept");
    fs::write(workspace.join("moved_from.txt"), "old line\n").expect("seed moved");
    fs::write(workspace.join("deleted.txt"), "remove me\n").expect("seed deleted");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_translation", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let patch_text = "*** Begin Patch\n*** Add File: added.txt\n+hello from add\n*** Update File: kept.txt\n@@\n-alpha\n+ALPHA\n*** Update File: moved_from.txt\n*** Move to: moved_to.txt\n@@\n-old line\n+new line\n*** Delete File: deleted.txt\n*** End Patch";

    let result = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": patch_text }),
        )
        .await
        .expect("apply_patch tool");
    handle.stop_run().await.expect("stop run");

    assert!(result
        .display_text
        .contains("Success. Updated the following files"));
    assert!(!result.artifacts.is_empty(), "expected diff artifacts");
    assert!(
        result
            .artifacts
            .iter()
            .all(|artifact| artifact.path.ends_with(".diff")),
        "expected only diff artifacts from translated hashline ops"
    );

    assert_eq!(
        fs::read_to_string(workspace.join("kept.txt")).expect("read kept"),
        "ALPHA\nbeta\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("moved_to.txt")).expect("read moved_to"),
        "new line\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("added.txt")).expect("read added"),
        "hello from add"
    );
    assert!(
        !workspace.join("moved_from.txt").exists(),
        "move source should be removed"
    );
    assert!(
        !workspace.join("deleted.txt").exists(),
        "deleted file should be removed"
    );
}

#[tokio::test]
async fn native_apply_patch_emits_edit_applied_events_for_each_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("kept.txt"), "alpha\nbeta\n").expect("seed kept");
    fs::write(workspace.join("moved_from.txt"), "old line\n").expect("seed moved");
    fs::write(workspace.join("deleted.txt"), "remove me\n").expect("seed deleted");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let run = handle
        .start_run("native_apply_patch_events", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let patch_text = "*** Begin Patch\n*** Add File: added.txt\n+hello from add\n*** Update File: kept.txt\n@@\n-alpha\n+ALPHA\n*** Update File: moved_from.txt\n*** Move to: moved_to.txt\n@@\n-old line\n+new line\n*** Delete File: deleted.txt\n*** End Patch";

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": patch_text }),
        )
        .await
        .expect("request tool call");

    wait_for_tool_call_finished(&run.events_path, &tool_call_id, Duration::from_secs(2)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    let applied_paths = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::EditApplied(data)
                if event.correlation_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some((data.path.clone(), data.diff_rel_path.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    let applied_path_set = applied_paths
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        applied_path_set,
        std::collections::BTreeSet::from([
            "added.txt".to_string(),
            "deleted.txt".to_string(),
            "kept.txt".to_string(),
            "moved_to.txt".to_string(),
        ])
    );
    assert!(
        applied_paths.iter().all(|(_, diff_rel_path)| diff_rel_path
            .as_deref()
            .is_some_and(|path| path.ends_with(".diff"))),
        "every apply_patch edit should expose a diff artifact"
    );
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn native_edit_rewrites_file_via_hashline_and_emits_diff_artifact() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("notes.md"), "alpha\nbeta\ngamma\n").expect("seed notes");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_routes", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "notes.md").await;

    let result = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "notes.md",
                "oldString": "beta\n",
                "newString": "BETA\n",
            }),
        )
        .await
        .expect("edit tool");
    handle.stop_run().await.expect("stop run");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("notes.md")).expect("read notes"),
        "alpha\nBETA\ngamma\n"
    );
    assert!(
        result
            .artifacts
            .iter()
            .any(|artifact| artifact.path.ends_with(".diff")),
        "native edit should emit a diff artifact"
    );
}

#[tokio::test]
async fn native_edit_revert_succeeds_when_visible_multiline_text_drops_trailing_padding() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(
        workspace.join("tool_lifecycle.snap"),
        "before line    \nsecond line    \nfooter\n",
    )
    .expect("seed snapshot");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_revert_visible_text", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "tool_lifecycle.snap").await;

    handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "tool_lifecycle.snap",
                "oldString": "before line    \nsecond line    \n",
                "newString": "BEFORE line    \nSECOND line    \n",
            }),
        )
        .await
        .expect("forward edit");

    let reverted = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "tool_lifecycle.snap",
                "oldString": "BEFORE line\nSECOND line\n",
                "newString": "before line    \nsecond line    \n",
            }),
        )
        .await
        .expect("revert edit");
    handle.stop_run().await.expect("stop run");

    assert!(reverted.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("tool_lifecycle.snap")).expect("read reverted file"),
        "before line    \nsecond line    \nfooter\n"
    );
}

#[tokio::test]
async fn native_edit_not_found_error_guides_reread_and_whitespace_check() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("notes.md"), "alpha\nbeta\ngamma\n").expect("seed notes");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_not_found_guidance", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "notes.md").await;

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "notes.md",
                "oldString": "missing\nblock\n",
                "newString": "replacement\nblock\n",
            }),
        )
        .await
        .expect_err("missing block should fail");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("re-read the file"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("including whitespace"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn hashline_translation_rejects_invalid_patch_without_file_mutation() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let original = "alpha\nbeta\n";
    fs::write(workspace.join("anchor.txt"), original).expect("seed anchor");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_invalid", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let invalid_patch = "*** Begin Patch\n*** Add File: should-not-exist.txt\n+temporary\n*** Update File: anchor.txt\n@@\n-missing-line\n+changed\n*** End Patch";

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": invalid_patch }),
        )
        .await
        .expect_err("invalid patch should fail");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("hunk context not found"),
        "expected translated verification error, got: {error}"
    );
    assert!(error.contains("anchor.txt"), "unexpected error: {error}");
    assert_eq!(
        fs::read_to_string(workspace.join("anchor.txt")).expect("read anchor"),
        original
    );
    assert!(
        !workspace.join("should-not-exist.txt").exists(),
        "failed translation must not mutate files"
    );
}

#[tokio::test]
async fn compat_apply_patch_relaxes_unique_trailing_whitespace_context_matches() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("anchor.txt"), "alpha  \nbeta\n").expect("seed anchor");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_relaxed_whitespace", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let patch_text =
        "*** Begin Patch\n*** Update File: anchor.txt\n@@\n-alpha\n+ALPHA\n beta\n*** End Patch";

    let result = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": patch_text }),
        )
        .await
        .expect("apply_patch with relaxed whitespace match");
    handle.stop_run().await.expect("stop run");

    assert!(result.display_text.contains("M anchor.txt"));
    assert_eq!(
        fs::read_to_string(workspace.join("anchor.txt")).expect("read anchor"),
        "ALPHA\nbeta\n"
    );
}

#[tokio::test]
async fn compat_apply_patch_rejects_ambiguous_relaxed_context_matches_without_mutation() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let original = "alpha  \nbeta \nmid\nalpha\t\nbeta  \n";
    fs::write(workspace.join("anchor.txt"), original).expect("seed anchor");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_ambiguous_relaxed", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let patch_text =
        "*** Begin Patch\n*** Update File: anchor.txt\n@@\n-alpha\n+ALPHA\n beta\n*** End Patch";

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": patch_text }),
        )
        .await
        .expect_err("ambiguous relaxed match should fail");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("multiple regions match"),
        "unexpected error: {error}"
    );
    assert!(error.contains("anchor.txt"), "unexpected error: {error}");
    assert_eq!(
        fs::read_to_string(workspace.join("anchor.txt")).expect("read anchor"),
        original
    );
}

#[tokio::test]
async fn compat_apply_patch_prefers_exact_match_over_relaxed_candidate() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(
        workspace.join("anchor.txt"),
        "alpha  \nbeta\nmid\nalpha\nbeta\n",
    )
    .expect("seed anchor");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["apply_patch".to_string()],
    );

    let _run = handle
        .start_run("compat_apply_patch_prefers_exact", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let patch_text =
        "*** Begin Patch\n*** Update File: anchor.txt\n@@\n-alpha\n+ALPHA\n beta\n*** End Patch";

    let result = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "apply_patch",
            serde_json::json!({ "patchText": patch_text }),
        )
        .await
        .expect("exact match should win over relaxed candidate");
    handle.stop_run().await.expect("stop run");

    assert!(result.display_text.contains("M anchor.txt"));
    assert_eq!(
        fs::read_to_string(workspace.join("anchor.txt")).expect("read anchor"),
        "alpha  \nbeta\nmid\nALPHA\nbeta\n"
    );
}

#[tokio::test]
async fn native_write_requires_prior_read_for_existing_files() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("demo.txt"), "alpha\nbeta\n").expect("seed file");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["write".to_string()],
    );

    let _run = handle
        .start_run("native_write_requires_read", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "demo.txt",
                "content": "alpha\nBETA\n",
            }),
        )
        .await
        .expect_err("write without prior read should fail");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("You must read file"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("read(hashlineAnchors=true)") || error.contains("edit.hashline_scan"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn native_edit_rejects_stale_reads_after_external_modification() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("notes.md"), "alpha\nbeta\ngamma\n").expect("seed notes");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "edit".to_string()],
    );

    let _run = handle
        .start_run("native_edit_stale_read", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "notes.md").await;
    fs::write(workspace.join("notes.md"), "alpha\nBETA\ngamma\n").expect("external mutation");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "edit",
            serde_json::json!({
                "filePath": "notes.md",
                "oldString": "beta\n",
                "newString": "delta\n",
            }),
        )
        .await
        .expect_err("stale read should fail edit");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("modified since it was last read"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("Please read the file again"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("read(hashlineAnchors=true)") || error.contains("edit.hashline_scan"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn native_write_rejects_stale_reads_after_external_modification() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("demo.txt"), "alpha\nbeta\n").expect("seed file");

    let handle = test_coordinator(
        temp_dir.path(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["read".to_string(), "write".to_string()],
    );

    let _run = handle
        .start_run("native_write_stale_read", &workspace)
        .await
        .expect("start run");
    let worker_agent_id = handle
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    read_file_tool(&handle, &worker_agent_id, "demo.txt").await;
    fs::write(workspace.join("demo.txt"), "alpha\nBETA\n").expect("external mutation");

    let error = handle
        .execute_agent_tool_call(
            worker_actor(&worker_agent_id),
            Some("deep".to_string()),
            "write",
            serde_json::json!({
                "filePath": "demo.txt",
                "content": "alpha\nOMEGA\n",
            }),
        )
        .await
        .expect_err("stale read should fail write");
    handle.stop_run().await.expect("stop run");

    assert!(
        error.contains("modified since it was last read"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("Please read the file again"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("read(hashlineAnchors=true)") || error.contains("edit.hashline_scan"),
        "unexpected error: {error}"
    );
}

fn test_coordinator(
    session_dir: &Path,
    permission_policy: PermissionPolicy,
    toolset: Vec<String>,
) -> CoordinatorHandle {
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
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset,
        },
    );

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn worker_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

fn read_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event"))
        .collect()
}

async fn wait_for_tool_call_finished(events_path: &Path, tool_call_id: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if events_path.exists()
            && read_events(events_path).iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(data)
                        if data.tool_call_id == tool_call_id
                            && data.status == ToolCallStatus::Succeeded
                )
            })
        {
            return;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for ToolCallFinished for {tool_call_id} in {}",
                events_path.display()
            );
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn read_file_tool(handle: &CoordinatorHandle, worker_agent_id: &str, path: &str) {
    handle
        .execute_agent_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "read",
            serde_json::json!({
                "filePath": path,
                "offset": 1,
                "limit": 2000,
            }),
        )
        .await
        .expect("read tool");
}
