use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn aborted_response_compaction_preserves_abort_marker() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
                usage: Some(CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                }),
            },
        ],
        provider_text_events("Compaction summary of aborted turn."),
        provider_text_events("after cancellation"),
    ]);
    let coordinator = test_agent_tool_coordinator_with_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![NamedShellTool {
            id: "shell.block",
            output: "blocking output",
            started: Some(Arc::clone(&tool_started)),
            release: Some(Arc::clone(&tool_release)),
        }]),
        shell_only_permission_policy(),
        vec!["shell.block".to_string()],
        12,
        CompactionRuntimeConfig {
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_aborted_response_compaction_marker",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
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
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_secs(5), tool_started.notified())
        .await
        .unwrap_or_abort();
    let task_id = wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(cancelled_request_id.as_str())
                        && data.queue_key.as_deref().is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
            )
        })
    })
    .await
    .iter()
    .find_map(|event| match &event.payload {
        EventV1::TaskScheduled(data)
            if event.correlation_id.as_deref() == Some(cancelled_request_id.as_str()) =>
        {
            Some(data.task_id.clone())
        }
        _ => None,
    })
    .unwrap_or_abort();
    coordinator
        .cancel_task(task_id.clone(), "operator cancelled")
        .await
        .unwrap_or_abort();
    tool_release.notify_waiters();

    let events = wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if data.task_id == task_id && data.reason == "operator cancelled"
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after cancellation")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_secs(10), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == task_id && data.reason == "operator cancelled"
        )
    }));

    let requests = provider.requests();
    let follow_up = requests.last().unwrap_or_abort();
    let marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .unwrap_or_abort();
    assert!(marker.content.contains("Status: aborted"));
    assert!(marker.content.contains("Stage: cancelled"));
}
#[tokio::test]
async fn failed_response_compaction_failure_does_not_mask_original_error() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("summary call failed"),
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_artifact_failure",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
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
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        ))
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

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
            EventV1::SessionCompaction(data) if data.trigger_reason == "failed_response"
        )
    }));
}
#[tokio::test]
async fn critical_compaction_requested_hook_failure_does_not_commit() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };
    let coordinator = test_agent_coordinator_with_provider_compaction_and_hooks(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
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
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
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
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_millis(900), |events| {
        events.iter().any(|event| matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        ))
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

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
            EventV1::SessionCompaction(data) if data.trigger_reason == "failed_response"
        )
    }));
}
#[tokio::test]
async fn profile_max_iters_does_not_cap_tool_loops() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "loop_call_1".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                }),
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
                usage: Some(CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("completed after former cap".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                }),
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
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "loop past former cap")
        .await
        .unwrap_or_abort();

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
    coordinator.stop_run().await.unwrap_or_abort();

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
