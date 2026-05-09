use std::fs;

use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::ShellAllowlist;
use harness_core::edit::hashline::compute_line_hash;
use harness_tools::{coordinator_registry, coordinator_registry_with_internal_hashline_tools};
use serde_json::json;

mod common;

use common::{setup_workspace_fixture, test_context as common_test_context};

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-surface-tests", tool_call_id)
}

#[tokio::test]
async fn native_execution_surface_tools_execute_through_native_ids() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());

    let read = registry.get("read").expect("read in registry");
    let todo_write = registry.get("todowrite").expect("todowrite in registry");
    let todo_read = registry.get("todoread").expect("todoread in registry");
    let invalid = registry.get("invalid").expect("invalid in registry");
    assert!(todo_write.description().contains("structured task list"));
    assert!(todo_write.description().contains("test todo"));
    assert!(todo_write
        .description()
        .contains("never reply only \"Done\""));
    assert!(todo_write.description().contains("in_progress"));

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    read.call(
        test_context(workspace, "read"),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2000,
        }),
    )
    .await
    .expect("read");

    let todos_payload = json!({
        "todos": [
            {"content": "task", "status": "pending", "priority": "high"}
        ]
    });
    let todo_write_result = todo_write
        .call(test_context(workspace, "todo-write"), todos_payload.clone())
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
        .call(test_context(workspace, "todo-read"), json!({}))
        .await
        .expect("todoread");
    assert!(todo_read_result.display_text.contains("task"));

    let invalid_result = invalid
        .call(
            test_context(workspace, "invalid"),
            json!({
                "tool": "missing_tool",
                "error": "bad args",
            }),
        )
        .await
        .expect("invalid");
    assert!(invalid_result.display_text.contains("bad args"));
}

#[tokio::test]
async fn native_todowrite_accepts_legacy_text_shape_and_defaults_priority() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());

    let todo_write = registry.get("todowrite").expect("todowrite in registry");
    let todo_read = registry.get("todoread").expect("todoread in registry");

    let todo_write_result = todo_write
        .call(
            test_context(workspace, "todo-write-legacy"),
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
        .call(test_context(workspace, "todo-read-legacy"), json!({}))
        .await
        .expect("todoread legacy state");
    assert!(todo_read_result.display_text.contains("legacy text entry"));
    assert!(todo_read_result.display_text.contains("legacy title entry"));
    assert!(todo_read_result.display_text.contains("medium"));
}

#[tokio::test]
async fn native_todowrite_accepts_state_alias_from_model_tool_call() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());

    let todo_write = registry.get("todowrite").expect("todowrite in registry");
    let schema = todo_write.parameters_json_schema();
    assert!(schema.to_string().contains("\"state\""));

    let todo_write_result = todo_write
        .call(
            test_context(workspace, "todo-write-state-alias"),
            json!({
                "todos": [
                    {"title": "Test functionality", "state": "pending", "priority": "medium"}
                ]
            }),
        )
        .await
        .expect("state-alias todowrite");

    assert_eq!(
        todo_write_result.structured_json,
        Some(json!({
            "todos": [
                {"content": "Test functionality", "status": "pending", "priority": "medium"}
            ]
        }))
    );
}

#[tokio::test]
async fn native_todowrite_accepts_done_shape_and_schema_advertises_it() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());

    let todo_write = registry.get("todowrite").expect("todowrite in registry");
    let schema = todo_write.parameters_json_schema();
    assert!(schema.to_string().contains("\"done\""));
    assert_eq!(
        schema["properties"]["todos"]["items"]["anyOf"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );

    let todo_write_result = todo_write
        .call(
            test_context(workspace, "todo-write-done-shape"),
            json!({
                "todos": [
                    {"done": false, "text": "stress-test harness tools"},
                    {"done": true, "text": "verify read/write/edit/pty paths"}
                ]
            }),
        )
        .await
        .expect("done-shape todowrite");

    assert_eq!(
        todo_write_result.structured_json,
        Some(json!({
            "todos": [
                {"content": "stress-test harness tools", "status": "pending", "priority": "medium"},
                {"content": "verify read/write/edit/pty paths", "status": "completed", "priority": "medium"}
            ]
        }))
    );
}

#[tokio::test]
async fn native_todowrite_rejects_unknown_status_values() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());

    let todo_write = registry.get("todowrite").expect("todowrite in registry");

    let error = todo_write
        .call(
            test_context(workspace, "todo-write-invalid-status"),
            json!({
                "todos": [
                    {"text": "legacy text entry", "status": "doing"}
                ]
            }),
        )
        .await
        .expect_err("unknown todo status should fail");

    let error = error.to_string();
    assert!(error.contains("status must be one of"));
    assert!(error.contains("doing"));
}

#[tokio::test]
async fn native_public_edit_uses_hashline_surface_and_reports_success() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");
    let edit_description = edit.description();

    assert!(edit_description.contains("one edit call per file snapshot"));
    assert!(edit_description.contains("do not overlap ranges"));
    assert!(edit_description.contains("do not insert inside a replaced range"));
    assert!(edit_description.contains("merge touching changes into one replace"));

    let schema = edit.parameters_json_schema();
    assert!(schema["properties"]["edits"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("same original file snapshot")));
    assert!(
        schema["properties"]["edits"]["items"]["properties"]["end"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("must not overlap"))
    );
    assert!(
        schema["properties"]["edits"]["items"]["properties"]["lines"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("no unchanged boundary lines"))
    );

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(workspace, "edit"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(workspace, "edit-start-alias"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\nnext\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(workspace, "edit-opless-delete"),
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
async fn native_public_edit_rejects_delete_flag_with_edit_payload() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");
    let schema = edit.parameters_json_schema();

    assert_eq!(schema["type"], json!("object"));
    assert_eq!(schema["required"], json!(["filePath"]));
    assert_eq!(schema["properties"]["edits"]["minItems"], json!(1));
    assert!(schema["properties"]["delete"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("remove the whole file by path")));

    let file_path = workspace.join("surface.txt");
    fs::write(&file_path, "before\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(workspace, "edit-delete-compat"),
            json!({
                "filePath": file_path.display().to_string(),
                "delete": true,
                "editId": "delete-path-only",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", compute_line_hash("before")),
                        "end": format!("1#{}", compute_line_hash("before")),
                        "lines": null
                    }
                ]
            }),
        )
        .await
        .expect_err("delete=true should reject edit payloads");

    let error = error.to_string();
    assert!(error.contains("cannot be combined with edits"));
    assert!(
        file_path.exists(),
        "delete should not run when edit payload is invalid"
    );
}

#[test]
fn native_provider_tool_defs_accept_edit_and_question_export_schemas() {
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-safe-native-schemas".to_string(),
        category: "test".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec!["edit".to_string(), "question".to_string()],
    };

    let defs = build_provider_tool_defs(&profile, &registry)
        .expect("native edit/question schemas should be provider-safe");

    assert_eq!(defs.len(), 2);
    for def in defs {
        assert_eq!(def.parameters["type"], json!("object"));
    }
}

#[tokio::test]
async fn native_public_edit_rejects_opless_anchored_non_delete_shape() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\nnext\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(workspace, "edit-opless-anchored-non-delete"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "before\nnext\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(workspace, "edit-opless-anchorless"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "current\nnext\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(workspace, "edit-quoted-anchor"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "current\nnext\n").expect("seed existing file");

    let result = edit
        .call(
            test_context(workspace, "edit-hash-only-anchor"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").expect("read in registry");
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    read.call(
        test_context(workspace, "read-disambiguation-window"),
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
            test_context(workspace, "edit-read-window-hash-only-anchor"),
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
async fn native_internal_hashline_scan_disambiguates_hash_only_anchor_for_edit() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry_with_internal_hashline_tools(ShellAllowlist::default());
    let scan = registry
        .get("edit.hashline_scan")
        .expect("edit.hashline_scan in registry");
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    scan.call(
        test_context(workspace, "scan-disambiguation-window"),
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
            test_context(workspace, "edit-scan-window-hash-only-anchor"),
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
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").expect("read in registry");
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    read.call(
        test_context(workspace, "read-stale-disambiguation-window"),
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
            test_context(workspace, "edit-stale-read-window-hash-only-anchor"),
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
    assert!(error.contains("Re-read the file"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).expect("read edited file"),
        "same\nanother\nsame\n"
    );
}

#[tokio::test]
async fn native_public_edit_rejects_ambiguous_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(workspace, "edit-ambiguous-hash-only-anchor"),
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
    assert!(error.contains("Re-read the file"));
    assert!(error.contains(&format!(">>> 1#{}|same", compute_line_hash("same"))));
    assert!(error.contains(&format!(">>> 3#{}|same", compute_line_hash("same"))));
}

#[tokio::test]
async fn native_public_edit_rejects_unknown_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "same\nother\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(workspace, "edit-missing-hash-only-anchor"),
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
    assert!(error.contains("Re-read the file"));
}

#[tokio::test]
async fn native_public_edit_stale_anchor_error_includes_refresh_snippet() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");

    fs::write(workspace.join("surface.txt"), "current\nnext\n").expect("seed existing file");

    let error = edit
        .call(
            test_context(workspace, "edit-stale"),
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
    assert!(error.contains("re-read the file"));
    assert!(error.contains(">>> 1#"));
    assert!(error.contains("|current"));
    assert!(error.contains(">>> 2#"));
}
