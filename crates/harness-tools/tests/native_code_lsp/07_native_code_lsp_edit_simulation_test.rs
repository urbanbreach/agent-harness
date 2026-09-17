use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo},
    redact::DefaultRedactor,
};
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use std::sync::Arc;

struct VerificationProvider;

#[async_trait::async_trait]
impl Provider for VerificationProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        assert!(request
            .tools
            .as_ref()
            .unwrap_or_abort()
            .iter()
            .any(|tool| tool.tool_id == "lsp"));
        Box::pin(tokio_stream::iter([
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(
                "LSP verification: errors, fixes, and unavailable checks.".to_string(),
            ),
            ProviderStreamEvent::Done { usage: None },
        ]))
    }
}

pub(super) async fn start_lsp_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    let mut config = CoordinatorConfig::new(workspace.join("sessions"));
    config.deterministic_store = true;
    config.provider = Arc::new(VerificationProvider);
    config.permission_policy = common::allow_all_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles.insert(
        "worker".to_string(),
        AgentProfile {
            name: "worker".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "Check changed files with lsp.".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: ["write", "edit", "apply_patch", "lsp"]
                .map(str::to_string)
                .to_vec(),
            permission_ruleset: Vec::new(),
        },
    );
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("lsp-verification", workspace)
        .await
        .unwrap_or_abort();
    let worker = handle
        .spawn_agent(common::anonymous_supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();
    (handle, run, worker)
}

pub(super) fn record_lsp_evidence(run: &RunInfo, name: &str) {
    let Some(root) = std::env::var_os("HARNESS_LSP_EVIDENCE_DIR") else {
        return;
    };
    let root = std::path::PathBuf::from(root).join(name);
    fs::create_dir_all(root.join("artifacts")).unwrap_or_abort();
    fs::copy(&run.events_path, root.join("events.jsonl")).unwrap_or_abort();
    for entry in fs::read_dir(&run.artifacts_dir).unwrap_or_abort() {
        let entry = entry.unwrap_or_abort();
        if entry.file_type().unwrap_or_abort().is_file() {
            fs::copy(entry.path(), root.join("artifacts").join(entry.file_name()))
                .unwrap_or_abort();
        }
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "serializes the process-local LSP configuration"
)]
async fn native_lsp_edit_simulation_reports_errors_fixes_and_unavailability() {
    let _lock = test_lock().lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path();
    fs::create_dir(workspace.join("src")).unwrap_or_abort();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname=\"lsp-simulation\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap_or_abort();
    let server_log = workspace.join("protocol.jsonl");
    let _config = LspConfigGuard::install(super::protocol::logged_protocol_config(
        "cached",
        &server_log,
    ));
    let (handle, run, worker) = start_lsp_run(workspace).await;
    let calls = [
        (
            "write",
            json!({"path":"src/lib.rs", "content":"BROKEN\n"}),
            true,
        ),
        (
            "edit",
            json!({"path":"src/lib.rs", "oldString":"BROKEN", "newString":"pub fn fixed() {}"}),
            false,
        ),
        (
            "apply_patch",
            json!({"patchText":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-pub fn fixed() {}\n+BROKEN\n*** End Patch"}),
            true,
        ),
        (
            "edit",
            json!({"path":"src/lib.rs", "edits":[{"op":"replace", "pos":format!("1#{}", harness_core::edit::hashline::compute_line_hash("BROKEN")), "lines":["pub fn fixed() {}"]}]}),
            false,
        ),
    ];
    for (tool, args, broken) in calls {
        let result = handle
            .execute_agent_tool_call(common::worker_actor(&worker), None, tool, args)
            .await
            .unwrap_or_abort();
        assert_eq!(
            result.display_text.contains("Broken source detected"),
            broken,
            "{tool}: {}",
            result.display_text
        );
        assert_eq!(
            result.display_text.contains("no issues found"),
            !broken,
            "{tool}: {}",
            result.display_text
        );
    }
    let sibling = handle
        .spawn_agent(common::anonymous_supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();
    let clean = handle
        .execute_agent_tool_call(
            common::worker_actor(&sibling),
            None,
            "lsp",
            json!({"operation":"fileDiagnostics", "filePath":"src/lib.rs"}),
        )
        .await
        .unwrap_or_abort();
    assert!(clean.display_text.contains("No diagnostics found"));

    let protocol = super::protocol::protocol_log(&server_log);
    assert_eq!(
        protocol
            .iter()
            .filter(|entry| entry["method"] == "initialize")
            .count(),
        1,
        "edit checks and another agent must share the server"
    );
    assert_eq!(
        protocol
            .iter()
            .filter(|entry| entry["method"] == "textDocument/didChange")
            .count(),
        3
    );

    let mut unavailable = super::protocol::protocol_server_config("pull");
    unavailable
        .servers
        .get_mut("rust")
        .unwrap_or_abort()
        .command = Some(vec![workspace
        .join("missing-language-server")
        .display()
        .to_string()]);
    let _unavailable = LspConfigGuard::install(unavailable);
    let result = handle
        .execute_agent_tool_call(
            common::worker_actor(&worker),
            None,
            "edit",
            json!({"path":"src/lib.rs", "oldString":"fixed", "newString":"saved"}),
        )
        .await
        .unwrap_or_abort();
    assert!(result.display_text.contains("LSP diagnostics unavailable"));
    assert_eq!(
        result.structured_json.unwrap_or_abort()["diagnostics"]["unavailable"],
        true
    );
    assert!(fs::read_to_string(workspace.join("src/lib.rs"))
        .unwrap_or_abort()
        .contains("saved"));
    assert!(handle
        .execute_agent_tool_call(
            common::worker_actor(&worker),
            None,
            "lsp",
            json!({"operation":"fileDiagnostics", "filePath":"src/lib.rs"})
        )
        .await
        .is_err());
    handle.stop_run().await.unwrap_or_abort();
    #[cfg(target_os = "linux")]
    super::protocol::wait_for_process_exit(protocol[0]["pid"].as_u64().unwrap_or_abort()).await;
    let events = common::read_events(&run.events_path);
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        harness_core::event::EventV1::ProviderRequestFinished(data) if data.finish_reason != "error"
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                harness_core::event::EventV1::ToolCallFinished(_)
            ))
            .count(),
        7
    );
    record_lsp_evidence(&run, "simulation");
}
