use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::config::ShellAllowlist;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::edit::hashline::compute_line_hash;
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
async fn native_todowrite_accepts_legacy_text_shape_and_defaults_priority() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());

    let todo_write = registry.get("todowrite").expect("todowrite in registry");
    let todo_read = registry.get("todoread").expect("todoread in registry");

    let todo_write_result = todo_write
        .call(
            test_context(&workspace, "todo-write-legacy"),
            json!({
                "todos": [
                    {"id": "todo-1", "text": "legacy text entry", "status": "in_progress"},
                    {"title": "legacy title entry", "status": "pending", "priority": "low"}
                ]
            }),
        )
        .await
        .expect("legacy todowrite");

    assert_eq!(
        todo_write_result.structured_json,
        Some(json!({
            "todos": [
                {"content": "legacy text entry", "status": "in_progress", "priority": "medium"},
                {"content": "legacy title entry", "status": "pending", "priority": "low"}
            ]
        }))
    );

    let todo_read_result = todo_read
        .call(test_context(&workspace, "todo-read-legacy"), json!({}))
        .await
        .expect("todoread legacy state");
    assert!(todo_read_result.display_text.contains("legacy text entry"));
    assert!(todo_read_result.display_text.contains("legacy title entry"));
    assert!(todo_read_result.display_text.contains("medium"));
}

#[tokio::test]
async fn native_public_edit_uses_hashline_surface_and_reports_success() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(&workspace, "edit"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", compute_line_hash("before")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect("hashline edit");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "after\n"
    );
}

#[tokio::test]
async fn native_public_edit_accepts_start_alias_for_pos() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(&workspace, "edit-start-alias"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "start": format!("1#{}", compute_line_hash("before")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect("hashline edit with start alias");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "after\n"
    );
}

#[tokio::test]
async fn native_public_edit_accepts_opless_anchored_delete_shape() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\nnext\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(&workspace, "edit-opless-delete"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "pos": format!("1#{}", compute_line_hash("before")),
                        "lines": null,
                    }
                ],
            }),
        )
        .await
        .expect("op-less anchored delete should normalize to replace/delete");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "next\n"
    );
}

#[tokio::test]
async fn native_public_edit_rejects_opless_anchored_non_delete_shape() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\nnext\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(&workspace, "edit-opless-anchored-non-delete"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "pos": format!("1#{}", compute_line_hash("before")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("op-less anchored non-delete should fail with targeted guidance");

    let error = error.to_string();
    assert!(error.contains("missing op"));
    assert!(error.contains("replace, append, and prepend can all use pos/end anchors"));
    assert!(!error.contains("missing field `op`"));
}

#[tokio::test]
async fn native_public_edit_rejects_opless_anchorless_shape() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\nnext\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(&workspace, "edit-opless-anchorless"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("op-less anchorless edit should fail with targeted guidance");

    let error = error.to_string();
    assert!(error.contains("missing op"));
    assert!(error.contains("append inserts at EOF and prepend inserts at BOF"));
    assert!(!error.contains("missing field `op`"));
}

#[tokio::test]
async fn native_public_edit_accepts_quoted_refresh_snippet_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "current\nnext\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(&workspace, "edit-quoted-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!(">>> 1#{}|current", compute_line_hash("current")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect("hashline edit with quoted refresh snippet anchor");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "after\nnext\n"
    );
}

#[tokio::test]
async fn native_public_edit_accepts_unique_hash_only_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "current\nnext\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(&workspace, "edit-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("current")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect("hashline edit with unique hash-only anchor");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "after\nnext\n"
    );
}

#[tokio::test]
async fn native_public_edit_uses_recent_hashline_read_to_disambiguate_hash_only_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").expect("read in registry");
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    read.call(
        test_context(&workspace, "read-disambiguation-window"),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2,
        }),
    )
    .await
    .expect("anchored read should succeed");

    let result = edit
        .call(
            test_context(&workspace, "edit-read-window-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect("hashline edit should use recent read window to disambiguate");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "after\nother\nsame\n"
    );
}

#[tokio::test]
async fn native_public_edit_uses_hashline_scan_to_disambiguate_hash_only_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let scan = registry
        .get("edit.hashline_scan")
        .expect("edit.hashline_scan in registry");
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    scan.call(
        test_context(&workspace, "scan-disambiguation-window"),
        json!({
            "path": "surface.txt",
            "start_line": 1,
            "limit": 2,
        }),
    )
    .await
    .expect("hashline scan should succeed");

    let result = edit
        .call(
            test_context(&workspace, "edit-scan-window-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect("hashline edit should use recent scan window to disambiguate");

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "after\nother\nsame\n"
    );
}

#[tokio::test]
async fn native_public_edit_ignores_stale_recent_hashline_read_for_hash_only_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").expect("read in registry");
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    read.call(
        test_context(&workspace, "read-stale-disambiguation-window"),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2,
        }),
    )
    .await
    .expect("anchored read should succeed");

    fs::write(workspace.join("surface.txt"), "same\nanother\nsame\n")
        .expect("mutate file after anchored read");

    let error = edit
        .call(
            test_context(&workspace, "edit-stale-read-window-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("stale cached anchors should not disambiguate hash-only anchor");

    let error = error.to_string();
    assert!(error.contains("matches multiple current lines"));
    assert!(error.contains("read(hashlineAnchors=true)"));
    assert!(error.contains("edit.hashline_scan"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "same\nanother\nsame\n"
    );
}

#[tokio::test]
async fn native_public_edit_rejects_ambiguous_hash_only_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(&workspace, "edit-ambiguous-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("ambiguous hash-only anchor should fail");

    let error = error.to_string();
    assert!(error.contains("omitted its line number and matches multiple current lines"));
    assert!(error.contains("read(hashlineAnchors=true)"));
    assert!(error.contains("edit.hashline_scan"));
    assert!(error.contains(&format!(">>> 1#{}|same", compute_line_hash("same"))));
    assert!(error.contains(&format!(">>> 3#{}|same", compute_line_hash("same"))));
}

#[tokio::test]
async fn native_public_edit_rejects_unknown_hash_only_anchor() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(&workspace, "edit-missing-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": "#deadbeefdead",
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("unknown hash-only anchor should fail");

    let error = error.to_string();
    assert!(error.contains("does not match any current line"));
    assert!(error.contains("read(hashlineAnchors=true)"));
    assert!(error.contains("edit.hashline_scan"));
}

#[tokio::test]
async fn native_public_edit_stale_anchor_error_includes_refresh_snippet() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "current\nnext\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(&workspace, "edit-stale"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", compute_line_hash("stale")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("stale anchor should fail");

    let error = error.to_string();
    assert!(error.contains("Copy updated tags from this snippet"));
    assert!(error.contains("edit.hashline_scan"));
    assert!(error.contains(">>> 1#"));
    assert!(error.contains("|current"));
    assert!(error.contains(">>> 2#"));
}

#[tokio::test]
async fn native_registry_exposes_only_single_surface_ids() {
    let registry = coordinator_registry(ShellAllowlist::default());
    for tool_id in [
        "question",
        "invalid",
        "write",
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
    assert!(registry.get("apply_patch").is_none());
}
