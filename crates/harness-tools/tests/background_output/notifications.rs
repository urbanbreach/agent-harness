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
    collected: tokio::sync::Mutex<Vec<String>>,
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
        for message in request.messages.iter().filter(|message| {
            message.role == harness_providers::MessageRole::Tool
                && message.name.as_deref() == Some("spawn_subagent")
        }) {
            let id = message
                .content
                .lines()
                .find_map(|line| line.strip_prefix("subagent_id: "));
            let Some(id) = id else {
                return Box::pin(tokio_stream::iter([Stream::error(
                    "spawn result omitted its subagent identity",
                )]));
            };
            let mut collected = self.collected.lock().await;
            if !collected.iter().any(|known| known == id) {
                collected.push(id.into());
            }
        }
        if last.content == "launch" {
            let calls = (0..self.collect_reports.max(1)).map(|index| Stream::ToolCallComplete {
                tool_call_id: format!("launch-child-{index}"), function_name: "spawn_subagent".into(),
                arguments_json: json!({"prompt":if index == 0 {"child work"} else {"child failure"},"description":"Child report"}).to_string(),
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
                let block = last.name.as_deref() == Some("get_command_or_subagent_output");
                if block {
                    let statuses: Vec<_> = last
                        .content
                        .lines()
                        .filter_map(|line| {
                            line.strip_prefix("Status: ").or_else(|| {
                                line.strip_prefix("--- Task ")?
                                    .rsplit_once(" [")?
                                    .1
                                    .strip_suffix("] ---")
                            })
                        })
                        .collect();
                    let complete = statuses.len() == self.collect_reports
                        && statuses
                            .iter()
                            .all(|status| matches!(*status, "completed" | "failed"));
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
                        function_name: "get_command_or_subagent_output".into(),
                        arguments_json: json!({"task_ids":self.collected.lock().await.clone(),"timeout_ms":if block {30_000} else {0}})
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
                request.messages.iter().all(|message| message.role
                    != harness_providers::MessageRole::User
                    || !message.content.contains("child report")),
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
        collected: tokio::sync::Mutex::new(Vec::new()),
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
    parent.toolset = vec![
        "spawn_subagent".into(),
        "get_command_or_subagent_output".into(),
        "kill_command_or_subagent".into(),
    ];
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
    let parent_started = provider.parent_started.notified();
    let child_started = provider.child_started.notified();
    tokio::pin!(parent_started, child_started);
    parent_started.as_mut().enable();
    child_started.as_mut().enable();
    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            parent.clone(),
            "launch",
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(parent_started, child_started)
    })
    .await?;
    let child_ids: Vec<_> = coordinator
        .subagent_history()
        .await?
        .records
        .into_keys()
        .collect();
    let running = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(parent.clone())),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":child_ids}),
        )
        .await?;
    assert_eq!(
        running.structured_json.ok_or("missing poll result")?["Result"]["status"],
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
                "get_command_or_subagent_output",
                json!({"task_ids":[false]}),
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
            collected: tokio::sync::Mutex::new(Vec::new()),
        });
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.tool_registry = Arc::new(harness_tools::coordinator_registry(
            ShellAllowlist::default(),
        ));
        config.permission_policy = PermissionPolicy::allow_all();
        config.provider_model_concurrency = 3;
        let mut parent = AgentProfile::fallback("default");
        parent.toolset = vec![
            "spawn_subagent".into(),
            "get_command_or_subagent_output".into(),
            "kill_command_or_subagent".into(),
        ];
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
            if e.tool_id == "get_command_or_subagent_output" && e.args_summary.contains("\"timeout_ms\":30000"))
            },
            "parent did not wait for children",
        )
        .await?;
        provider.child.add_permits(count);
        let summarized = wait_for(
            &mut events,
            |event| {
                matches!(event, EventV1::TaskCompleted(e)
            if e.task_id.as_str() == request)
            },
            "parent did not summarize reports",
        )
        .await?;
        assert!(
            matches!(summarized.payload, EventV1::TaskCompleted(ref event)
                if event.result_summary == "reports summarized"),
            "parent completed without the child reports: {:?}",
            summarized.payload
        );
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

#[path = "notifications/authority.rs"]
mod authority;
