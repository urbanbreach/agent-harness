#[tokio::test]
#[ignore = "requires installed rust-analyzer and cargo; run explicitly for native LSP signoff"]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_rust_analyzer_detects_errors_and_confirms_fixes() {
    let _lock = test_lock().lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path();
    fs::create_dir(workspace.join("src")).unwrap_or_abort();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname=\"lsp-native-check\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap_or_abort();
    let server_log = temp.path().join("server.jsonl");
    let _config = LspConfigGuard::install(super::protocol::logged_protocol_config(
        "rust-analyzer",
        &server_log,
    ));
    let mut timings = Vec::new();
    let (handle, run, worker) = super::simulation::start_lsp_run(workspace).await;
    for (name, source, broken) in [
        ("syntax-error", "pub fn broken() { let = ; }\n", true),
        ("type-error", "pub fn broken() -> u32 { \"wrong\" }\n", true),
        ("fixed", "pub fn broken() -> u32 { 42 }\n", false),
    ] {
        fs::write(workspace.join("src/lib.rs"), source).unwrap_or_abort();
        let compiled = std::process::Command::new("cargo")
            .args(["check", "--offline", "--quiet"])
            .current_dir(workspace)
            .output()
            .unwrap_or_abort();
        assert_eq!(
            compiled.status.success(),
            !broken,
            "{name}: compiler oracle"
        );
        for operation in ["fileDiagnostics", "workspaceDiagnostics"] {
            let started = std::time::Instant::now();
            let result = handle
                .execute_agent_tool_call(
                    common::worker_actor(&worker),
                    None,
                    "lsp",
                    json!({
                        "operation": operation, "filePath": "src/lib.rs",
                    }),
                )
                .await
                .unwrap_or_abort();
            timings.push(json!({"scenario": name, "operation": operation, "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0}));
            let has_error = result.structured_json.as_ref().unwrap_or_abort()["diagnostics"]
                .as_array()
                .unwrap_or_abort()
                .iter()
                .flat_map(|report| report["diagnostics"].as_array().unwrap_or_abort())
                .any(|diagnostic| diagnostic["severity"] == 1);
            assert_eq!(
                has_error, broken,
                "{name}/{operation}: {}",
                result.display_text
            );
            println!("{name}/{operation}: {}", result.display_text);
        }
    }
    let servers = super::protocol::protocol_log(&server_log);
    assert_eq!(
        servers.len(),
        1,
        "all six checks must share one rust-analyzer process"
    );
    handle.stop_run().await.unwrap_or_abort();
    #[cfg(target_os = "linux")]
    super::protocol::wait_for_process_exit(servers[0]["pid"].as_u64().unwrap_or_abort()).await;
    let timing_report =
        json!({"optimized": !cfg!(debug_assertions), "processes": servers.len(), "calls": timings});
    println!("LSP_LATENCY {timing_report}");
    super::simulation::record_lsp_evidence(&run, "native");
    if let Some(root) = std::env::var_os("HARNESS_LSP_EVIDENCE_DIR") {
        fs::write(
            std::path::PathBuf::from(root).join("native/latency.json"),
            serde_json::to_vec_pretty(&timing_report).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }
}
