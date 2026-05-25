#[tokio::test]
async fn coordinator_runs_parallel_child_sessions_under_slot_limits() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = Arc::new(PromptScriptedProvider::new(
        BTreeMap::from([
            (
                "alpha-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("alpha-ok".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            ),
            (
                "beta-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("beta-ok".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            ),
        ]),
        Duration::from_millis(40),
    ));
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), provider, 2);

    let run = coordinator
        .start_run(
            "coord_parallel_child_sessions_under_limits",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let actor = supervisor_actor();
    let _alpha = coordinator
        .spawn_agent(actor.clone(), "alpha", None)
        .await
        .expect("spawn alpha");
    let _beta = coordinator
        .spawn_agent(actor.clone(), "beta", None)
        .await
        .expect("spawn beta");
    let _beta_two = coordinator
        .spawn_agent(actor, "beta", None)
        .await
        .expect("spawn second beta");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        let scheduled = events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskScheduled(data)
                        if data.queue_key.as_deref() == Some("provider_model:mock:model-1")
                )
            })
            .count();
        let scheduled_task_ids = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::TaskScheduled(data)
                    if data.queue_key.as_deref() == Some("provider_model:mock:model-1") =>
                {
                    Some(data.task_id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let completed = events
            .iter()
            .filter(|event| {
                matches!(&event.payload, EventV1::TaskCompleted(data) if scheduled_task_ids.contains(&data.task_id))
            })
            .count();
        scheduled == 4 && completed == 3
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let scheduled = events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("provider_model:mock:model-1") =>
            {
                Some((idx, data.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        scheduled.len(),
        4,
        "three tasks should yield one queued+restarted record"
    );

    let first_three_states = scheduled
        .iter()
        .take(3)
        .map(|(_, data)| data.state)
        .collect::<Vec<_>>();
    assert_eq!(
        first_three_states,
        vec![
            TaskScheduleState::Started,
            TaskScheduleState::Started,
            TaskScheduleState::Queued,
        ],
        "limit=2 should start two child sessions and deterministically queue the third"
    );

    let started = scheduled
        .iter()
        .filter(|(_, data)| data.state == TaskScheduleState::Started)
        .map(|(_, data)| data.task_id.clone())
        .collect::<Vec<_>>();
    let queued = scheduled
        .iter()
        .filter(|(_, data)| data.state == TaskScheduleState::Queued)
        .map(|(_, data)| data.task_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(
        started.len(),
        3,
        "all three child tasks should eventually start"
    );
    assert_eq!(
        queued.len(),
        1,
        "exactly one child task should queue at saturation"
    );
    assert_eq!(
        started
            .iter()
            .filter(|task_id| *task_id == &queued[0])
            .count(),
        1,
        "queued task should later transition to started once a slot frees"
    );

    let scheduled_task_ids = scheduled
        .iter()
        .map(|(_, data)| data.task_id.clone())
        .collect::<BTreeSet<_>>();
    let completed = events
        .iter()
        .filter(|event| {
            matches!(&event.payload, EventV1::TaskCompleted(data) if scheduled_task_ids.contains(&data.task_id))
        })
        .count();
    assert_eq!(
        completed, 3,
        "all child sessions should complete under limit=2"
    );
}
#[tokio::test]
async fn coordinator_isolates_parallel_child_failures() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = Arc::new(PromptScriptedProvider::new(
        BTreeMap::from([
            (
                "alpha-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::Error {
                        message: "alpha child failed".to_string(),
                    },
                ],
            ),
            (
                "beta-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("beta child ok".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            ),
        ]),
        Duration::from_millis(40),
    ));
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), provider, 2);

    let run = coordinator
        .start_run(
            "coord_parallel_child_failure_isolation",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let actor = supervisor_actor();
    let _alpha = coordinator
        .spawn_agent(actor.clone(), "alpha", None)
        .await
        .expect("spawn alpha");
    let _beta = coordinator
        .spawn_agent(actor, "beta", None)
        .await
        .expect("spawn beta");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        let scheduled_task_ids = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::TaskScheduled(data)
                    if data.queue_key.as_deref() == Some("provider_model:mock:model-1")
                        && data.state == TaskScheduleState::Started =>
                {
                    Some(data.task_id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        scheduled_task_ids.len() == 2
            && events
                .iter()
                .filter(|event| {
                    matches!(&event.payload, EventV1::TaskCompleted(data) if scheduled_task_ids.contains(&data.task_id))
                        || matches!(&event.payload, EventV1::TaskCancelled(data) if scheduled_task_ids.contains(&data.task_id))
                })
                .count()
                == 2
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let scheduled_task_ids = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("provider_model:mock:model-1")
                    && data.state == TaskScheduleState::Started =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        scheduled_task_ids.len(),
        2,
        "both child sessions should start in parallel under limit=2"
    );

    let queued = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if data.queue_key.as_deref() == Some("provider_model:mock:model-1")
                        && data.state == TaskScheduleState::Queued
            )
        })
        .count();
    assert_eq!(
        queued, 0,
        "no queueing expected with two slots and two children"
    );

    let completed = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCompleted(data) if scheduled_task_ids.contains(&data.task_id) => {
                Some(data.result_summary.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let cancelled = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCancelled(data) if scheduled_task_ids.contains(&data.task_id) => {
                Some(data.reason.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(completed.len(), 1, "one sibling should still complete");
    assert_eq!(cancelled.len(), 1, "one sibling failure should be isolated");
    assert!(
        completed
            .iter()
            .any(|summary| summary.contains("beta child ok")),
        "beta sibling should complete despite alpha failure"
    );
    assert!(
        cancelled
            .iter()
            .any(|reason| reason.contains("alpha child failed")),
        "alpha failure should be recorded without cancelling sibling"
    );
}
#[tokio::test]
async fn immediate_agent_turn_emits_single_started_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(5));

    let run = coordinator
        .start_run(
            "coord_agent_turn_started_immediate",
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
        .expect("request immediate turn");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        }) && events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let scheduled: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some((idx, event, data))
            }
            _ => None,
        })
        .collect();

    let started: Vec<_> = scheduled
        .iter()
        .filter(|(_, _, data)| data.state == TaskScheduleState::Started)
        .collect();
    let queued: Vec<_> = scheduled
        .iter()
        .filter(|(_, _, data)| data.state == TaskScheduleState::Queued)
        .collect();

    assert_eq!(
        started.len(),
        1,
        "immediate turns should emit one started event"
    );
    assert!(
        queued.is_empty(),
        "immediate turns should not emit queued events"
    );

    let (started_idx, started_event, started_data) = *started[0];
    assert_eq!(started_event.actor.kind, ActorKind::Worker);
    assert_eq!(
        started_event.actor.agent_id.as_deref(),
        Some(agent_id.as_str())
    );

    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_) if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
        .expect("provider request started event");
    assert!(
        started_idx < provider_started_idx,
        "started scheduling event should precede provider execution"
    );

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if data.task_id == started_data.task_id
                    && event.correlation_id.as_deref() == Some(request_id.as_str())
        )
    }));
}
#[tokio::test]
async fn queued_agent_turn_emits_started_when_dequeued() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(25));

    let run = coordinator
        .start_run(
            "coord_agent_turn_started_queued",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let alpha = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let beta = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .expect("spawn idle beta");

    let _first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), alpha, "alpha-prompt")
        .await
        .expect("request first turn");
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta.clone(), "beta-prompt")
        .await
        .expect("request queued turn");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        let scheduled = events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskScheduled(data)
                        if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                            && matches!(
                                data.state,
                                TaskScheduleState::Queued | TaskScheduleState::Started
                            )
                )
            })
            .count();
        let completed = events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
            )
        });
        scheduled == 2 && completed
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let scheduled: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str()) =>
            {
                Some((idx, event, data))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        scheduled.len(),
        2,
        "queued turns should emit queued then started"
    );
    assert_eq!(scheduled[0].2.state, TaskScheduleState::Queued);
    assert_eq!(scheduled[1].2.state, TaskScheduleState::Started);
    assert_eq!(scheduled[0].2.task_id, scheduled[1].2.task_id);

    for (_, event, _) in &scheduled {
        assert_eq!(event.actor.kind, ActorKind::Worker);
        assert_eq!(event.actor.agent_id.as_deref(), Some(beta.as_str()));
    }

    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_) if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
            )
        })
        .expect("provider request started event");
    assert!(
        scheduled[1].0 < provider_started_idx,
        "dequeue-time started event should be emitted before execution begins"
    );

    let task_id = scheduled[0].2.task_id.clone();
    let completed_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if data.task_id == task_id
                        && event.correlation_id.as_deref() == Some(queued_request_id.as_str())
            )
        })
        .expect("task completed event");
    assert!(provider_started_idx < completed_idx);
}
