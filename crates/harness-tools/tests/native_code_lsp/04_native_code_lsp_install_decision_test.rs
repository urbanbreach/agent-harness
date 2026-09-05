use harness_tools::UnwrapOrAbort;
use std::fs;

#[tokio::test]
async fn native_code_lsp_install_decision_records_allowed() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").unwrap_or_abort();

    let result = lsp
        .call(
            test_context(&workspace, "install-decision-allowed"),
            json!({
                "operation": "installDecision",
                "serverId": "rust",
                "decision": "allowed",
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(
        result
            .display_text
            .contains("Recorded decision 'allowed' for LSP server 'rust'"),
        "expected confirmation message, got: {}",
        result.display_text
    );

    let artifact_path = workspace.join("artifacts").join("lsp-install-decisions.json");
    let content = fs::read_to_string(&artifact_path).unwrap_or_abort();
    let decisions: serde_json::Value = serde_json::from_str(&content).unwrap_or_abort();
    assert_eq!(decisions["rust"], json!("allowed"));
}

#[tokio::test]
async fn native_code_lsp_install_decision_records_declined_and_merges() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let artifacts_dir = workspace.join("artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap_or_abort();

    // Pre-write an existing decision to verify merge behavior.
    let artifact_path = artifacts_dir.join("lsp-install-decisions.json");
    fs::write(
        &artifact_path,
        serde_json::to_string_pretty(&json!({"typescript": "allowed"})).unwrap_or_abort(),
    )
    .unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").unwrap_or_abort();

    let result = lsp
        .call(
            test_context(&workspace, "install-decision-declined"),
            json!({
                "operation": "installDecision",
                "serverId": "rust",
                "decision": "declined",
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(
        result
            .display_text
            .contains("Recorded decision 'declined' for LSP server 'rust'"),
        "expected confirmation message, got: {}",
        result.display_text
    );

    // Verify merge: both the pre-existing and new decision should be present.
    let content = fs::read_to_string(&artifact_path).unwrap_or_abort();
    let decisions: serde_json::Value = serde_json::from_str(&content).unwrap_or_abort();
    assert_eq!(decisions["typescript"], json!("allowed"));
    assert_eq!(decisions["rust"], json!("declined"));
}
