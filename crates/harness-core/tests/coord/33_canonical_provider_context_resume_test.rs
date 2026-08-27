use harness_core::attachment_transport::AttachmentMetadata;
use harness_core::config::{
    ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget,
};
use harness_core::model_resolution::ModelResolution;
use harness_core::UnwrapOrAbort;

fn drift_target(
    variant: &str,
    reasoning_effort: &str,
    text_verbosity: &str,
    reasoning_summary: &str,
    thinking: serde_json::Value,
) -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: "mock:model-1".to_string(),
        provider: "mock".to_string(),
        model: "model-1".to_string(),
        variant: Some(variant.to_string()),
        reasoning_effort: Some(reasoning_effort.to_string()),
        text_verbosity: Some(text_verbosity.to_string()),
        reasoning_summary: Some(reasoning_summary.to_string()),
        thinking: Some(thinking),
        limits: ResolvedModelLimits::from_values(
            Some(64_000),
            Some(60_000),
            Some(4_000),
            ModelLimitProvenance::explicit("g007 config drift red"),
        ),
        resolution: ModelResolution::default(),
        catalog_entry: None,
    }
}

fn coordinator_for_target(
    session_dir: &std::path::Path,
    provider: std::sync::Arc<dyn Provider>,
    target: ResolvedModelTarget,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = provider;
    config.tool_registry = test_tool_registry();
    config.permission_policy = allow_all_permission_policy();
    config.agent_profiles = agent_profiles();
    config.agent_profiles
        .get_mut("default")
        .unwrap_or_abort()
        .toolset = vec!["shell.run".to_string()];
    config.agent_model_targets.insert("default".to_string(), target);
    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn remove_physical_request_id(request: &CompletionRequest) -> serde_json::Value {
    let mut value = serde_json::to_value(request).unwrap_or_abort();
    if let Some(context) = value.get_mut("context").and_then(serde_json::Value::as_object_mut)
    {
        context.remove("request_id");
    }
    value
}

async fn await_turn_terminal(
    stream: &mut harness_core::store::EventStream,
    request_id: &str,
) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = stream.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() == Some(request_id)
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
}

fn clone_persisted_run(source: &std::path::Path, destination: &std::path::Path, run_id: &str) {
    std::fs::create_dir_all(destination.join("artifacts")).unwrap_or_abort();
    let events_path = destination.join("events.jsonl");
    std::fs::copy(
        source.join("events.jsonl"),
        &events_path,
    )
    .unwrap_or_abort();
    let events = load_events(&events_path);
    let next_seq = events.last().map_or(1, |event| event.seq + 1);
    let terminal = resume_fixture_event(
        run_id,
        next_seq,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "prefix cloned for restart".to_string(),
        }),
    );
    let mut body = std::fs::read_to_string(&events_path).unwrap_or_abort();
    body.push_str(&serde_json::to_string(&terminal).unwrap_or_abort());
    body.push('\n');
    std::fs::write(events_path, body).unwrap_or_abort();
}

#[tokio::test]
async fn restart_provider_request_matches_live_when_current_config_drifts() {
    // arrange
    // act
    // assert
    // Given: a completed tool pair and typed attachment persisted with target A.
    let source_dir = tempfile::tempdir().unwrap_or_abort();
    let restart_dir = tempfile::tempdir().unwrap_or_abort();
    let target_a = drift_target(
        "target-a-variant",
        "high",
        "low",
        "auto",
        json!({"budget": 1024, "mode": "target-a"}),
    );
    let target_b = drift_target(
        "target-b-variant",
        "minimal",
        "high",
        "none",
        json!({"budget": 256, "mode": "target-b"}),
    );
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "toolcall_000001".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: r#"{"command":"printf g007"}"#.to_string(),
            },
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 17,
                    completion_tokens: 5,
                    total_tokens: 22,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("completed tool prefix".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 23,
                    completion_tokens: 4,
                    total_tokens: 27,
                }),
            },
        ],
        provider_text_events("same continuation answer"),
    ]);
    let live = coordinator_for_target(source_dir.path(), Arc::new(provider.clone()), target_a.clone());
    let run = live
        .start_run("g007_config_drift", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = live
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let store = live.event_store().await.unwrap_or_abort();
    let mut prefix_events = store.subscribe(1).unwrap_or_abort();
    let attachment = AttachmentMetadata::from_bytes(
        "g007-attachment",
        "image/png",
        None,
        b"typed attachment bytes",
        None,
    );
    let prefix_request_id = live
        .request_agent_turn_with_model_target_and_selected_tags_and_attachments(
            supervisor_actor(),
            agent_id.clone(),
            "inspect the attached asset with shell.run",
            Default::default(),
            vec![attachment],
            target_a.clone(),
        )
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut prefix_events, &prefix_request_id).await;
    let prefix_requests = provider.requests();
    assert_eq!(prefix_requests.len(), 2, "prefix must contain provider tool pair");
    assert_eq!(prefix_requests[0].variant.as_deref(), Some("target-a-variant"));
    assert!(prefix_requests.iter().any(|request| request.tools.is_some()));
    assert!(load_events(&run.events_path).iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PromptAttachmentsSubmitted(data)
                if data.attachments.iter().any(|attachment| attachment.id == "g007-attachment")
        )
    }));

    // When: clone the persisted prefix, restart under target B, and issue identical continuations.
    let restart_run_dir = restart_dir.path().join(run.run_id.to_string());
    clone_persisted_run(&run.run_dir, &restart_run_dir, run.run_id.as_ref());
    let mut live_events = store.subscribe(1).unwrap_or_abort();
    let live_request_id = live
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            agent_id.clone(),
            "continue from the completed tool result",
            target_a,
        )
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut live_events, &live_request_id).await;

    let restarted_provider = CapturingProvider::new(vec!["same continuation answer"]);
    let restarted = coordinator_for_target(restart_dir.path(), Arc::new(restarted_provider.clone()), target_b);
    restarted
        .resume_run(run.run_id.to_string(), "interactive")
        .await
        .unwrap_or_else(|error| panic!("restart fixture failed to resume: {error}"));
    let restart_store = restarted.event_store().await.unwrap_or_abort();
    let mut restart_events = restart_store.subscribe(1).unwrap_or_abort();
    let restart_request_id = restarted
        .request_agent_turn(supervisor_actor(), agent_id, "continue from the completed tool result")
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut restart_events, &restart_request_id).await;
    live.stop_run().await.unwrap_or_abort();
    restarted.stop_run().await.unwrap_or_abort();

    // Then: only physical request_id is normalized; every semantic field and digest must match.
    let live_request = provider.requests().last().cloned().unwrap_or_abort();
    let restart_request = restarted_provider
        .requests()
        .last()
        .cloned()
        .unwrap_or_abort();
    eprintln!(
        "G007_RED compared_fields=provider_id,model_id,messages,temperature,max_tokens,variant,reasoning_effort,text_verbosity,reasoning_summary,thinking,tools,tool_choice,context,stream normalized_only=context.request_id live_digest={} restart_digest={} expected_mismatch_paths=/variant,/reasoning_effort,/text_verbosity,/reasoning_summary,/thinking",
        request_digest(&live_request),
        request_digest(&restart_request)
    );
    assert_eq!(
        remove_physical_request_id(&live_request),
        remove_physical_request_id(&restart_request),
        "full normalized CompletionRequest differs after current-config drift"
    );
    assert_eq!(
        request_digest(&live_request),
        request_digest(&restart_request),
        "full request digests must match after restart"
    );
}

include!("33_canonical_provider_context_resume_counters_test.rs");
include!("33_canonical_provider_context_resume_shape_test.rs");
