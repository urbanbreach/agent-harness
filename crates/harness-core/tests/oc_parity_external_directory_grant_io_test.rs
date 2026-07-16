//! T7: external_directory ask → allow → successful I/O; deny → no outside I/O.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::clock::FakeClock;
use harness_core::config::PermissionMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::{PermissionDecision, PermissionGrantScope, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_core::UnwrapOrAbort;
use serde_json::{json, Value};

mod common;

#[path = "common/oc_parity_permission_fixtures.rs"]
mod oc_parity;

use oc_parity::{
    permission_kinds_for_tool_call, tool_finished_status, wait_for_tool_settled,
    KIND_EXTERNAL_DIRECTORY,
};

struct GrantAwareReadTool;

#[async_trait]
impl Tool for GrantAwareReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let path = args_json
            .get("filePath")
            .or_else(|| args_json.get("path"))
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("filePath required".to_string()))?;
        let resolved = ctx.resolve_workspace_path(std::path::Path::new(path))?;
        let bytes = fs::read(&resolved).map_err(|source| ToolError::PathResolution {
            path: resolved.display().to_string(),
            source,
        })?;
        Ok(ToolResult::text(String::from_utf8_lossy(&bytes)))
    }
}

struct GrantAwareEditTool;

#[async_trait]
impl Tool for GrantAwareEditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let path = args_json
            .get("filePath")
            .or_else(|| args_json.get("path"))
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("filePath required".to_string()))?;
        let new_string = args_json
            .get("newString")
            .and_then(Value::as_str)
            .unwrap_or("mutated");
        let resolved = ctx.resolve_workspace_path(std::path::Path::new(path))?;
        fs::write(&resolved, new_string).map_err(|source| ToolError::PathResolution {
            path: resolved.display().to_string(),
            source,
        })?;
        Ok(ToolResult::text(format!("wrote {}", resolved.display())))
    }
}

struct GrantAwareBashTool;

#[async_trait]
impl Tool for GrantAwareBashTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let command = args_json
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("");
        for token in command.split_whitespace() {
            if token.starts_with('/') {
                let resolved = ctx.resolve_workspace_path(std::path::Path::new(token))?;
                let bytes = fs::read(&resolved).map_err(|source| ToolError::PathResolution {
                    path: resolved.display().to_string(),
                    source,
                })?;
                return Ok(ToolResult::text(String::from_utf8_lossy(&bytes)));
            }
        }
        Ok(ToolResult::text("bash ok"))
    }
}

fn grant_io_coordinator(session_dir: &std::path::Path) -> CoordinatorHandle {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(GrantAwareReadTool));
    registry.register(Arc::new(GrantAwareEditTool));
    registry.register(Arc::new(GrantAwareBashTool));

    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
    .with_ask_timeout_ms(30_000);
    config.tool_registry = Arc::new(registry);

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn first_permission_id(events: &[EventEnvelopeV1], tool_call_id: &str) -> String {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort()
}

async fn wait_for_tool_finished(
    events_path: &std::path::Path,
    tool_call_id: &str,
    max_yields: usize,
) -> Vec<EventEnvelopeV1> {
    for _ in 0..max_yields {
        let events = common::load_events(events_path);
        if tool_finished_status(&events, tool_call_id).is_some() {
            return events;
        }
        tokio::task::yield_now().await;
    }
    common::load_events(events_path)
}

#[tokio::test]
async fn external_directory_allow_grants_successful_outside_read_bytes() {
    // Given: outside file with known bytes; primary tools Allow; external_directory Ask
    // When: read outside → PermissionRequested → resolve Allow
    // Then: tool succeeds and returns file bytes
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let external = temp_dir.path().join("outside-read.txt");
    fs::write(&external, b"secret-outside-bytes").unwrap_or_abort();

    let session = tempfile::tempdir().unwrap_or_abort();
    let coordinator = grant_io_coordinator(session.path());
    let run = coordinator
        .start_run("t7_grant_read_allow", session.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            common::supervisor_actor(),
            None,
            "read",
            json!({"filePath": external}),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|k| k == KIND_EXTERNAL_DIRECTORY),
        "expected external_directory ask; got {kinds:?}"
    );
    let permission_id = first_permission_id(&events, &tool_call_id);
    coordinator
        .resolve_permission(permission_id, PermissionDecision::Allow, None)
        .await
        .unwrap_or_abort();

    let events = wait_for_tool_finished(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(
        tool_finished_status(&events, &tool_call_id),
        Some(ToolCallStatus::Succeeded)
    );
    let output = events.iter().find_map(|event| match &event.payload {
        EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => {
            data.output_summary.clone()
        }
        _ => None,
    });
    assert_eq!(
        output.as_deref(),
        Some("secret-outside-bytes"),
        "allow must enable successful outside read bytes; got {output:?}"
    );
}

#[tokio::test]
async fn external_directory_deny_leaves_outside_file_unchanged() {
    // Given: outside file with original content
    // When: edit outside → PermissionRequested → resolve Deny
    // Then: file content unchanged; tool not succeeded
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let external = temp_dir.path().join("outside-edit.txt");
    fs::write(&external, b"original-content").unwrap_or_abort();

    let session = tempfile::tempdir().unwrap_or_abort();
    let coordinator = grant_io_coordinator(session.path());
    let run = coordinator
        .start_run("t7_grant_edit_deny", session.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            common::supervisor_actor(),
            None,
            "edit",
            json!({
                "filePath": external,
                "oldString": "original-content",
                "newString": "mutated-by-tool",
            }),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    let permission_id = first_permission_id(&events, &tool_call_id);
    coordinator
        .resolve_permission(permission_id, PermissionDecision::Deny, None)
        .await
        .unwrap_or_abort();

    let events = wait_for_tool_finished(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_ne!(
        tool_finished_status(&events, &tool_call_id),
        Some(ToolCallStatus::Succeeded)
    );
    let content = fs::read_to_string(&external).unwrap_or_abort();
    assert_eq!(
        content, "original-content",
        "deny must leave zero outside I/O"
    );
}

#[tokio::test]
async fn bash_multi_path_external_checks_both_paths() {
    // Given: bash command with two absolute external paths
    // When: requested
    // Then: external_directory ask (both paths collected; no silent partial skip)
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let a = temp_dir.path().join("a.txt");
    let b = temp_dir.path().join("b.txt");
    fs::write(&a, b"aa").unwrap_or_abort();
    fs::write(&b, b"bb").unwrap_or_abort();

    let session = tempfile::tempdir().unwrap_or_abort();
    let coordinator = grant_io_coordinator(session.path());
    let run = coordinator
        .start_run("t7_bash_multi", session.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let command = format!("cat {} {}", a.display(), b.display());
    let tool_call_id = coordinator
        .request_tool_call(
            common::supervisor_actor(),
            None,
            "bash",
            json!({"command": command}),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|k| k == KIND_EXTERNAL_DIRECTORY),
        "multi-path bash must emit external_directory ask; got {kinds:?}"
    );
}

#[tokio::test]
async fn always_grant_is_prefix_scoped_not_whole_fs() {
    // Given: always grant after approving /tmp-like parent path
    // When: second read under same parent, third under unrelated parent
    // Then: sibling under prefix authorized without re-ask; unrelated still asks
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let parent = temp_dir.path().join("granted-parent");
    fs::create_dir_all(&parent).unwrap_or_abort();
    let first = parent.join("one.txt");
    let sibling = parent.join("two.txt");
    let unrelated = temp_dir.path().join("other/secret.txt");
    fs::create_dir_all(unrelated.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(&first, b"one").unwrap_or_abort();
    fs::write(&sibling, b"two").unwrap_or_abort();
    fs::write(&unrelated, b"nope").unwrap_or_abort();

    let session = tempfile::tempdir().unwrap_or_abort();
    let coordinator = grant_io_coordinator(session.path());
    let run = coordinator
        .start_run("t7_always_prefix", session.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let first_id = coordinator
        .request_tool_call(
            common::supervisor_actor(),
            None,
            "read",
            json!({"filePath": first}),
        )
        .await
        .unwrap_or_abort();
    let events = wait_for_tool_settled(&run.events_path, &first_id, 80).await;
    let permission_id = first_permission_id(&events, &first_id);
    coordinator
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    let _ = wait_for_tool_finished(&run.events_path, &first_id, 80).await;

    let sibling_id = coordinator
        .request_tool_call(
            common::supervisor_actor(),
            None,
            "read",
            json!({"filePath": sibling}),
        )
        .await
        .unwrap_or_abort();
    let sibling_events = wait_for_tool_finished(&run.events_path, &sibling_id, 80).await;
    let sibling_kinds = permission_kinds_for_tool_call(&sibling_events, &sibling_id);
    assert!(
        !sibling_kinds.iter().any(|k| k == KIND_EXTERNAL_DIRECTORY),
        "sibling under always prefix must not re-ask; got {sibling_kinds:?}"
    );
    assert_eq!(
        tool_finished_status(&sibling_events, &sibling_id),
        Some(ToolCallStatus::Succeeded)
    );

    let unrelated_id = coordinator
        .request_tool_call(
            common::supervisor_actor(),
            None,
            "read",
            json!({"filePath": unrelated}),
        )
        .await
        .unwrap_or_abort();
    let unrelated_events = wait_for_tool_settled(&run.events_path, &unrelated_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let unrelated_kinds = permission_kinds_for_tool_call(&unrelated_events, &unrelated_id);
    assert!(
        unrelated_kinds.iter().any(|k| k == KIND_EXTERNAL_DIRECTORY),
        "path outside always prefix must still ask; got {unrelated_kinds:?}"
    );
}

#[test]
fn always_external_path_prefix_never_promotes_root() {
    use harness_core::perm::always_external_path_prefix;
    let exact = always_external_path_prefix(std::path::Path::new("/alone"));
    assert_eq!(exact, PathBuf::from("/alone"));
    let parent = always_external_path_prefix(std::path::Path::new("/tmp/foo/bar.txt"));
    assert_eq!(parent, PathBuf::from("/tmp/foo"));
}

#[test]
fn external_path_prefix_covers_descendants_only() {
    use harness_core::perm::external_path_prefix_covers;
    assert!(external_path_prefix_covers("/tmp/foo", "/tmp/foo/a.txt"));
    assert!(external_path_prefix_covers("/tmp/foo", "/tmp/foo"));
    assert!(!external_path_prefix_covers("/tmp/foo", "/tmp/bar/x"));
    assert!(!external_path_prefix_covers("/tmp/foo", "/tmp/foobar"));
}
