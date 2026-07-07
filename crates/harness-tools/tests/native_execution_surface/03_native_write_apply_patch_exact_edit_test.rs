use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn native_public_edit_accepts_exact_string_shape() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();
    let schema = edit.parameters_json_schema();

    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["properties"]["path"]["type"], json!("string"));
    assert!(schema["properties"].get("filePath").is_none());
    assert_eq!(schema["properties"]["oldString"]["type"], json!("string"));
    assert_eq!(schema["properties"]["newString"]["type"], json!("string"));
    assert_eq!(schema["properties"]["replaceAll"]["type"], json!("boolean"));
    assert_eq!(schema["required"], json!(["path", "oldString", "newString"]));
    assert!(schema.get("anyOf").is_none());
    assert!(edit.description().contains("oldString"));
    assert!(!edit.description().contains("LINE#HASH"));

    fs::write(workspace.join("exact.txt"), "alpha\nbeta\nbeta\n").unwrap_or_abort();

    // act
    let error = edit
        .call(
            test_context(workspace, "edit-exact-multiple"),
            json!({
                "path": "exact.txt",
                "oldString": "beta",
                "newString": "BETA"
            }),
        )
        .await
        .expect_err("multiple matches require replaceAll");

    // assert
    assert!(
        error
            .to_string()
            .contains("Found multiple matches for oldString"),
        "unexpected error: {error}"
    );

    let result = edit
        .call(
            test_context(workspace, "edit-exact-replace-all"),
            json!({
                "path": "exact.txt",
                "oldString": "beta",
                "newString": "BETA",
                "replaceAll": true
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Edited file successfully: exact.txt"));
    assert!(result.display_text.contains("Replacements: 2"));
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.get("operation")),
        Some(&json!("edit"))
    );
    assert!(
        result
            .structured_json
            .as_ref()
            .is_some_and(|value| value.get("diagnostics").is_some())
    );
    assert!(
        result
            .structured_json
            .as_ref()
            .is_some_and(|value| value.get("format_warning").is_some())
    );
    assert_eq!(
        fs::read_to_string(workspace.join("exact.txt")).unwrap_or_abort(),
        "alpha\nBETA\nBETA\n"
    );
}

#[tokio::test]
async fn native_public_edit_creates_missing_file_when_old_string_is_empty() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    // act
    let result = edit
        .call(
            test_context(workspace, "edit-create-empty-old"),
            json!({
                "filePath": "created-by-edit.txt",
                "oldString": "",
                "newString": "created\n"
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("Edited file successfully: created-by-edit.txt"));
    assert_eq!(
        fs::read_to_string(workspace.join("created-by-edit.txt")).unwrap_or_abort(),
        "created\n"
    );
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|value| value.get("created"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(
        result
            .structured_json
            .as_ref()
            .is_some_and(|value| value.get("diagnostics").is_some())
    );
}

#[tokio::test]
async fn native_public_edit_rejects_empty_old_string_for_existing_file() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();
    fs::write(workspace.join("existing.txt"), "existing\n").unwrap_or_abort();

    // act
    let error = edit
        .call(
            test_context(workspace, "edit-existing-empty-old"),
            json!({
                "filePath": "existing.txt",
                "oldString": "",
                "newString": "replacement\n"
            }),
        )
        .await
        .expect_err("empty oldString cannot edit existing files");

    // assert
    assert!(error.to_string().contains("oldString cannot be empty"));
    assert_eq!(
        fs::read_to_string(workspace.join("existing.txt")).unwrap_or_abort(),
        "existing\n"
    );
}

#[tokio::test]
async fn native_public_edit_accepts_baseline_line_trimmed_fallback() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("trimmed.txt"), "fn main() {\n    println!(\"hi\");\n}\n")
        .unwrap_or_abort();

    // act
    edit.call(
        test_context(workspace, "edit-line-trimmed-fallback"),
        json!({
            "filePath": "trimmed.txt",
            "oldString": "fn main() {\nprintln!(\"hi\");\n}",
            "newString": "fn main() {\n    println!(\"bye\");\n}"
        }),
    )
    .await
    .unwrap_or_abort();

    // assert
    assert_eq!(
        fs::read_to_string(workspace.join("trimmed.txt")).unwrap_or_abort(),
        "fn main() {\n    println!(\"bye\");\n}\n"
    );
}

#[tokio::test]
async fn native_public_edit_accepts_baseline_whitespace_and_escape_fallbacks() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(
        workspace.join("fallbacks.txt"),
        "let value = alpha    beta;\nlet escaped = line\nvalue;\n",
    )
    .unwrap_or_abort();

    // act
    edit.call(
        test_context(workspace, "edit-whitespace-fallback"),
        json!({
            "filePath": "fallbacks.txt",
            "oldString": "alpha beta",
            "newString": "gamma delta"
        }),
    )
    .await
    .unwrap_or_abort();
    edit.call(
        test_context(workspace, "edit-escape-fallback"),
        json!({
            "filePath": "fallbacks.txt",
            "oldString": "line\\nvalue",
            "newString": "line value"
        }),
    )
    .await
    .unwrap_or_abort();

    // assert
    assert_eq!(
        fs::read_to_string(workspace.join("fallbacks.txt")).unwrap_or_abort(),
        "let value = gamma delta;\nlet escaped = line value;\n"
    );
}

#[tokio::test]
async fn native_public_edit_accepts_baseline_context_fallback() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(
        workspace.join("context.txt"),
        "start\nactual middle\nstable middle\nend\n",
    )
    .unwrap_or_abort();

    // act
    edit.call(
        test_context(workspace, "edit-context-fallback"),
        json!({
            "filePath": "context.txt",
            "oldString": "start\nexpected middle\nstable middle\nend",
            "newString": "start\nreplacement middle\nend"
        }),
    )
    .await
    .unwrap_or_abort();

    // assert
    assert_eq!(
        fs::read_to_string(workspace.join("context.txt")).unwrap_or_abort(),
        "start\nreplacement middle\nend\n"
    );
}

#[tokio::test]
async fn native_public_edit_preserves_literal_prefixes_in_hashline_lines() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("literal.txt"), "old\n").unwrap_or_abort();

    // act
    edit.call(
        test_context(workspace, "edit-literal-prefixes"),
        json!({
            "filePath": "literal.txt",
            "edits": [
                {
                    "op": "replace",
                    "pos": format!("1#{}", compute_line_hash("old")),
                    "lines": [
                        "  indented",
                        "+literal plus",
                        "-literal minus",
                        ">>> literal arrows"
                    ]
                }
            ]
        }),
    )
    .await
    .unwrap_or_abort();

    // assert
    assert_eq!(
        fs::read_to_string(workspace.join("literal.txt")).unwrap_or_abort(),
        "  indented\n+literal plus\n-literal minus\n>>> literal arrows\n"
    );
}

#[tokio::test]
async fn native_write_creates_and_overwrites_one_file() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let write = registry.get("write").unwrap_or_abort();
    let schema = write.parameters_json_schema();

    assert_eq!(schema["required"], json!(["path", "content"]));
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["properties"]["path"]["type"], json!("string"));
    assert!(schema["properties"].get("filePath").is_none());

    // act
    let create = write
        .call(
            test_context(workspace, "write-create"),
            json!({
                "filePath": "created.txt",
                "content": "first\n"
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(create.display_text.contains("Created file successfully: created.txt"));
    assert_eq!(
        create
            .structured_json
            .as_ref()
            .and_then(|json| json["existed"].as_bool()),
        Some(false)
    );

    let alias = write
        .call(
            test_context(workspace, "write-path-alias"),
            json!({
                "path": "created.txt",
                "content": "alias\n"
            }),
        )
        .await
        .unwrap_or_abort();
    assert!(alias.display_text.contains("Wrote file successfully: created.txt"));

    let overwrite = write
        .call(
            test_context(workspace, "write-overwrite"),
            json!({
                "filePath": "created.txt",
                "content": "second\n"
            }),
        )
        .await
        .unwrap_or_abort();
    assert!(overwrite.display_text.contains("Wrote file successfully: created.txt"));
    assert_eq!(
        overwrite
            .structured_json
            .as_ref()
            .and_then(|json| json["existed"].as_bool()),
        Some(true)
    );
    assert_eq!(
        fs::read_to_string(workspace.join("created.txt")).unwrap_or_abort(),
        "second\n"
    );
}

#[tokio::test]
async fn native_apply_patch_applies_add_update_and_delete() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").unwrap_or_abort();

    fs::write(workspace.join("update.txt"), "alpha\nbeta\n").unwrap_or_abort();
    fs::write(workspace.join("delete.txt"), "remove me\n").unwrap_or_abort();

    // act
    let result = apply_patch
        .call(
            test_context(workspace, "apply-patch"),
            json!({
                "patchText": "*** Begin Patch\n*** Add File: added.txt\n+new file\n*** Update File: update.txt\n@@\n alpha\n-beta\n+BETA\n*** Delete File: delete.txt\n*** End Patch"
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("Applied patch sequentially:"));
    assert!(result.display_text.contains("A added.txt"));
    assert!(result.display_text.contains("M update.txt"));
    assert!(result.display_text.contains("D delete.txt"));
    assert_eq!(
        fs::read_to_string(workspace.join("added.txt")).unwrap_or_abort(),
        "new file\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("update.txt")).unwrap_or_abort(),
        "alpha\nBETA\n"
    );
    assert!(!workspace.join("delete.txt").exists());
}

#[tokio::test]
async fn native_apply_patch_rejects_move_without_mutating_workspace() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").unwrap_or_abort();

    fs::write(workspace.join("source.txt"), "alpha\nbeta\n").unwrap_or_abort();

    // act
    let error = apply_patch
        .call(
            test_context(workspace, "apply-patch-move"),
            json!({
                "patchText": "*** Begin Patch\n*** Update File: source.txt\n*** Move to: moved.txt\n@@\n alpha\n-beta\n+BETA\n*** End Patch"
            }),
        )
        .await
        .expect_err("apply_patch move should match baseline rejection");

    // assert
    assert!(
        error
            .to_string()
            .contains("apply_patch moves are not supported yet"),
        "unexpected error: {error}"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("source.txt")).unwrap_or_abort(),
        "alpha\nbeta\n"
    );
    assert!(!workspace.join("moved.txt").exists());
}
