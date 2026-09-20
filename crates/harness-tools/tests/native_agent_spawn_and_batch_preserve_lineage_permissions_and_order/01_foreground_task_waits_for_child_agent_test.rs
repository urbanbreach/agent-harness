use harness_tools::UnwrapOrAbort;

#[tokio::test]
async fn foreground_task_waits_for_child_agent_turn_after_child_tool_result() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let provider = Arc::new(ChildToolThenFinalProvider::new());
    let provider_clone = Arc::clone(&provider);
    let (handle, run, worker_id) = spawn_run_with_provider(&workspace, provider_clone).await;

    // act
    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Child reads first",
                "prompt": "Read fixture.txt, then report completion.",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);

    // assert
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    assert!(finished
        .output_summary
        .as_deref()
        .unwrap_or_abort()
        .contains("child final after read"));

    let output = finished.output_json.as_ref().unwrap_or_abort();
    assert_eq!(
        output.get("result_summary"),
        Some(&json!("child final after read"))
    );
    assert_eq!(
        output.pointer("/child_tool_call_counts/requested"),
        Some(&json!(1))
    );

    let child_request_id = output
        .get("child_request_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    let child_tool_finish_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(child_request_id)
                    && data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.task_scope)
                        == Some(TaskTerminalScope::ToolCall) =>
            {
                Some(event.seq)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let child_agent_finish_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(child_request_id)
                    && data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.task_scope)
                        == Some(TaskTerminalScope::AgentTurn) =>
            {
                Some(event.seq)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let parent_task_tool_finish_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == task_tool_call_id => {
                Some(event.seq)
            }
            _ => None,
        })
        .unwrap_or_abort();

    assert!(child_tool_finish_seq < child_agent_finish_seq);
    assert!(child_agent_finish_seq < parent_task_tool_finish_seq);
    assert_eq!(provider.requests().await.len(), 2);
}

#[tokio::test]
async fn generic_task_inherits_parent_turn_model_when_subagent_model_is_implicit() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let provider = Arc::new(TaskCallingProvider::default());
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    let provider_clone = Arc::clone(&provider);
    config.provider = provider_clone;
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref_explicit = false;
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

    // act
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_task_model_inheritance", &workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .unwrap_or_abort();
    let request_id = handle
        .request_agent_turn_with_model(
            anonymous_supervisor_actor(),
            worker_id,
            "delegate to default",
            Some("default:parent-model".to_string()),
            Some(AgentModelSettings {
                variant: Some("parent-variant".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
                thinking: None,
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_request_terminal(&run.events_path, &request_id).await;
    let requests = provider.requests().await;

    // assert
    assert!(requests.len() >= 2);
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "parent-model");
    assert_eq!(requests[1].variant.as_deref(), Some("parent-variant"));
    assert_eq!(requests[1].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(requests[1].text_verbosity.as_deref(), Some("low"));
    assert_eq!(requests[1].reasoning_summary.as_deref(), Some("auto"));
}

#[tokio::test]
async fn generic_task_keeps_explicit_subagent_model_over_parent_turn_model() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let provider = Arc::new(TaskCallingProvider::default());
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    let provider_clone = Arc::clone(&provider);
    config.provider = provider_clone;
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref = "default:generic-child".to_string();
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

    // act
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_task_model_override", &workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .unwrap_or_abort();
    let request_id = handle
        .request_agent_turn_with_model(
            anonymous_supervisor_actor(),
            worker_id,
            "delegate to default",
            Some("default:parent-model".to_string()),
            Some(AgentModelSettings {
                variant: Some("parent-variant".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
                thinking: None,
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_request_terminal(&run.events_path, &request_id).await;
    let requests = provider.requests().await;

    // assert
    assert!(requests.len() >= 2);
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "generic-child");
    assert_eq!(requests[1].variant, None);
    assert_eq!(requests[1].reasoning_effort, None);
    assert_eq!(requests[1].text_verbosity, None);
    assert_eq!(requests[1].reasoning_summary, None);
}

struct NestedDelegationProvider {
    script: Mutex<std::collections::VecDeque<Vec<ProviderStreamEvent>>>,
}

#[async_trait]
impl Provider for NestedDelegationProvider {
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

    async fn stream_completion(&self, _request: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::iter(
            self.script.lock().await.pop_front().unwrap_or_abort(),
        ))
    }
}

// Test-only compatibility spelling; production registry aliases remain unchanged.
struct DelegationAlias(Arc<dyn harness_core::tool::Tool>);

#[async_trait]
impl harness_core::tool::Tool for DelegationAlias {
    fn id(&self) -> &str {
        "agent.spawn"
    }
    fn capability(&self) -> harness_core::tool::ToolCapability {
        self.0.capability()
    }
    fn parameters_json_schema(&self) -> Value {
        self.0.parameters_json_schema()
    }
    async fn call(
        &self,
        context: harness_core::tool::ToolContext,
        args: Value,
    ) -> Result<harness_core::tool::ToolResult, harness_core::tool::ToolError> {
        self.0.call(context, args).await
    }
}

#[tokio::test]
async fn native_tool_scheduler_nested_foreground_delegation_completes_at_limit_one() {
    for (outer, inner) in [
        ("task", "task"),
        ("agent.spawn", "agent.spawn"),
        ("task", "agent.spawn"),
        ("agent.spawn", "task"),
        ("batch", "batch"),
    ] {
        assert_nested_foreground_delegation(outer, inner).await;
    }
}

async fn assert_nested_foreground_delegation(outer: &str, inner: &str) {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);
    let delegate = |wrapper: &str, prompt: &str| {
        let args = json!({"prompt": prompt, "subagent_type": "general",
            "run_in_background": false, "load_skills": []});
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: format!("call_{prompt}"),
            function_name: harness_core::tool::sanitize_tool_function_name(wrapper),
            arguments_json: if wrapper == "batch" {
                json!({"tool_calls": [{"tool": "task", "parameters": args}]}).to_string()
            } else {
                args.to_string()
            },
        }
    };
    let turn = |event| {
        vec![
            ProviderStreamEvent::Start,
            event,
            ProviderStreamEvent::Done { usage: None },
        ]
    };
    let provider = Arc::new(NestedDelegationProvider {
        script: Mutex::new(
            [
                turn(delegate(outer, "child")),
                turn(delegate(inner, "grandchild")),
                turn(ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "read_fixture".to_string(),
                    function_name: "read".to_string(),
                    arguments_json: json!({"filePath":"fixture.txt"}).to_string(),
                }),
                turn(ProviderStreamEvent::TextDelta(
                    "grandchild finished".to_string(),
                )),
                turn(ProviderStreamEvent::TextDelta("child finished".to_string())),
                turn(ProviderStreamEvent::TextDelta(
                    "parent finished".to_string(),
                )),
            ]
            .into(),
        ),
    });
    let mut registry = coordinator_registry(ShellAllowlist::default());
    registry.register(Arc::new(DelegationAlias(
        registry.get("task").unwrap_or_abort(),
    )));
    let mut general = named_worker_profile("general", &["task", "agent.spawn", "batch", "read"]);
    // This fixture explicitly allows another generation; production child defaults still deny it.
    general.permission_ruleset.push(PermissionRule {
        permission: "task".to_string(),
        pattern: "*".to_string(),
        action: PermissionAction::Allow,
    });
    let mut config = CoordinatorConfig::new(workspace.join("sessions"));
    config.provider_model_concurrency = 1;
    config.tool_concurrency = 1;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.provider = Arc::<NestedDelegationProvider>::clone(&provider);
    config.tool_registry = Arc::new(registry);
    config.agent_profiles = BTreeMap::from([
        (
            "default".to_string(),
            worker_profile(&["task", "agent.spawn", "batch", "read"]),
        ),
        ("general".to_string(), general),
    ]);
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("nested_foreground", &workspace)
        .await
        .unwrap_or_abort();
    let parent = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let mut stream = handle
        .event_store()
        .await
        .unwrap_or_abort()
        .subscribe(1)
        .unwrap_or_abort();
    let request = handle
        .request_agent_turn(anonymous_supervisor_actor(), parent.clone(), "delegate")
        .await
        .unwrap_or_abort();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            let event = event.unwrap_or_abort();
            if event.correlation_id.as_deref() != Some(request.as_str()) {
                continue;
            }
            let scope = match &event.payload {
                EventV1::TaskCompleted(data) => data
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope),
                EventV1::TaskCancelled(data) => data.task_scope,
                _ => None,
            };
            if scope == Some(TaskTerminalScope::AgentTurn) {
                return;
            }
        }
    })
    .await
    .unwrap_or_abort();
    let events = read_events(&run.events_path);
    let completed: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCompleted(data)
                if data.metadata.as_ref().and_then(|m| m.task_scope)
                    == Some(TaskTerminalScope::AgentTurn) =>
            {
                Some((
                    data.result_summary.as_str(),
                    event.actor.agent_id.as_deref().unwrap_or_abort(),
                ))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        completed
            .iter()
            .map(|(summary, _)| *summary)
            .collect::<Vec<_>>(),
        vec!["grandchild finished", "child finished", "parent finished"],
        "{outer} -> {inner}"
    );
    assert_eq!(completed[2].1, parent);
    for (child, parent) in [
        (completed[0].1, completed[1].1),
        (completed[1].1, completed[2].1),
    ] {
        assert!(events
            .iter()
            .any(|event| matches!(&event.payload, EventV1::AgentSpawned(data)
            if data.agent_id == child && data.parent_agent_id.as_deref() == Some(parent))));
    }
    assert!(provider.script.lock().await.is_empty());
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(_) | EventV1::StaleDetected(_)
    )));
    handle.stop_run().await.unwrap_or_abort();
}
