#[tokio::test]
async fn aborted_response_compaction_preserves_abort_marker() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tool_started = Arc::new(Notify::new());
    let tool_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial before cancellation {}",
                "C".repeat(12_000)
            )),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "blocking_tool".to_string(),
                function_name: "shell_block".to_string(),
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
        provider_text_events("after cancellation"),
    ]);
    let coordinator = test_agent_tool_coordinator_with_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![NamedShellTool {
            id: "shell.block",
            output: "blocking output",
            started: Some(tool_started.clone()),
            release: Some(tool_release.clone()),
        }]),
        shell_only_permission_policy(),
        vec!["shell.block".to_string()],
        12,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_aborted_response_compaction_marker",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;

    let cancelled_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            agent_id.clone(),
            "cancel after assistant",
        )
        .await
        .expect("cancellable turn");
    tokio::time::timeout(Duration::from_millis(700), tool_started.notified())
        .await
        .expect("blocking tool should start");
    let task_id = load_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(cancelled_request_id.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("agent task id");
    coordinator
        .cancel_task(task_id.clone(), "operator cancelled")
        .await
        .expect("cancel running agent turn");
    tool_release.notify_waiters();

    let events = wait_for_events(&run.events_path, Duration::from_millis(900), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(data)
                    if data.trigger_reason == "aborted_response"
                        && data.through_request_id.as_deref() == Some(cancelled_request_id.as_str())
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after cancellation")
        .await
        .expect("request follow-up turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "after cancellation"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == task_id && data.reason == "operator cancelled"
        )
    }));
    let checkpoint = checkpoint_for_trigger(&run, &events, "aborted_response");
    let aborted_turn = checkpoint
        .recent_turns
        .iter()
        .find(|turn| !turn.status.is_completed())
        .expect("aborted turn remains provider-visible");
    assert_eq!(
        aborted_turn.status,
        harness_core::agent::ProviderConversationTurnStatus::Aborted
    );
    assert_eq!(aborted_turn.failure_stage.as_deref(), Some("cancelled"));
    assert_eq!(
        aborted_turn.failure_reason.as_deref(),
        Some("operator cancelled")
    );
    assert!(aborted_turn
        .assistant_response
        .contains("partial before cancellation"));

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up request");
    let marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("aborted marker should remain in provider-visible context");
    assert!(marker.content.contains("Status: aborted"));
    assert!(marker.content.contains("Stage: cancelled"));
}
#[tokio::test]
async fn failed_response_compaction_failure_does_not_mask_original_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::error("provider exploded"),
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_artifact_failure",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;
    fs::remove_dir_all(&run.artifacts_dir).expect("remove artifacts dir");
    fs::write(&run.artifacts_dir, "not a directory").expect("replace artifacts dir with file");

    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("failing turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionFailed(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionWritten(data) if data.trigger_reason == "failed_response"
        )
    }));
}
#[tokio::test]
async fn critical_compaction_requested_hook_failure_records_compaction_failed() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::error("provider exploded"),
        ],
    ]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("failed-terminal-compaction-blocker".to_string()),
                event: HookLifecycleEvent::CompactionRequested,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "if [ \"${HARNESS_HOOK_OUTCOME:-}\" = failed_response ]; then printf 'blocked failed terminal compaction'; exit 23; fi; printf ok".to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: true,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
        },
        suppress_execution: false,
    };
    let coordinator = test_agent_coordinator_with_provider_compaction_and_hooks(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
        hook_runtime_config,
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_hook_failure",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;

    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("failing turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(900), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionFailed(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionWritten(data) if data.trigger_reason == "failed_response"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionWritten(data) if data.trigger_reason == "failed_response"
        )
    }));
}
#[tokio::test]
async fn profile_max_iters_does_not_cap_tool_loops() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "loop_call_1".to_string(),
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
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "loop_call_2".to_string(),
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
            ProviderStreamEvent::TextDelta("completed after former cap".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                },
            },
        ],
    ]);
    let provider_handle = provider.clone();
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        shell_only_permission_policy(),
        vec!["shell.run".to_string()],
        2,
    );

    let run = coordinator
        .start_run(
            "coord_profile_max_iters_not_enforced",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "loop past former cap")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "completed after former cap"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("max_iters")
        )
    }));

    let started_tools = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::ToolCallStarted(_)))
        .count();
    assert_eq!(
        started_tools, 2,
        "max_iters=2 should not stop the third provider phase after two tool loops"
    );

    let requests = provider_handle.requests();
    assert_eq!(requests.len(), 3, "expected all provider phases to run");
    let final_messages = &requests[2].messages;
    assert!(final_messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "loop past former cap"
    }));
    assert!(final_messages
        .iter()
        .any(|message| { message.role == MessageRole::Tool && message.content.contains("ok {}") }));
}
