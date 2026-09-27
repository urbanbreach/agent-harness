use super::*;
use harness_core::event::{EventEnvelopeV1, EventV1};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

async fn wait_for(
    events: &mut harness_core::store::EventStream,
    predicate: impl Fn(&EventV1) -> bool,
    missing: &'static str,
) -> Result<EventEnvelopeV1, Box<dyn std::error::Error>> {
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = events.next().await {
                let event = event?;
                if predicate(&event.payload) {
                    return Ok(event);
                }
            }
            Err(harness_core::store::EventStoreError::Invalid(missing))
        })
        .await
        .map_err(|_| missing)??,
    )
}

struct NotificationGate {
    parent: tokio::sync::Semaphore,
    child: tokio::sync::Semaphore,
    parent_started: tokio::sync::Notify,
    child_started: tokio::sync::Notify,
    collect_reports: usize,
    wakeups: AtomicUsize,
}
#[async_trait::async_trait]
impl harness_providers::Provider for NotificationGate {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        let Some(last) = request.messages.last() else {
            return Box::pin(tokio_stream::iter([Stream::error("missing prompt")]));
        };
        if last.content == "launch" {
            let calls = (0..self.collect_reports.max(1)).map(|index| Stream::ToolCallComplete {
                tool_call_id: format!("launch-child-{index}"), function_name: "task".into(),
                arguments_json: json!({"subagent_type":"child","prompt":if index == 0 {"child work"} else {"child failure"},"run_in_background":true,"load_skills":[]}).to_string(),
            }).chain(std::iter::once(Stream::Done { usage: None })).collect::<Vec<_>>();
            return Box::pin(tokio_stream::iter(calls));
        }
        let answer = if matches!(last.content.as_str(), "child work" | "child failure") {
            self.child_started.notify_one();
            if let Ok(permit) = self.child.acquire().await {
                permit.forget();
            }
            if last.content == "child failure" {
                return Box::pin(tokio_stream::iter([Stream::error("child failed")]));
            }
            "child report"
        } else if last.role == harness_providers::MessageRole::Tool {
            if self.collect_reports > 0 {
                let block = last.name.as_deref() == Some("background_output");
                if block {
                    let output: serde_json::Value = serde_json::from_str(&last.content)
                        .map_err(|e| e.to_string())
                        .unwrap_or_else(|e| json!({"error":e}));
                    let data = &output["data"];
                    let complete = if let Some(tasks) = data["tasks"].as_array() {
                        tasks.len() == self.collect_reports
                            && tasks.iter().all(|task| {
                                matches!(task["status"].as_str(), Some("completed" | "failed"))
                            })
                    } else {
                        data["status"] == "completed"
                    };
                    if complete {
                        return Box::pin(tokio_stream::iter([
                            Stream::TextDelta("reports summarized".into()),
                            Stream::Done { usage: None },
                        ]));
                    }
                }
                return Box::pin(tokio_stream::iter([
                    Stream::ToolCallComplete {
                        tool_call_id: format!("collect-{block}"),
                        function_name: "background_output".into(),
                        arguments_json: json!({"all":true,"wait_mode":"all","block":block})
                            .to_string(),
                    },
                    Stream::Done { usage: None },
                ]));
            }
            self.parent_started.notify_one();
            if let Ok(permit) = self.parent.acquire().await {
                permit.forget();
            }
            "parent finished"
        } else if last.content == "check history" {
            assert!(
                request
                    .messages
                    .iter()
                    .all(|message| !message.content.starts_with("Background child ")),
                "consumed notifications must not reappear in provider history"
            );
            "history checked"
        } else {
            self.wakeups.fetch_add(1, Ordering::SeqCst);
            assert!(
                last.content.contains("child report"),
                "notification must include the child result"
            );
            "notification delivered"
        };
        Box::pin(tokio_stream::iter([
            Stream::TextDelta(answer.into()),
            Stream::Done { usage: None },
        ]))
    }
}

#[tokio::test]
async fn accepted_background_work_reserves_parent_queue_capacity_for_its_notification(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::event::EventV1;
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(NotificationGate {
        parent: tokio::sync::Semaphore::new(0),
        child: tokio::sync::Semaphore::new(0),
        parent_started: tokio::sync::Notify::new(),
        child_started: tokio::sync::Notify::new(),
        collect_reports: 0,
        wakeups: AtomicUsize::new(0),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config.command_buffer = 1;
    config.provider_model_concurrency = 2;
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = vec!["task".into(), "background_output".into()];
    config.agent_profiles.insert("default".into(), parent);
    config
        .agent_profiles
        .insert("child".into(), AgentProfile::fallback("child"));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("notification capacity", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            parent.clone(),
            "launch",
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(
            provider.parent_started.notified(),
            provider.child_started.notified()
        )
    })
    .await?;
    let running = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(parent.clone())),
            None,
            "background_output",
            json!({"all":true}),
        )
        .await?;
    assert_eq!(
        running.structured_json.ok_or("missing poll result")?["status"],
        "running"
    );
    assert!(
        coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                parent.clone(),
                "manual queued work"
            )
            .await
            .is_err(),
        "the accepted child must retain a notification slot"
    );
    provider.child.add_permits(1);
    let notification = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if let EventV1::BackgroundTaskNotification(e) = event?.payload {
                return Ok::<_, harness_core::store::EventStoreError>(e);
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "missing notification",
        ))
    })
    .await??;
    let followup = notification
        .delivered_turn_request_id
        .ok_or("lost notification")?;
    assert!(
        coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::Worker, Some(parent.clone())),
                None,
                "background_output",
                json!({"all":true,"full_session":true,"since_message_id":"missing"}),
            )
            .await
            .is_err(),
        "a failed history read must not consume the queued notification"
    );
    assert!(
        coordinator
            .compact_agent_context(parent, None, "manual")
            .await
            .is_err(),
        "manual compaction must respect the same queue capacity"
    );
    provider.parent.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCompleted(e) if e.task_id.as_str() == followup && e.result_summary == "notification delivered") { return Ok::<_, harness_core::store::EventStoreError>(()); }
        }
        Err(harness_core::store::EventStoreError::Invalid("notification was not consumed"))
    }).await??;
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn polled_child_reports_suppress_parent_wakeups_in_live_and_resumed_history(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::event::EventV1;
    use tokio_stream::StreamExt;

    for count in [1, 2] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(NotificationGate {
            parent: tokio::sync::Semaphore::new(0),
            child: tokio::sync::Semaphore::new(0),
            parent_started: tokio::sync::Notify::new(),
            child_started: tokio::sync::Notify::new(),
            collect_reports: count,
            wakeups: AtomicUsize::new(0),
        });
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.tool_registry = Arc::new(harness_tools::coordinator_registry(
            ShellAllowlist::default(),
        ));
        config.permission_policy = PermissionPolicy::allow_all();
        config.provider_model_concurrency = 3;
        let mut parent = AgentProfile::fallback("default");
        parent.toolset = vec!["task".into(), "background_output".into()];
        config.agent_profiles.insert("default".into(), parent);
        config
            .agent_profiles
            .insert("child".into(), AgentProfile::fallback("child"));
        let coordinator = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator
            .start_run("consumed reports", temp.path())
            .await?;
        let parent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let mut events = coordinator.subscribe_new_events().await?;
        let request = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                parent.clone(),
                "launch",
            )
            .await?;
        // The first poll sees running children; the next tool waits for their reports.
        wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::ToolCallRequested(e)
            if e.tool_id == "background_output" && e.args_summary.contains("\"block\":true"))
            },
            "parent did not wait for children",
        )
        .await?;
        provider.child.add_permits(count);
        wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::TaskCompleted(e)
            if e.task_id.as_str() == request && e.result_summary == "reports summarized")
            },
            "parent did not summarize reports",
        )
        .await?;
        for resumed in [false, true] {
            let active = if resumed {
                let restored = spawn_coordinator(
                    config.clone(),
                    Arc::new(FakeClock::new()),
                    Arc::new(DefaultRedactor::default()),
                );
                restored
                    .resume_run(run.run_id.to_string(), "resume consumed reports")
                    .await?;
                restored
            } else {
                coordinator.clone()
            };
            let mut events = active.subscribe_new_events().await?;
            let request = active
                .request_agent_turn(
                    EventActor::new(ActorKind::User, None),
                    parent.clone(),
                    "check history",
                )
                .await?;
            wait_for(
                &mut events,
                |event| {
                    matches!(event, EventV1::TaskCompleted(e)
                if e.task_id.as_str() == request && e.result_summary == "history checked")
                },
                "history check did not finish",
            )
            .await?;
            active.stop_run().await?;
        }
        assert_eq!(
            provider.wakeups.load(Ordering::SeqCst),
            0,
            "a consumed report must not wake the parent"
        );
    }
    Ok(())
}
