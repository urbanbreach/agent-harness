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
async fn native_execution_surface_routes_compat_aliases_through_one_handler() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());

    let fs_write = registry.get("fs.write").expect("fs.write in registry");
    let write = registry.get("write").expect("write in registry");
    let todo_write = registry.get("todo.write").expect("todo.write in registry");
    let todo_write_alias = registry.get("todowrite").expect("todowrite in registry");
    let todo_read = registry.get("todo.read").expect("todo.read in registry");
    let todo_read_alias = registry.get("todoread").expect("todoread in registry");
    let invalid = registry
        .get("tool.invalid")
        .expect("tool.invalid in registry");
    let invalid_alias = registry.get("invalid").expect("invalid in registry");

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    let native_write = fs_write
        .call(
            test_context(&workspace, "native-fs-write"),
            json!({
                "path": "surface.txt",
                "content": "shared path\n",
            }),
        )
        .await
        .expect("fs.write");
    let compat_write = write
        .call(
            test_context(&workspace, "compat-write"),
            json!({
                "filePath": "surface.txt",
                "content": "shared path\n",
            }),
        )
        .await
        .expect("write");
    assert!(native_write
        .display_text
        .contains("Wrote file successfully:"));
    assert!(compat_write
        .display_text
        .contains("Wrote file successfully:"));
    let native_write_json = native_write
        .structured_json
        .clone()
        .expect("native fs.write structured json");
    let compat_write_json = compat_write
        .structured_json
        .clone()
        .expect("compat write structured json");
    assert_eq!(native_write_json.get("path"), compat_write_json.get("path"));
    assert_eq!(
        native_write_json.get("resolved_path"),
        compat_write_json.get("resolved_path")
    );
    assert_eq!(
        native_write_json.get("changed_ranges"),
        compat_write_json.get("changed_ranges")
    );

    let todos_payload = json!({
        "todos": [
            {"content": "task", "status": "pending", "priority": "high"}
        ]
    });
    let native_todo_write = todo_write
        .call(
            test_context(&workspace, "native-todo-write"),
            todos_payload.clone(),
        )
        .await
        .expect("todo.write");
    let compat_todo_write = todo_write_alias
        .call(test_context(&workspace, "compat-todo-write"), todos_payload)
        .await
        .expect("todowrite");
    assert_eq!(
        native_todo_write.display_text,
        compat_todo_write.display_text
    );
    assert_eq!(
        native_todo_write.structured_json,
        compat_todo_write.structured_json
    );

    let native_todo_read = todo_read
        .call(test_context(&workspace, "native-todo-read"), json!({}))
        .await
        .expect("todo.read");
    let compat_todo_read = todo_read_alias
        .call(test_context(&workspace, "compat-todo-read"), json!({}))
        .await
        .expect("todoread");
    assert_eq!(native_todo_read.display_text, compat_todo_read.display_text);
    assert_eq!(
        native_todo_read.structured_json,
        compat_todo_read.structured_json
    );

    let native_invalid = invalid
        .call(
            test_context(&workspace, "native-invalid"),
            json!({
                "tool": "write",
                "error": "bad args",
            }),
        )
        .await
        .expect("tool.invalid");
    let compat_invalid = invalid_alias
        .call(
            test_context(&workspace, "compat-invalid"),
            json!({
                "tool": "write",
                "error": "bad args",
            }),
        )
        .await
        .expect("invalid");
    assert_eq!(native_invalid.display_text, compat_invalid.display_text);
    assert_eq!(
        native_invalid.structured_json,
        compat_invalid.structured_json
    );
}

#[tokio::test]
async fn native_registry_exposes_canonical_and_alias_ids_without_behavior_fork() {
    let registry = coordinator_registry(ShellAllowlist::default());
    for (canonical, alias) in [
        ("user.question", "question"),
        ("tool.invalid", "invalid"),
        ("fs.write", "write"),
        ("web.fetch", "webfetch"),
        ("todo.write", "todowrite"),
        ("todo.read", "todoread"),
        ("skill.load", "skill"),
        ("search.web", "websearch"),
        ("search.code", "codesearch"),
        ("code.lsp", "lsp"),
        ("code.lsp.rename", "code.lsp.rename"),
        ("tool.batch", "batch"),
        ("plan.exit", "plan_exit"),
        ("agent.spawn", "task"),
    ] {
        assert!(
            registry.get(canonical).is_some(),
            "missing canonical tool {canonical}"
        );
        assert!(registry.get(alias).is_some(), "missing alias tool {alias}");
    }

    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let todo_write = registry.get("todo.write").expect("todo.write in registry");
    let todo_write_alias = registry.get("todowrite").expect("todowrite in registry");

    let native = todo_write
        .call(
            test_context(&workspace, "registry-native"),
            json!({
                "todos": [
                    {"content": "registry", "status": "pending", "priority": "medium"}
                ]
            }),
        )
        .await
        .expect("todo.write");
    let alias = todo_write_alias
        .call(
            test_context(&workspace, "registry-alias"),
            json!({
                "todos": [
                    {"content": "registry", "status": "pending", "priority": "medium"}
                ]
            }),
        )
        .await
        .expect("todowrite");
    assert_eq!(native.display_text, alias.display_text);
    assert_eq!(native.structured_json, alias.structured_json);
}
