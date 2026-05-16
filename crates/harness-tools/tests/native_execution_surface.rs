use std::fs;

use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::ShellAllowlist;
use harness_core::edit::hashline::compute_line_hash;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, UserMessageSubmittedEvent,
};
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
async fn native_look_at_extracts_text_and_routes_media() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    fs::write(workspace.join("look.txt"), "visible text evidence").expect("look fixture");

    let tool = registry.get("look_at").expect("look_at in registry");
    let result = tool
        .call(
            test_context(workspace, "look-at"),
            json!({ "goal": "describe file", "file_path": "look.txt" }),
        )
        .await
        .expect("look_at text file");
    let structured = result.structured_json.expect("look_at json");
    assert_eq!(structured["status"], "ok");
    assert_eq!(
        structured["inputs"][0]["extracted_text"],
        "visible text evidence"
    );
    assert!(structured["route"].is_null());

    let image_result = tool
        .call(
            test_context(workspace, "look-at-image"),
            json!({ "goal": "describe image", "image_data": "iVBORw0KGgo=" }),
        )
        .await
        .expect("look_at image_data");
    assert_eq!(
        image_result.structured_json.unwrap()["route"]["subagent_type"],
        "multimodal-looker"
    );
}

#[tokio::test]
async fn native_terminal_tools_are_registered_and_dependency_gated() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());

    let list = registry
        .get("terminal_list")
        .expect("terminal_list")
        .call(test_context(workspace, "terminal-list"), json!({}))
        .await
        .expect("terminal_list reports even when tmux missing");
    assert!(list.structured_json.unwrap().get("sessions").is_some());

    let spawn = registry.get("terminal_spawn").expect("terminal_spawn");
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        let err = spawn
            .call(
                test_context(workspace, "terminal-spawn"),
                json!({ "session_id": "demo", "command": "printf hi" }),
            )
            .await
            .expect_err("missing tmux is dependency-gated");
        assert!(err.to_string().contains("tmux"));
    }
}

#[tokio::test]
async fn native_session_tools_read_replay_safe_session_logs() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let session_dir = workspace.parent().expect("workspace parent").join("run_1");
    fs::create_dir_all(&session_dir).expect("session dir");
    let run_started = EventEnvelopeV1 {
        schema_version: 1,
        event_id: "event_1".to_string(),
        seq: 1,
        run_id: "run_1".to_string(),
        mono_ms: 1,
        ts: Some("2026-05-15T00:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: EventV1::RunStarted(RunStartedEvent {
            run_name: "fixture run".to_string(),
            workspace_root: workspace.display().to_string(),
        }),
    };
    let user_message = EventEnvelopeV1 {
        schema_version: 1,
        event_id: "event_2".to_string(),
        seq: 2,
        run_id: "run_1".to_string(),
        mono_ms: 2,
        ts: Some("2026-05-15T00:00:01Z".to_string()),
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_1".to_string(),
            text: "find the needle".to_string(),
        }),
    };
    fs::write(
        session_dir.join("events.jsonl"),
        format!(
            "{}\n{}\n",
            serde_json::to_string(&run_started).expect("serialize run_started"),
            serde_json::to_string(&user_message).expect("serialize user_message")
        ),
    )
    .expect("events");

    let registry = coordinator_registry(ShellAllowlist::default());
    let list = registry
        .get("session_list")
        .expect("session_list")
        .call(test_context(workspace, "session-list"), json!({}))
        .await
        .expect("session list");
    assert_eq!(
        list.structured_json
            .as_ref()
            .and_then(|value| value.pointer("/sessions/0/session_id")),
        Some(&json!("run_1"))
    );

    let read = registry
        .get("session_read")
        .expect("session_read")
        .call(
            test_context(workspace, "session-read"),
            json!({ "session_id": "run_1", "include_transcript": true }),
        )
        .await
        .expect("session read");
    assert_eq!(
        read.structured_json
            .as_ref()
            .and_then(|value| value.get("event_count")),
        Some(&json!(2))
    );

    let search = registry
        .get("session_search")
        .expect("session_search")
        .call(
            test_context(workspace, "session-search"),
            json!({ "query": "needle", "session_id": "run_1" }),
        )
        .await
        .expect("session search");
    assert_eq!(
        search
            .structured_json
            .as_ref()
            .and_then(|value| value.get("match_count")),
        Some(&json!(1))
    );

    for invalid_session_id in [".", "..", "run_1/../run_1", r"run_1\..\run_1"] {
        let err = registry
            .get("session_read")
            .expect("session_read")
            .call(
                test_context(workspace, "session-read-invalid"),
                json!({ "session_id": invalid_session_id }),
            )
            .await
            .expect_err("session ids must not traverse outside the session directory");
        assert!(
            err.to_string().contains("non-traversing"),
            "unexpected error for {invalid_session_id}: {err}"
        );
    }
}

#[tokio::test]
async fn native_ast_grep_tools_are_first_class_and_dependency_gated() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    fs::write(
        workspace.join("sample.rs"),
        "fn main() { println!(\"hi\"); }\n",
    )
    .expect("sample");
    let registry = coordinator_registry(ShellAllowlist::default());
    let search = registry.get("ast_grep_search").expect("ast_grep_search");
    let outcome = search
        .call(
            test_context(workspace, "ast-grep-search"),
            json!({
                "pattern": "fn $NAME() { $$$ }",
                "lang": "rust",
                "paths": ["sample.rs"]
            }),
        )
        .await;
    match outcome {
        Ok(result) => {
            assert_ne!(
                result
                    .structured_json
                    .as_ref()
                    .and_then(|value| value.get("status")),
                Some(&json!("unsupported"))
            );
        }
        Err(err) => {
            assert!(
                err.to_string().contains("ast-grep CLI not found"),
                "unexpected ast-grep error: {err}"
            );
        }
    }

    let replace = registry.get("ast_grep_replace").expect("ast_grep_replace");
    let denied = replace
        .call(
            test_context(workspace, "ast-grep-replace"),
            json!({
                "pattern": "println!($$$)",
                "rewrite": "eprintln!($$$)",
                "lang": "rust",
                "paths": ["sample.rs"],
                "dry_run": false
            }),
        )
        .await
        .expect_err("non-dry-run replace must stay mediated");
    assert!(denied.to_string().contains("dry-run previews only"));
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
        fallback_model_refs: Vec::new(),
        fallback_model_settings: Vec::new(),
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
