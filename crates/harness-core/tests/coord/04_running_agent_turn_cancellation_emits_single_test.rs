#[tokio::test]
async fn running_agent_turn_cancellation_emits_single_owner_aware_terminal_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(100));

    let run = coordinator
        .start_run(
            "coord_agent_turn_cancel_running",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .expect("request running turn");

    let task_id = load_events(&run.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some(data.task_id)
            }
            _ => None,
        })
        .expect("running agent task id");

    coordinator
        .cancel_task(task_id.clone(), "manual running cancellation")
        .await
        .expect("cancel running agent turn");

    tokio::task::yield_now().await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let terminal_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            ) || matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        terminal_events.len(),
        1,
        "running turn should emit exactly one terminal event"
    );
    assert!(matches!(
        &terminal_events[0].payload,
        EventV1::TaskCancelled(data) if data.reason == "manual running cancellation"
    ));
    assert_task_event_context(
        terminal_events[0],
        &EventActor::new(ActorKind::Worker, Some(agent_id)),
        &request_id,
    );
    let cancelled = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cancelled.len(), 1, "running turn should not double-cancel");
    assert!(cancelled
        .iter()
        .all(|event| event.actor.kind == ActorKind::Worker));
}
#[tokio::test]
async fn coord_agent_turn_provider_events_have_isolated_correlation_ids() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(5));

    let run = coordinator
        .start_run("coord_agent_corr", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let _ = coordinator
        .spawn_agent(actor.clone(), "alpha", None)
        .await
        .expect("spawn alpha");
    let _ = coordinator
        .spawn_agent(actor, "beta", None)
        .await
        .expect("spawn beta");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        let mut provider_request_ids = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::ProviderRequestStarted(data) => Some(data.request_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        provider_request_ids.sort();
        provider_request_ids.dedup();
        provider_request_ids.len() == 2
            && events
                .iter()
                .filter(|event| matches!(event.payload, EventV1::ProviderRequestFinished(_)))
                .count()
                >= 2
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let mut provider_request_ids = Vec::new();

    for event in &events {
        if let EventV1::ProviderRequestStarted(ref data) = event.payload {
            provider_request_ids.push(data.request_id.clone());
        }
    }

    provider_request_ids.sort();
    provider_request_ids.dedup();
    assert_eq!(
        provider_request_ids.len(),
        2,
        "expected one provider request_id per spawned agent"
    );

    for provider_request_id in provider_request_ids {
        let related: Vec<_> = events
            .iter()
            .filter(|event| match &event.payload {
                EventV1::ProviderRequestStarted(data) => data.request_id == provider_request_id,
                EventV1::ProviderStreamDelta(data) => data.request_id == provider_request_id,
                EventV1::ProviderRequestFinished(data) => data.request_id == provider_request_id,
                _ => false,
            })
            .collect();

        assert!(
            !related.is_empty(),
            "request {} should have related provider events",
            provider_request_id
        );
        let turn_id = related[0]
            .correlation_id
            .as_deref()
            .expect("provider event correlation should be stable turn id");
        assert!(related
            .iter()
            .all(|event| event.correlation_id.as_deref() == Some(turn_id)));
        assert_ne!(
            turn_id, provider_request_id,
            "provider request id should be distinct from stable turn correlation id"
        );
    }
}
#[tokio::test]
async fn provider_single_call_returns_tool_intents_without_executing_tools() {
    let tool_call_count = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CountingShellTool {
        calls: tool_call_count.clone(),
    }));
    let tool_registry = Arc::new(registry);

    let profile = AgentProfile {
        name: "alpha".to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "single-call-system".to_string(),
        temperature: Some(0.0),
        cache_retention: Default::default(),
        max_iters: Some(12),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec!["shell.run".to_string()],
    };
    let request = AgentRequest {
        agent_id: "agent_1".to_string(),
        prompt: "single provider call".to_string(),
        prompt_context: None,
        selected_file_tags: Vec::new(),
        selected_agent_tags: Vec::new(),
        selected_resource_tags: Vec::new(),
        model_ref: "mock:model-1".to_string(),
        model_settings: AgentModelSettings::default(),
    };
    let tool_defs = build_provider_tool_defs(&profile, tool_registry.as_ref())
        .expect("build provider tool defs");
    let function_name = tool_defs.first().expect("tool def").function_name.clone();
    let messages = build_provider_context_messages(
        &profile,
        &ProviderContext::default(),
        &request.provider_prompt(),
    );
    let expected_request = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-1".to_string(),
        messages: messages.clone(),
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: Some(tool_defs.clone()),
        tool_choice: Some(harness_providers::ToolChoice::Auto),
        context: Default::default(),
        stream: true,
    };
    let mut scripted = BTreeMap::new();
    scripted.insert(
        request_digest(&expected_request),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ReasoningDelta("thinking".to_string()),
            ProviderStreamEvent::TextDelta("I will call tools".to_string()),
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "first_call".to_string(),
                function_name: Some(function_name.clone()),
                arguments_delta: "{".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "first_call".to_string(),
                function_name: function_name.clone(),
                arguments_json: r#"{"cmd":"one"}"#.to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "second_call".to_string(),
                function_name: function_name.clone(),
                arguments_json: r#"{"cmd":"two"}"#.to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 3,
                    total_tokens: 5,
                },
            },
        ],
    );
    let provider = Arc::new(MockProvider::new(scripted));
    let events = Arc::new(Mutex::new(Vec::<AgentRuntimeEvent>::new()));

    let response = stream_assistant_response_once(
        StreamAssistantResponseOnceRequest {
            provider,
            profile: &profile,
            model: AgentModelRef::parse(&request.model_ref),
            model_settings: request.model_settings.clone(),
            turn_request_id: "turn_1".to_string(),
            provider_request_id: "provider_call_1".to_string(),
            session_id: Some("agent-test".to_string()),
            prompt_summary: &request.prompt,
            retry_metadata: None,
            context: ProviderBoundaryContext::ProviderMessages {
                messages: &messages,
            },
            tool_defs: &tool_defs,
        },
        {
            let events = events.clone();
            move |event| {
                let events = events.clone();
                async move {
                    events.lock().expect("lock events").push(event);
                }
            }
        },
    )
    .await
    .expect("single provider call succeeds");

    assert_eq!(tool_call_count.load(Ordering::SeqCst), 0);
    assert_eq!(response.text, "I will call tools");
    assert_eq!(response.reasoning, "thinking");
    assert_eq!(response.stop_reason, "done");
    assert_eq!(response.tool_call_deltas.len(), 1);
    assert_eq!(
        response
            .tool_intents
            .iter()
            .map(|intent| (
                intent.tool_call_id.as_str(),
                intent.function_name.as_str(),
                intent.tool_id.as_str(),
                intent.arguments.clone(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "first_call",
                function_name.as_str(),
                "shell.run",
                json!({"cmd": "one"}),
            ),
            (
                "second_call",
                function_name.as_str(),
                "shell.run",
                json!({"cmd": "two"}),
            ),
        ]
    );

    let events = events.lock().expect("lock events");
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderRequestStarted(started)
            if started.request_id == "provider_call_1"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderReasoningDelta { request_id, delta }
            if request_id == "provider_call_1" && delta == "thinking"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderRequestFinished(finished)
            if finished.request_id == "provider_call_1" && finished.finish_reason == "done"
    )));
}
#[tokio::test]
async fn provider_calls_in_one_turn_have_unique_request_ids() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "provider_call_1".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("final answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        test_tool_registry(),
        shell_only_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_provider_unique_request_ids_in_turn",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let turn_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "needs a tool")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                        && data.result_summary == "final answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let provider_started = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                Some(data.request_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        provider_started.len(),
        2,
        "one logical agent turn with one tool result should make two provider calls"
    );
    let unique_provider_request_ids = provider_started.iter().collect::<BTreeSet<_>>();
    assert_eq!(
        unique_provider_request_ids.len(),
        provider_started.len(),
        "each provider call should have its own provider request id while event correlation remains on the stable agent turn request id"
    );
    for event in events.iter().filter(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                    && provider_started.contains(&data.request_id)
        )
    }) {
        let EventV1::ProviderRequestStarted(data) = &event.payload else {
            unreachable!("filtered provider start events")
        };
        let metadata = data.metadata.as_ref().expect("provider start metadata");
        assert_eq!(metadata.turn_id.as_deref(), Some(turn_request_id.as_str()));
        assert_eq!(
            metadata.provider_call_id.as_deref(),
            Some(data.request_id.as_str())
        );
    }
    assert!(events.iter().all(|event| {
        let provider_request_id = match &event.payload {
            EventV1::ProviderRequestStarted(data) => Some(&data.request_id),
            EventV1::ProviderStreamDelta(data) => Some(&data.request_id),
            EventV1::ProviderRequestFinished(data) => Some(&data.request_id),
            _ => None,
        };

        provider_request_id.is_none_or(|request_id| {
            !provider_started.contains(request_id)
                || event.correlation_id.as_deref() == Some(turn_request_id.as_str())
        })
    }));
}
