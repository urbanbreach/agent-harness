pub(super) fn protocol_server_config(mode: &str) -> LspConfig {
    LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                command: Some(vec![
                    "python3".to_string(),
                    format!(
                        "{}/tests/fixtures/lsp_protocol_server.py",
                        env!("CARGO_MANIFEST_DIR")
                    ),
                    mode.to_string(),
                ]),
                ..LspServerConfig::default()
            },
        )]),
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_consumes_pull_reports_and_checks_workspace_files() {
    let _lock = test_lock().lock().unwrap_or_abort();
    let temp = setup_workspace();
    let workspace = temp.path().join("workspace");
    fs::write(workspace.join("src/lib.rs"), "BROKEN\n").unwrap_or_abort();
    let _config = LspConfigGuard::install(protocol_server_config("pull"));
    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry.get("lsp").unwrap_or_abort();
    for operation in ["fileDiagnostics", "workspaceDiagnostics"] {
        let result = tool
            .call(
                test_context(&workspace, operation),
                json!({
                    "operation": operation, "filePath": "src/lib.rs",
                }),
            )
            .await
            .unwrap_or_abort();
        assert!(
            result.display_text.contains("Broken source detected"),
            "{operation}: {}",
            result.display_text
        );
        assert_eq!(
            result.structured_json.unwrap_or_abort()["result"]["diagnosticCount"],
            1
        );
    }
    fs::write(workspace.join("src/lib.rs"), "pub fn fixed() {}\n").unwrap_or_abort();
    let clean = tool
        .call(
            test_context(&workspace, "fixed"),
            json!({
                "operation": "fileDiagnostics", "filePath": "src/lib.rs",
            }),
        )
        .await
        .unwrap_or_abort();
    assert!(clean.display_text.contains("No diagnostics found"));
    for index in 0..201 {
        fs::write(workspace.join(format!("src/generated_{index}.rs")), "").unwrap_or_abort();
    }
    let oversized = tool
        .call(
            test_context(&workspace, "oversized"),
            json!({
                "operation": "workspaceDiagnostics", "filePath": "src/lib.rs",
            }),
        )
        .await;
    assert!(
        matches!(oversized, Err(harness_core::tool::ToolError::Execution(message)) if message.contains("use fileDiagnostics"))
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_waits_for_current_push_diagnostics_and_method_fallback() {
    let _lock = test_lock().lock().unwrap_or_abort();
    let temp = setup_workspace();
    let workspace = temp.path().join("workspace");
    fs::write(workspace.join("src/lib.rs"), "BROKEN\n").unwrap_or_abort();
    for mode in ["push", "versionless", "fallback", "cold"] {
        let _config = LspConfigGuard::install(protocol_server_config(mode));
        let registry = coordinator_registry(ShellAllowlist::default());
        let result = registry
            .get("lsp")
            .unwrap_or_abort()
            .call(
                test_context(&workspace, mode),
                json!({
                    "operation": "fileDiagnostics", "filePath": "src/lib.rs",
                }),
            )
            .await
            .unwrap_or_abort();
        assert!(
            result.display_text.contains("Broken source detected"),
            "{mode}: {}",
            result.display_text
        );
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_never_reports_invalid_or_disconnected_diagnostics_as_clean() {
    let _lock = test_lock().lock().unwrap_or_abort();
    let temp = setup_workspace();
    let workspace = temp.path().join("workspace");
    for mode in ["unchanged", "malformed", "close", "mutate"] {
        let _config = LspConfigGuard::install(protocol_server_config(mode));
        let registry = coordinator_registry(ShellAllowlist::default());
        let result = registry
            .get("lsp")
            .unwrap_or_abort()
            .call(
                test_context(&workspace, mode),
                json!({
                    "operation": "fileDiagnostics", "filePath": "src/lib.rs",
                }),
            )
            .await;
        assert!(
            result.is_err(),
            "{mode} must be unavailable, got {result:?}"
        );
    }
}

pub(super) fn logged_protocol_config(mode: &str, log: &Path) -> LspConfig {
    let mut config = protocol_server_config(mode);
    config.servers.get_mut("rust").unwrap_or_abort().env =
        BTreeMap::from([("LSP_PROTOCOL_LOG".to_string(), log.display().to_string())]);
    config
}

pub(super) fn protocol_log(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub(super) async fn wait_for_protocol(path: &Path, method: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !protocol_log(path)
            .iter()
            .any(|entry| entry["method"] == method || entry["id"] == method)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort();
}

#[cfg(target_os = "linux")]
pub(super) async fn wait_for_process_exit(pid: u64) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort();
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_reuses_servers_for_concurrent_calls_and_versioned_edits() {
    let _lock = test_lock().lock().unwrap_or_abort();
    for mode in ["pull", "cached", "push", "versionless", "fallback"] {
        let temp = setup_workspace();
        let workspace = temp.path().join("workspace");
        let log = temp.path().join("protocol.jsonl");
        let _config = LspConfigGuard::install(logged_protocol_config(mode, &log));
        let registry = coordinator_registry(ShellAllowlist::default());
        let tool = registry.get("lsp").unwrap_or_abort();
        let context = test_context(&workspace, "persistent");
        let args = json!({"operation": "fileDiagnostics", "filePath": "src/lib.rs"});
        fs::write(workspace.join("src/lib.rs"), "BROKEN\n").unwrap_or_abort();
        let concurrent = tokio::join!(
            tool.call(context.clone(), args.clone()),
            tool.call(context.clone(), args.clone()),
            tool.call(context.clone(), args.clone()),
        );
        for result in <[_; 3]>::from(concurrent) {
            assert!(result
                .unwrap_or_abort()
                .display_text
                .contains("Broken source detected"));
        }
        for (source, broken) in [
            ("fixed", false),
            ("BROKEN again", true),
            ("fixed again", false),
        ] {
            fs::write(workspace.join("src/lib.rs"), source).unwrap_or_abort();
            let result = tool
                .call(context.clone(), args.clone())
                .await
                .unwrap_or_abort();
            assert_eq!(
                result.display_text.contains("Broken source detected"),
                broken,
                "{mode}: {}",
                result.display_text
            );
        }
        if mode == "cached" {
            let events = protocol_log(&log);
            let last = events
                .iter()
                .rev()
                .find(|entry| entry["method"] == "textDocument/diagnostic")
                .unwrap_or_abort();
            wait_for_protocol(&log, &format!("idle-{}", last["id"])).await;
        }
        let hover = tool
            .call(
                context.clone(),
                json!({"operation": "hover", "filePath": "src/lib.rs", "line": 1, "character": 1}),
            )
            .await
            .unwrap_or_abort();
        assert!(hover.display_text.contains("No results found"));
        registry.get("lsp.rename").unwrap_or_abort().call(context.clone(), json!({"filePath": "src/lib.rs", "line": 1, "character": 1, "newName": "renamed", "apply": false})).await.unwrap_or_abort();
        let events = protocol_log(&log);
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["method"] == "textDocument/hover")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["method"] == "initialize")
                .count(),
            1,
            "{mode}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["method"] == "textDocument/didOpen")
                .count(),
            1,
            "{mode}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["method"] == "textDocument/didChange")
                .map(|entry| entry["params"]["textDocument"]["version"]
                    .as_i64()
                    .unwrap_or_abort())
                .collect::<Vec<_>>(),
            vec![1, 2, 3],
            "{mode}"
        );
        if mode == "cached" {
            assert!(events
                .iter()
                .any(|entry| entry["params"]["previousResultId"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("0:"))));
        }
        let pid = events[0]["pid"].as_u64().unwrap_or_abort();
        drop(context);
        #[cfg(target_os = "linux")]
        wait_for_process_exit(pid).await;
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_recovers_after_crash_and_cancels_initialization_or_requests() {
    let _lock = test_lock().lock().unwrap_or_abort();
    for mode in ["exit-once", "hang-initialize", "hang", "hang-write"] {
        let temp = setup_workspace();
        let workspace = temp.path().join("workspace");
        let log = temp.path().join("protocol.jsonl");
        let _config = LspConfigGuard::install(logged_protocol_config(mode, &log));
        let registry = coordinator_registry(ShellAllowlist::default());
        let tool = registry.get("lsp").unwrap_or_abort();
        let context = test_context(&workspace, "recovery");
        let args = json!({"operation": "fileDiagnostics", "filePath": "src/lib.rs"});
        if mode == "hang-write" {
            fs::write(workspace.join("src/lib.rs"), "x".repeat(1024 * 1024)).unwrap_or_abort();
        }
        if mode == "exit-once" {
            assert!(tool.call(context.clone(), args.clone()).await.is_err());
            let result = tool.call(context.clone(), args).await.unwrap_or_abort();
            assert!(result.display_text.contains("No diagnostics found"));
            assert_eq!(
                protocol_log(&log)
                    .iter()
                    .filter(|entry| entry["method"] == "initialize")
                    .count(),
                2
            );
            continue;
        }
        let call_context = context.clone();
        let operation = tokio::spawn(async move { tool.call(call_context, args).await });
        wait_for_protocol(
            &log,
            if mode == "hang-initialize" {
                "initialize"
            } else if mode == "hang-write" {
                "blocked-write"
            } else {
                "textDocument/diagnostic"
            },
        )
        .await;
        let pid = protocol_log(&log)[0]["pid"].as_u64().unwrap_or_abort();
        {
            let _other_config = LspConfigGuard::install(logged_protocol_config(
                "pull",
                &temp.path().join("other.jsonl"),
            ));
            let other = registry.get("lsp").unwrap_or_abort();
            assert!(other
                .call(
                    context.clone(),
                    json!({"operation": "fileDiagnostics", "filePath": "src/extra.rs"})
                )
                .await
                .is_ok());
        }
        operation.abort();
        assert!(operation.await.unwrap_err().is_cancelled());
        // The run remains alive: cancellation must kill the blocked server by itself.
        #[cfg(target_os = "linux")]
        wait_for_process_exit(pid).await;
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_bounds_project_pool_and_closes_deleted_or_redirected_documents() {
    let _lock = test_lock().lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path();
    let log = workspace.join("protocol.jsonl");
    let _config = LspConfigGuard::install(logged_protocol_config("pull", &log));
    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry.get("lsp").unwrap_or_abort();
    let context = test_context(workspace, "bounded-pool");
    for index in 0..7 {
        let root = workspace.join(format!("project{index}"));
        fs::create_dir_all(&root).unwrap_or_abort();
        fs::write(root.join("Cargo.toml"), "").unwrap_or_abort();
        fs::write(root.join("lib.rs"), "fixed").unwrap_or_abort();
        tool.call(
            context.clone(),
            json!({"operation": "fileDiagnostics", "filePath": format!("project{index}/lib.rs")}),
        )
        .await
        .unwrap_or_abort();
    }
    let starts = protocol_log(&log)
        .into_iter()
        .filter(|entry| entry["method"] == "initialize")
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 7);
    #[cfg(target_os = "linux")]
    wait_for_process_exit(starts[0]["pid"].as_u64().unwrap_or_abort()).await;
    // Opening beyond the buffer limit must close old documents, then reopen with a new version.
    for index in 0..200 {
        let file = format!("project6/z{index:03}.rs");
        fs::write(workspace.join(&file), "fixed").unwrap_or_abort();
        tool.call(
            context.clone(),
            json!({"operation": "fileDiagnostics", "filePath": file}),
        )
        .await
        .unwrap_or_abort();
    }
    tool.call(
        context.clone(),
        json!({"operation": "fileDiagnostics", "filePath": "project6/lib.rs"}),
    )
    .await
    .unwrap_or_abort();
    let events = protocol_log(&log);
    let opened = events
        .iter()
        .filter(|entry| {
            entry["method"] == "textDocument/didOpen"
                && entry["params"]["textDocument"]["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.ends_with("/project6/lib.rs"))
        })
        .collect::<Vec<_>>();
    assert_eq!(opened.len(), 2);
    assert!(
        opened[1]["params"]["textDocument"]["version"]
            .as_i64()
            .unwrap_or_abort()
            > 0
    );
    assert_eq!(
        events
            .iter()
            .filter(|entry| entry["method"] == "initialize")
            .count(),
        7
    );
    let root = workspace.join("project6");
    let redirected = root.join("lib.rs");
    fs::remove_file(&redirected).unwrap_or_abort();
    let outside = tempfile::NamedTempFile::new().unwrap_or_abort();
    fs::write(outside.path(), "MUST_NOT_READ_REDIRECTED_DOCUMENT").unwrap_or_abort();
    std::os::unix::fs::symlink(outside.path(), &redirected).unwrap_or_abort();
    fs::write(root.join("other.rs"), "fixed").unwrap_or_abort();
    let args = json!({"operation": "fileDiagnostics", "filePath": "project6/other.rs"});
    let result = tool.call(context.clone(), args.clone()).await;
    assert!(result.is_ok(), "closing a redirected document: {result:?}");
    assert!(protocol_log(&log)
        .iter()
        .any(|entry| entry["method"] == "textDocument/didClose"));
    assert!(!fs::read_to_string(&log)
        .unwrap_or_abort()
        .contains("MUST_NOT_READ_REDIRECTED_DOCUMENT"));
    assert_eq!(
        protocol_log(&log)
            .iter()
            .filter(|entry| entry["method"] == "initialize")
            .count(),
        7
    );
    let mut changed_config = logged_protocol_config("pull", &log);
    changed_config
        .servers
        .get_mut("rust")
        .unwrap_or_abort()
        .initialization = Some(json!({"newConfiguration": true}));
    let _changed = LspConfigGuard::install(changed_config);
    tool.call(context.clone(), args).await.unwrap_or_abort();
    assert_eq!(
        protocol_log(&log)
            .iter()
            .filter(|entry| entry["method"] == "initialize")
            .count(),
        8
    );
    drop(context);
    #[cfg(target_os = "linux")]
    for entry in protocol_log(&log)
        .iter()
        .filter(|entry| entry["method"] == "initialize")
    {
        wait_for_process_exit(entry["pid"].as_u64().unwrap_or_abort()).await;
    }
}
