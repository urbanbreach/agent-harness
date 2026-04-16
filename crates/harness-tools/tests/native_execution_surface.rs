use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::config::ShellAllowlist;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolContext;
use harness_tools::coordinator_registry;
use serde_json::json;

fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: "run-native-surface-tests".to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
        category: Some("deep".to_string()),
        plan_mode: false,
        plan_exit_target_profile: None,
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    temp_dir
}

#[tokio::test]
async fn native_execution_surface_tools_execute_through_native_ids() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());

    let read = registry.get("read").expect("read in registry");
    let write = registry.get("write").expect("write in registry");
    let todo_write = registry.get("todowrite").expect("todowrite in registry");
    let todo_read = registry.get("todoread").expect("todoread in registry");
    let invalid = registry.get("invalid").expect("invalid in registry");

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    read.call(
        test_context(&workspace, "read"),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2000,
        }),
    )
    .await
    .expect("read");

    let write_result = write
        .call(
            test_context(&workspace, "write"),
            json!({
                "filePath": "surface.txt",
                "content": "shared path\n",
            }),
        )
        .await
        .expect("write");
    assert!(write_result
        .display_text
        .contains("Wrote file successfully:"));
    let write_json = write_result
        .structured_json
        .clone()
        .expect("write structured json");
    assert_eq!(write_json.get("path"), Some(&json!("surface.txt")));
    assert_eq!(
        write_json
            .get("resolved_path")
            .and_then(serde_json::Value::as_str),
        Some(workspace.join("surface.txt").to_string_lossy().as_ref())
    );
    assert!(write_json.get("changed_ranges").is_some());

    let todos_payload = json!({
        "todos": [
            {"content": "task", "status": "pending", "priority": "high"}
        ]
    });
    let todo_write_result = todo_write
        .call(
            test_context(&workspace, "todo-write"),
            todos_payload.clone(),
        )
        .await
        .expect("todowrite");
    assert!(!todo_write_result.display_text.trim().is_empty());
    assert_eq!(
        todo_write_result.structured_json,
        Some(json!({
            "todos": [
                {"content": "task", "status": "pending", "priority": "high"}
            ]
        }))
    );

    let todo_read_result = todo_read
        .call(test_context(&workspace, "todo-read"), json!({}))
        .await
        .expect("todoread");
    assert!(todo_read_result.display_text.contains("task"));

    let invalid_result = invalid
        .call(
            test_context(&workspace, "invalid"),
            json!({
                "tool": "write",
                "error": "bad args",
            }),
        )
        .await
        .expect("invalid");
    assert!(invalid_result.display_text.contains("bad args"));
}

#[tokio::test]
async fn native_registry_exposes_only_single_surface_ids() {
    let registry = coordinator_registry(ShellAllowlist::default());
    for tool_id in [
        "question",
        "invalid",
        "write",
        "apply_patch",
        "webfetch",
        "todowrite",
        "todoread",
        "skill",
        "websearch",
        "codesearch",
        "lsp",
        "lsp.rename",
        "batch",
        "plan_exit",
        "task",
        "list",
    ] {
        assert!(
            registry.get(tool_id).is_some(),
            "missing native tool {tool_id}"
        );
    }

    assert!(
        registry.get("edit").is_some(),
        "missing canonical tool edit"
    );
}
