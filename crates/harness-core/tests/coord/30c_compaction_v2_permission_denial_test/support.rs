pub(super) struct DeniedExecutionTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for DeniedExecutionTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _context: ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::text("unexpected execution"))
    }
}

pub(super) struct DenialScenario {
    pub(super) continuation: CompletionRequest,
    pub(super) followup: CompletionRequest,
    pub(super) events: Vec<EventEnvelopeV1>,
    pub(super) denied_request_id: String,
    pub(super) tool_calls: usize,
}

pub(super) fn denial_coordinator(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
    calls: Arc<AtomicUsize>,
) -> CoordinatorHandle {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(DeniedExecutionTool { calls }));
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.provider = provider;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = ask_shell_permission_policy();
    let mut profile = agent_profiles().remove("default").unwrap_or_abort();
    profile.toolset = vec!["shell.run".to_string()];
    profile.max_iters = Some(8);
    profile.tool_failure_mode = harness_core::config::ToolFailureMode::ContinueAsToolMessage;
    config.agent_profiles = BTreeMap::from([("default".to_string(), profile)]);
    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

pub(super) async fn denial_turn(
    coordinator: &CoordinatorHandle,
    agent_id: &str,
    prompt: &str,
) -> String {
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, prompt)
        .await
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    event.payload,
                    EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();
    request_id
}

pub(super) async fn denied_tool_turn(
    coordinator: &CoordinatorHandle,
    agent_id: &str,
    prompt: &str,
) -> String {
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, prompt)
        .await
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() != Some(request_id.as_str()) {
                continue;
            }
            if let EventV1::PermissionRequested(permission) = &event.payload {
                coordinator
                    .resolve_permission(
                        permission.permission_id.clone(),
                        RuntimePermissionDecision::Deny,
                        Some("runtime owner denial".to_string()),
                    )
                    .await
                    .unwrap_or_abort();
            }
            if matches!(
                event.payload,
                EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
            ) {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();
    request_id
}

pub(super) async fn capture_denial_scenario() -> DenialScenario {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_denied".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done { usage: None },
        ],
        provider_text_events("continued after denied tool"),
        provider_text_events("followup answer"),
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let initial = denial_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        Arc::clone(&calls),
    );
    let run = initial
        .start_run(
            "compaction-v2-permission-denial",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let agent_id = initial
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let denied_request_id = denied_tool_turn(&initial, &agent_id, "deny this tool").await;
    let continuation = provider.requests()[1].clone();

    initial.stop_run().await.unwrap_or_abort();
    let resumed = denial_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        Arc::clone(&calls),
    );
    resumed
        .resume_run(run.run_id.as_str(), "compaction-v2-permission-denial")
        .await
        .unwrap_or_abort();
    denial_turn(&resumed, &agent_id, "inspect denied tool history").await;
    resumed.stop_run().await.unwrap_or_abort();

    DenialScenario {
        continuation,
        followup: provider.requests()[2].clone(),
        events: load_events(&run.events_path),
        denied_request_id,
        tool_calls: calls.load(Ordering::SeqCst),
    }
}

pub(super) fn assert_one_denied_pair(
    request: &CompletionRequest,
    tool_call_id: &str,
    surface: &str,
) {
    let call_indices = request
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message
                .assistant_tool_calls
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|call| call.tool_call_id == tool_call_id)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let result_indices = request
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.tool_call_id.as_deref() == Some(tool_call_id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(call_indices.len(), 1, "{surface} assistant call count");
    assert_eq!(result_indices.len(), 1, "{surface} failed result count");
    assert!(call_indices[0] < result_indices[0]);
    assert_eq!(request.messages[result_indices[0]].role, MessageRole::Tool);
    assert!(request.messages[result_indices[0]]
        .content
        .to_ascii_lowercase()
        .contains("deni"));
}

pub(super) fn only_assistant_tool_call_id(request: &CompletionRequest) -> String {
    let tool_call_ids = request
        .messages
        .iter()
        .flat_map(|message| message.assistant_tool_calls.as_deref().unwrap_or_default())
        .map(|call| call.tool_call_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(tool_call_ids.len(), 1);
    tool_call_ids[0].clone()
}
