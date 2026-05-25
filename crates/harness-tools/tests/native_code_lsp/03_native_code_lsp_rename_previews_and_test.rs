#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_rename_previews_and_applies_workspace_edits() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-rename-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake rename bin dir");
    let primary_uri = file_uri(&workspace.join("src/lib.rs"));
    let secondary_uri = file_uri(&workspace.join("src/other.rs"));
    install_fake_rename_lsp_binary(
        &fake_bin,
        "custom-rename-lsp",
        &primary_uri,
        &secondary_uri,
        "ok",
        "documentChanges",
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec![fake_lsp_command(&fake_bin, "custom-rename-lsp")]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let rename_tool = registry.get("lsp.rename").expect("lsp.rename tool");

    let preview = rename_tool
        .call(
            test_context(&workspace, "rename-preview"),
            json!({
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 4,
                "newName": "renamed",
                "apply": false,
            }),
        )
        .await
        .expect("rename preview");
    assert!(preview.display_text.contains("Prepared LSP rename preview"));
    assert!(preview.display_text.contains("helper"));
    assert!(preview.display_text.contains("Re-run with `apply: true`"));
    let preview_json = preview
        .structured_json
        .clone()
        .expect("rename preview structured json");
    assert_eq!(preview_json["operation"], json!("renameSymbol"));
    assert_eq!(preview_json["applied"], json!(false));
    assert_eq!(preview_json["symbol"], json!("helper"));
    assert_eq!(preview_json["preview"]["file_count"], json!(2));
    assert_eq!(preview_json["preview"]["text_edit_count"], json!(2));
    assert_eq!(
        preview_json["preview"]["annotations"][0]["label"],
        json!("Semantic rename")
    );
    assert!(first_diagnostic_message(&preview_json).contains("prepare_ok=True; workspace_ok=True"));
    assert_eq!(
        fs::read_to_string(workspace.join("src/lib.rs")).expect("read lib.rs after preview"),
        "fn helper() {}\n\nfn caller() {\n    helper();\n}\n"
    );

    let apply = rename_tool
        .call(
            test_context(&workspace, "rename-apply"),
            json!({
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 4,
                "newName": "renamed",
                "apply": true,
            }),
        )
        .await
        .expect("rename apply");
    assert!(apply.display_text.contains("Applied LSP rename"));
    assert_eq!(
        fs::read_to_string(workspace.join("src/lib.rs")).expect("read lib.rs after apply"),
        "fn renamed() {}\n\nfn caller() {\n    helper();\n}\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("src/other.rs")).expect("read other.rs after apply"),
        "fn another() {\n    renamed();\n}\n"
    );
    let apply_json = apply.structured_json.expect("rename apply structured json");
    assert_eq!(apply_json["applied"], json!(true));
    assert_eq!(apply_json["appliedEdits"].as_array().map(Vec::len), Some(2),);
    assert_eq!(apply.artifacts.len(), 2);
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_rename_reports_unsupported_server_behavior() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-rename-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake rename bin dir");
    let primary_uri = file_uri(&workspace.join("src/lib.rs"));
    let secondary_uri = file_uri(&workspace.join("src/other.rs"));
    install_fake_rename_lsp_binary(
        &fake_bin,
        "unsupported-rename-lsp",
        &primary_uri,
        &secondary_uri,
        "null",
        "documentChanges",
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec![fake_lsp_command(&fake_bin, "unsupported-rename-lsp")]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let rename_tool = registry.get("lsp.rename").expect("lsp.rename tool");
    let error = rename_tool
        .call(
            test_context(&workspace, "rename-unsupported"),
            json!({
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 4,
                "newName": "renamed",
                "apply": false,
            }),
        )
        .await
        .expect_err("unsupported rename server should fail");
    match error {
        ToolError::Execution(message) => assert!(
            message.contains("rename is unavailable"),
            "expected unsupported rename error, got {message:?}"
        ),
        other => panic!("expected execution error, got {other:?}"),
    }
}
