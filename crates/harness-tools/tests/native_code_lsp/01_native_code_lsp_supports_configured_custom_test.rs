#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_configured_custom_servers() {
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
            diagnostic_message: "rust override diagnostic",
            empty_diagnostics: false,
            env_key: "RUST_LOG",
            env_value: "trace",
            expected_language_id: "rust",
            initialization_path: "cargo.allFeatures",
            initialization_value: "true",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-ts-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/typescript-definition.ts",
            diagnostic_message: "typescript override diagnostic",
            empty_diagnostics: false,
            env_key: "TS_SERVER_LOG",
            env_value: "verbose",
            expected_language_id: "typescript",
            initialization_path: "preferences.importModuleSpecifierPreference",
            initialization_value: "relative",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-local-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/custom-definition.foo",
            diagnostic_message: "custom server diagnostic",
            empty_diagnostics: false,
            env_key: "CUSTOM_LSP_MODE",
            env_value: "enabled",
            expected_language_id: "foo",
            initialization_path: "feature.mode",
            initialization_value: "custom",
        },
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([
            (
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
            ),
            (
                "typescript".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec![fake_lsp_command(&fake_bin, "custom-ts-lsp")]),
                    extensions: None,
                    env: BTreeMap::from([("TS_SERVER_LOG".to_string(), "verbose".to_string())]),
                    initialization: Some(json!({
                        "preferences": {
                            "importModuleSpecifierPreference": "relative",
                        }
                    })),
                },
            ),
            (
                "custom-local".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec![fake_lsp_command(&fake_bin, "custom-local-lsp")]),
                    extensions: Some(vec![".foo".to_string()]),
                    env: BTreeMap::from([("CUSTOM_LSP_MODE".to_string(), "enabled".to_string())]),
                    initialization: Some(json!({
                        "feature": {
                            "mode": "custom",
                        }
                    })),
                },
            ),
        ]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let rust_result = lsp
        .call(
            test_context(&workspace, "configured-rust"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("configured rust request");
    assert!(rust_result.display_text.contains("rust-definition.rs"));
    assert!(rust_result.display_text.contains("Diagnostics:"));
    assert!(rust_result
        .display_text
        .contains("rust override diagnostic"));
    let rust_json = rust_result
        .structured_json
        .clone()
        .expect("configured rust structured json");
    assert_eq!(rust_json["server"]["name"], json!("rust"));
    assert_server_command(&rust_json, &["custom-rust-analyzer"]);
    assert!(
        first_diagnostic_message(&rust_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let ts_result = lsp
        .call(
            test_context(&workspace, "configured-typescript"),
            json!({
                "operation": "goToDefinition",
                "filePath": "web/app.ts",
                "line": 2,
                "character": 9,
            }),
        )
        .await
        .expect("configured typescript request");
    assert!(ts_result
        .display_text
        .contains("typescript override diagnostic"));
    let ts_json = ts_result
        .structured_json
        .clone()
        .expect("configured typescript structured json");
    assert_eq!(ts_json["server"]["name"], json!("typescript"));
    assert_server_command(&ts_json, &["custom-ts-lsp"]);
    assert!(first_diagnostic_message(&ts_json).contains("env_ok=True; lang_ok=True; init_ok=True"));

    let custom_result = lsp
        .call(
            test_context(&workspace, "configured-custom"),
            json!({
                "operation": "goToDefinition",
                "filePath": "custom/schema.foo",
                "line": 3,
                "character": 2,
            }),
        )
        .await
        .expect("configured custom request");
    assert!(custom_result.display_text.contains("custom-definition.foo"));
    assert!(custom_result
        .display_text
        .contains("custom server diagnostic"));
    let custom_json = custom_result
        .structured_json
        .expect("configured custom structured json");
    assert_eq!(custom_json["server"]["name"], json!("custom-local"));
    assert_server_command(&custom_json, &["custom-local-lsp"]);
    assert!(custom_json["diagnostics"][0]["file_path"]
        .as_str()
        .expect("custom diagnostic file path")
        .ends_with("custom/schema.foo"));
    assert!(
        first_diagnostic_message(&custom_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_returns_empty_definition_without_retry_loop() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    let counter_path = temp_dir.path().join("definition-count.txt");
    install_empty_definition_lsp_binary(&bin_dir, "custom-rust-analyzer", &counter_path);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec![fake_lsp_command(&bin_dir, "custom-rust-analyzer")]),
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
            test_context(&workspace, "empty-definition"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("empty definition request should succeed");

    assert_eq!(result.display_text, "No results found for goToDefinition");
    assert_eq!(
        fs::read_to_string(&counter_path).expect("read counter"),
        "1"
    );
}
#[test]
fn native_code_lsp_supports_configured_builtin_and_custom_servers() {
    native_code_lsp_supports_configured_custom_servers();
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_additional_builtin_server_presets() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-python-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/python-definition.py",
            diagnostic_message: "python preset diagnostic",
            empty_diagnostics: false,
            env_key: "PYRIGHT_PYTHON_FORCE_VERSION",
            env_value: "latest",
            expected_language_id: "python",
            initialization_path: "python.analysis.typeCheckingMode",
            initialization_value: "basic",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-go-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/go-definition.go",
            diagnostic_message: "go preset diagnostic",
            empty_diagnostics: false,
            env_key: "GOPLS_LOG",
            env_value: "debug",
            expected_language_id: "go",
            initialization_path: "gopls.staticcheck",
            initialization_value: "true",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-json-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/json-definition.json",
            diagnostic_message: "json preset diagnostic",
            empty_diagnostics: false,
            env_key: "JSON_LSP_TRACE",
            env_value: "verbose",
            expected_language_id: "json",
            initialization_path: "json.validate.enable",
            initialization_value: "true",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-yaml-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/yaml-definition.yaml",
            diagnostic_message: "yaml preset diagnostic",
            empty_diagnostics: false,
            env_key: "YAML_LSP_TRACE",
            env_value: "verbose",
            expected_language_id: "yaml",
            initialization_path: "yaml.keyOrdering",
            initialization_value: "false",
        },
    );
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([
            (
                "python".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec![fake_lsp_command(&fake_bin, "custom-python-lsp"), "--stdio".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([(
                        "PYRIGHT_PYTHON_FORCE_VERSION".to_string(),
                        "latest".to_string(),
                    )]),
                    initialization: Some(json!({
                        "python": {
                            "analysis": {
                                "typeCheckingMode": "basic",
                            }
                        }
                    })),
                },
            ),
            (
                "go".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec![fake_lsp_command(&fake_bin, "custom-go-lsp")]),
                    extensions: None,
                    env: BTreeMap::from([("GOPLS_LOG".to_string(), "debug".to_string())]),
                    initialization: Some(json!({
                        "gopls": {
                            "staticcheck": true,
                        }
                    })),
                },
            ),
            (
                "json".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec![fake_lsp_command(&fake_bin, "custom-json-lsp"), "--stdio".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("JSON_LSP_TRACE".to_string(), "verbose".to_string())]),
                    initialization: Some(json!({
                        "json": {
                            "validate": {
                                "enable": true,
                            }
                        }
                    })),
                },
            ),
            (
                "yaml".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec![fake_lsp_command(&fake_bin, "custom-yaml-lsp"), "--stdio".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("YAML_LSP_TRACE".to_string(), "verbose".to_string())]),
                    initialization: Some(json!({
                        "yaml": {
                            "keyOrdering": false,
                        }
                    })),
                },
            ),
        ]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let python_result = lsp
        .call(
            test_context(&workspace, "builtin-python"),
            json!({
                "operation": "goToDefinition",
                "filePath": "python/app.py",
                "line": 4,
                "character": 12,
            }),
        )
        .await
        .expect("python preset request");
    let python_json = python_result
        .structured_json
        .clone()
        .expect("python structured json");
    assert_eq!(python_json["server"]["name"], json!("python"));
    assert_server_command(&python_json, &["custom-python-lsp", "--stdio"]);
    assert!(
        first_diagnostic_message(&python_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let go_result = lsp
        .call(
            test_context(&workspace, "builtin-go"),
            json!({
                "operation": "goToDefinition",
                "filePath": "go/main.go",
                "line": 5,
                "character": 2,
            }),
        )
        .await
        .expect("go preset request");
    let go_json = go_result
        .structured_json
        .clone()
        .expect("go structured json");
    assert_eq!(go_json["server"]["name"], json!("go"));
    assert_server_command(&go_json, &["custom-go-lsp"]);
    assert!(first_diagnostic_message(&go_json).contains("env_ok=True; lang_ok=True; init_ok=True"));

    let json_result = lsp
        .call(
            test_context(&workspace, "builtin-json"),
            json!({
                "operation": "hover",
                "filePath": "config/settings.json",
                "line": 2,
                "character": 4,
            }),
        )
        .await
        .expect("json preset request");
    let json_json = json_result
        .structured_json
        .clone()
        .expect("json structured json");
    assert_eq!(json_json["server"]["name"], json!("json"));
    assert_server_command(&json_json, &["custom-json-lsp", "--stdio"]);
    assert!(
        first_diagnostic_message(&json_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let yaml_result = lsp
        .call(
            test_context(&workspace, "builtin-yaml"),
            json!({
                "operation": "documentSymbol",
                "filePath": "config/service.yaml",
            }),
        )
        .await
        .expect("yaml preset request");
    let yaml_json = yaml_result.structured_json.expect("yaml structured json");
    assert_eq!(yaml_json["server"]["name"], json!("yaml"));
    assert_server_command(&yaml_json, &["custom-yaml-lsp", "--stdio"]);
    assert!(
        first_diagnostic_message(&yaml_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );
}
