#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_direct_file_and_workspace_diagnostics() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-definition.rs",
            diagnostic_message: "rust direct diagnostic",
            empty_diagnostics: false,
            env_key: "RUST_LOG",
            env_value: "trace",
            expected_language_id: "rust",
            initialization_path: "cargo.allFeatures",
            initialization_value: "true",
        },
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec![fake_lsp_command(&fake_bin, "custom-rust-analyzer")]),
                extensions: None,
                env: BTreeMap::from([("RUST_LOG".to_string(), "trace".to_string())]),
                initialization: Some(json!({
                    "cargo": {
                        "allFeatures": true,
                    }
                })),
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let file_result = lsp
        .call(
            test_context(&workspace, "direct-file-diagnostics"),
            json!({
                "operation": "fileDiagnostics",
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect("file diagnostics request with reusable cursor metadata");
    assert!(file_result.display_text.contains("Diagnostics for"));
    assert!(file_result
        .display_text
        .contains("src/lib.rs:1:1 Error rust direct diagnostic"));
    let file_json = file_result
        .structured_json
        .clone()
        .expect("file diagnostics structured json");
    assert_eq!(file_json["result"]["scope"], json!("file"));
    assert_eq!(file_json["result"]["diagnosticCount"], json!(1));
    assert!(file_json["diagnostics"][0]["file_path"]
        .as_str()
        .expect("file diagnostic path")
        .ends_with("src/lib.rs"));
    assert!(
        first_diagnostic_message(&file_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let workspace_result = lsp
        .call(
            test_context(&workspace, "direct-workspace-diagnostics"),
            json!({
                "operation": "workspaceDiagnostics",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect("workspace diagnostics request");
    assert!(workspace_result.display_text.contains("Diagnostics for"));
    assert!(workspace_result
        .display_text
        .contains("src/lib.rs:1:1 Error rust direct diagnostic"));
    assert!(workspace_result
        .display_text
        .contains("src/extra.rs:1:1 Error rust direct diagnostic"));
    assert!(workspace_result
        .display_text
        .contains("src/other.rs:1:1 Error rust direct diagnostic"));
    let workspace_json = workspace_result
        .structured_json
        .expect("workspace diagnostics structured json");
    assert_eq!(workspace_json["result"]["scope"], json!("workspace"));
    assert_eq!(workspace_json["result"]["filesScanned"], json!(3));
    assert_eq!(workspace_json["result"]["diagnosticCount"], json!(3));
    assert_eq!(
        workspace_json["diagnostics"]
            .as_array()
            .expect("workspace diagnostics array")
            .len(),
        3
    );
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_reports_empty_direct_diagnostics_cleanly() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-definition.rs",
            diagnostic_message: "unused because diagnostics are empty",
            empty_diagnostics: true,
            env_key: "",
            env_value: "",
            expected_language_id: "rust",
            initialization_path: "",
            initialization_value: "",
        },
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec![fake_lsp_command(&fake_bin, "custom-rust-analyzer")]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let result = lsp
        .call(
            test_context(&workspace, "empty-file-diagnostics"),
            json!({
                "operation": "fileDiagnostics",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect("empty file diagnostics request");
    let expected_path = workspace.join("src/lib.rs").display().to_string();
    assert_eq!(
        result.display_text,
        format!("No diagnostics found for {expected_path}")
    );
    let result_json = result
        .structured_json
        .expect("empty file diagnostics structured json");
    assert_eq!(result_json["result"]["scope"], json!("file"));
    assert_eq!(result_json["result"]["diagnosticCount"], json!(0));
    assert_eq!(result_json["diagnostics"][0]["diagnostics"], json!([]));
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_rejects_disabled_or_unsupported_servers() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([
            (
                "rust".to_string(),
                LspServerConfig {
                    disabled: true,
                    command: None,
                    extensions: None,
                    env: BTreeMap::new(),
                    initialization: None,
                },
            ),
            (
                "custom-local".to_string(),
                LspServerConfig {
                    disabled: true,
                    command: Some(vec!["custom-local-lsp".to_string()]),
                    extensions: Some(vec![".foo".to_string()]),
                    env: BTreeMap::new(),
                    initialization: None,
                },
            ),
        ]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let disabled_rust = lsp
        .call(
            test_context(&workspace, "disabled-rust"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect_err("disabled rust should fail");
    expect_invalid_arguments(
        disabled_rust,
        "configured lsp server `rust` is disabled for extension .rs",
    );

    let disabled_custom = lsp
        .call(
            test_context(&workspace, "disabled-custom"),
            json!({
                "operation": "goToDefinition",
                "filePath": "custom/schema.foo",
                "line": 3,
                "character": 2,
            }),
        )
        .await
        .expect_err("disabled custom server should fail");
    expect_invalid_arguments(
        disabled_custom,
        "configured lsp server `custom-local` is disabled for extension .foo",
    );

    let unsupported_language = lsp
        .call(
            test_context(&workspace, "unsupported-language"),
            json!({
                "operation": "goToDefinition",
                "filePath": "unsupported.lua",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("unsupported language should fail");
    expect_invalid_arguments(
        unsupported_language,
        "unsupported lsp language extension: .lua",
    );
}
#[test]
fn native_code_lsp_rejects_disabled_or_unsupported_server_requests() {
    native_code_lsp_rejects_disabled_or_unsupported_servers();
}
#[tokio::test]
async fn native_code_lsp_validates_inputs_by_operation_shape() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let missing_position = lsp
        .call(
            test_context(&workspace, "missing-position"),
            json!({
                "operation": "hover",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect_err("hover should require cursor coordinates");
    expect_invalid_arguments(missing_position, "missing field `line`");

    let missing_query = lsp
        .call(
            test_context(&workspace, "missing-query"),
            json!({
                "operation": "workspaceSymbol",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect_err("workspaceSymbol should require a query");
    expect_invalid_arguments(missing_query, "missing field `query`");
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_non_position_operations_without_cursor_placeholders() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-symbol.rs",
            diagnostic_message: "rust symbol diagnostic",
            empty_diagnostics: false,
            env_key: "",
            env_value: "",
            expected_language_id: "rust",
            initialization_path: "",
            initialization_value: "",
        },
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec![fake_lsp_command(&fake_bin, "custom-rust-analyzer")]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let document_symbols = lsp
        .call(
            test_context(&workspace, "document-symbol"),
            json!({
                "operation": "documentSymbol",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect("documentSymbol request");
    let document_json = document_symbols
        .structured_json
        .clone()
        .expect("documentSymbol structured json");
    assert_eq!(document_json["operation"], json!("documentSymbol"));
    assert_eq!(
        document_json["filePath"],
        json!(workspace.join("src/lib.rs").display().to_string())
    );
    assert!(document_json.get("line").is_none());
    assert!(document_json.get("character").is_none());

    let workspace_symbols = lsp
        .call(
            test_context(&workspace, "workspace-symbol"),
            json!({
                "operation": "workspaceSymbol",
                "filePath": "src/lib.rs",
                "query": "helper",
            }),
        )
        .await
        .expect("workspaceSymbol request");
    let workspace_json = workspace_symbols
        .structured_json
        .expect("workspaceSymbol structured json");
    assert_eq!(workspace_json["operation"], json!("workspaceSymbol"));
    assert_eq!(workspace_json["query"], json!("helper"));
    assert_eq!(workspace_json["result"][0]["query"], json!("helper"));
    assert!(workspace_json.get("line").is_none());
    assert!(workspace_json.get("character").is_none());
}
#[tokio::test]
async fn native_code_lsp_rejects_unsupported_operation_cleanly() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let unsupported_operation = lsp
        .call(
            test_context(&workspace, "unsupported-operation"),
            json!({
                "operation": "renameSymbol",
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("unsupported operation should fail");
    expect_invalid_arguments(
        unsupported_operation,
        "use lsp.rename for the explicit workspace-editing rename flow",
    );

    let repeated_unsupported_operation = lsp
        .call(
            test_context(&workspace, "compat-unsupported-operation"),
            json!({
                "operation": "renameSymbol",
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("compat unsupported operation should fail");
    expect_invalid_arguments(
        repeated_unsupported_operation,
        "use lsp.rename for the explicit workspace-editing rename flow",
    );
}
