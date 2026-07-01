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
async fn native_public_edit_schema_uses_exact_surface_and_runtime_accepts_hashline_compat() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").expect("edit in registry");
    let edit_description = edit.description();

    assert!(edit_description.contains("oldString"));
    assert!(edit_description.contains("newString"));
    assert!(edit_description.contains("replaceAll"));
    assert!(!edit_description.contains("LINE#HASH"));
    assert!(!edit_description.contains("delete=true"));

    let schema = edit.parameters_json_schema();
    assert_eq!(schema["required"], json!(["path", "oldString", "newString"]));
    assert_eq!(schema["properties"]["path"]["type"], json!("string"));
    assert_eq!(schema["properties"]["oldString"]["type"], json!("string"));
    assert_eq!(schema["properties"]["newString"]["type"], json!("string"));
    assert_eq!(schema["properties"]["replaceAll"]["type"], json!("boolean"));
    assert!(schema["properties"].get("filePath").is_none());
    assert!(schema["properties"].get("edits").is_none());

    fs::write(workspace.join("surface.txt"), "before\n").expect("seed existing file");

    // act
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

    // assert
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
    assert_eq!(schema["required"], json!(["path", "oldString", "newString"]));
    assert_eq!(schema["properties"]["path"]["type"], json!("string"));
    assert!(schema["properties"].get("filePath").is_none());
    assert!(schema["properties"].get("edits").is_none());
    assert!(schema["properties"].get("delete").is_none());

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
async fn native_bash_allows_redirection_and_cat() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let bash = registry.get("bash").expect("bash in registry");

    // act
    let result = bash
        .call(
            test_context(workspace, "bash-redirection-cat"),
            json!({
                "command": "echo hello > tmp.txt && cat tmp.txt",
                "description": "write and read with redirection",
            }),
        )
        .await
        .expect("bash redirection and cat should succeed");

    // assert
    assert!(result.display_text.contains("hello"));
    assert!(workspace.join("tmp.txt").exists());
}

#[tokio::test]
async fn native_bash_allows_pipeline_grep() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let bash = registry.get("bash").expect("bash in registry");

    // act
    let result = bash
        .call(
            test_context(workspace, "bash-pipeline-grep"),
            json!({
                "command": "echo a; echo b | grep b",
                "description": "pipe shell output through grep",
            }),
        )
        .await
        .expect("bash pipeline with grep should succeed");

    // assert
    assert!(result.display_text.contains('b'));
}

#[tokio::test]
async fn native_bash_allows_touch_and_rm() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let bash = registry.get("bash").expect("bash in registry");

    // act
    bash
        .call(
            test_context(workspace, "bash-touch-rm"),
            json!({
                "command": "touch tmp.txt && rm tmp.txt",
                "description": "touch then remove a workspace file",
            }),
        )
        .await
        .expect("bash touch and rm should succeed");

    // assert
    assert!(!workspace.join("tmp.txt").exists());
}

#[tokio::test]
async fn native_bash_rejects_python3_c() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let bash = registry.get("bash").expect("bash in registry");

    // act
    let error = bash
        .call(
            test_context(workspace, "bash-python3-c"),
            json!({
                "command": "python3 -c \"print('ok')\"",
                "description": "run python3 inline script",
            }),
        )
        .await
        .expect_err("bash python3 -c should be blocked");

    // assert
    assert!(error.to_string().contains("interpreter command-eval flags"));
}

#[tokio::test]
async fn native_bash_records_permission_patterns_in_metadata() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let bash = registry.get("bash").expect("bash in registry");

    // act
    let result = bash
        .call(
            test_context(workspace, "bash-patterns-metadata"),
            json!({
                "command": "cargo test -p harness-core",
                "description": "record permission patterns",
            }),
        )
        .await
        .expect("bash should record permission patterns");

    // assert
    let metadata = result.structured_json.expect("structured json");
    let always = metadata["permission_always_patterns"]
        .as_array()
        .expect("always patterns array");
    assert!(always.iter().any(|value| value == "cargo test *"));
}
