use super::*;
use crate::UnwrapOrAbort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    EventV1, ProviderRequestStartedEvent, ProviderStreamDeltaEvent, TaskScheduleState,
    TaskScheduledEvent,
};
use std::sync::{Arc, Mutex};

pub(super) fn capturing_live_app() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let captured = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        captured.lock().unwrap_or_abort().push(intent);
    });
    (AppState::new_live(None, false, Some(sink)), intents)
}

pub(super) fn active_turn(app: &mut AppState) {
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_active".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".into(),
            provider_id: "mock".into(),
            model_id: "p0-04-model".into(),
            prompt_summary: "active".into(),
            request_digest: "digest-p0-04-active".into(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_active",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_active".into(),
            delta: "streaming".into(),
        }),
    ));
}

/// Starts or ends one automatic maintenance pass: its lifecycle events and the progress
/// events of its card. Queued input flushes when the lifecycle ends.
pub(super) fn maintenance(app: &mut AppState, reason: &str, active: bool) {
    use harness_core::event::{
        CompactionLifecycleEvent, LifecyclePhase, LiveEventEnvelope, LiveEventV1, RuntimeEvent,
    };
    let reason = if reason == "manual" {
        "threshold"
    } else {
        reason
    };
    let live = |payload| {
        RuntimeEvent::Live(Box::new(LiveEventEnvelope {
            event_id: format!("queue-{active}"),
            run_id: "run_app_tests".into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("queue-agent".into())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("agent:queue-agent".into()),
            payload,
        }))
    };
    let lifecycle = |phase| CompactionLifecycleEvent {
        agent_id: "queue-agent".into(),
        operation_id: "queue-op".into(),
        phase,
        reason: reason.into(),
        action: "context-full".into(),
        will_retry: matches!(reason, "overflow" | "incomplete"),
        aborted: false,
        skipped: false,
        error: None,
        result: None,
    };
    let progress = live(LiveEventV1::CompactionProgress {
        agent_id: "queue-agent".into(),
        generation: 1,
        trigger_reason: reason.into(),
        preview: active.then(String::new),
        stage: None,
    });
    if active {
        app.ingest_runtime_event(live(LiveEventV1::CompactionLifecycle(lifecycle(
            LifecyclePhase::Start,
        ))));
        app.ingest_runtime_event(progress);
    } else {
        app.ingest_runtime_event(progress);
        app.ingest_runtime_event(live(LiveEventV1::CompactionLifecycle(lifecycle(
            LifecyclePhase::End,
        ))));
    }
}

#[test]
fn maintenance_flush_reparses_commands_and_preserves_requested_lanes() {
    for retry in [false, true] {
        assert_mixed_maintenance_flush(retry);
    }
}

fn queue_mixed_drafts(app: &mut AppState) {
    for (text, follow_up) in [
        ("/timestamps", false),
        ("first ordinary", false),
        ("/thinking", false),
        ("after yield", true),
    ] {
        app.handle_paste(text);
        if follow_up {
            app.submit_follow_up_prompt();
        } else {
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
    }
}

fn assert_mixed_maintenance_flush(retry: bool) {
    let (mut app, intents) = capturing_live_app();
    let timestamps = app.transcript_view.show_transcript_timestamps;
    let thinking = app.transcript_view.show_transcript_thinking;
    maintenance(&mut app, if retry { "overflow" } else { "manual" }, true);
    queue_mixed_drafts(&mut app);
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.maintenance_queue_lanes(), (3, 1));
    maintenance(&mut app, if retry { "overflow" } else { "manual" }, false);
    // The queue was snapshotted and emptied before the first delivery.
    assert_eq!(app.maintenance_queue_lanes(), (0, 0));
    assert_ne!(app.transcript_view.show_transcript_timestamps, timestamps);
    assert_eq!(app.transcript_view.show_transcript_thinking, thinking);
    let (id, first) = last_queued_submission(&intents);
    assert!(if retry {
        matches!(first, UiIntent::SteerPrompt { text, .. } if text == "first ordinary")
    } else {
        matches!(first, UiIntent::SubmitPrompt { text, .. } if text == "first ordinary")
    });
    app.maintenance_prompt_receipt(id, Ok(()));
    assert_mixed_maintenance_tail(&mut app, &intents, retry, thinking);
}

fn assert_mixed_maintenance_tail(
    app: &mut AppState,
    intents: &Arc<Mutex<Vec<UiIntent>>>,
    retry: bool,
    thinking: bool,
) {
    if retry {
        // Commands keep running locally in order when the retry delivers every item.
        assert_ne!(app.transcript_view.show_transcript_thinking, thinking);
    } else {
        // Text after the first ordinary prompt is delivered to its lane, never executed.
        assert_eq!(app.transcript_view.show_transcript_thinking, thinking);
        let (id, second) = last_queued_submission(intents);
        assert!(matches!(second, UiIntent::SteerPrompt { text, .. } if text == "/thinking"));
        app.maintenance_prompt_receipt(id, Ok(()));
    }
    let (id, last) = last_queued_submission(intents);
    assert!(matches!(last, UiIntent::FollowUpPrompt { text, .. } if text == "after yield"));
    app.maintenance_prompt_receipt(id, Ok(()));
    assert_eq!(app.maintenance_queue_lanes(), (0, 0));
    assert!(app.composer.maintenance_queue.awaiting.is_none());
}

#[test]
fn maintenance_flush_executes_command_only_queue_in_order() {
    let (mut app, intents) = capturing_live_app();
    let timestamps = app.transcript_view.show_transcript_timestamps;
    let thinking = app.transcript_view.show_transcript_thinking;
    maintenance(&mut app, "manual", true);
    for command in ["/timestamps", "/thinking"] {
        app.handle_paste(command);
        app.submit_prompt();
    }
    maintenance(&mut app, "manual", false);
    assert_ne!(app.transcript_view.show_transcript_timestamps, timestamps);
    assert_ne!(app.transcript_view.show_transcript_thinking, thinking);
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn last_queued_submission(intents: &Arc<Mutex<Vec<UiIntent>>>) -> (u64, UiIntent) {
    intents
        .lock()
        .unwrap_or_abort()
        .last()
        .and_then(|intent| match intent {
            UiIntent::QueuedPrompt { id, prompt } => Some((*id, *prompt.clone())),
            _ => None,
        })
        .unwrap_or_abort()
}

#[test]
fn failed_maintenance_admission_restores_attachments_and_remaining_draft(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("attachment.txt");
    std::fs::write(&path, "retained bytes")?;
    let ingestor = crate::attachment_lifecycle::AttachmentIngestor::new(
        crate::attachment_lifecycle::AttachmentPolicy::new(temp.path())?,
    );
    let (mut app, intents) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    app.handle_paste("attached queued");
    app.file_mention_tags.push(FileMentionTag {
        start: 0,
        end: 8,
        part: file_mentions::FileMentionSelectedTag::Agent(
            harness_core::file_tag::SelectedAgentTag {
                name: "worker".into(),
                source: harness_core::file_tag::FileTagSource {
                    start: 0,
                    end: 8,
                    value: "attached".into(),
                },
            },
        ),
    });
    app.composer_attach(
        crate::composer_atoms::AttachmentId::new(1),
        ingestor.ingest_file(
            &path,
            &crate::attachment_lifecycle::CancellationToken::new(),
        )?,
    )?;
    app.submit_prompt();
    assert!(app.composer.slice.attachments().is_empty());
    app.handle_paste("follow-up queued");
    app.submit_follow_up_prompt();
    app.clear_prompt_input();
    app.handle_paste("existing draft");
    app.composer_attach(
        crate::composer_atoms::AttachmentId::new(1),
        ingestor.ingest_file(
            &path,
            &crate::attachment_lifecycle::CancellationToken::new(),
        )?,
    )?;
    maintenance(&mut app, "manual", false);
    let id = match intents.lock().unwrap_or_abort().last() {
        Some(UiIntent::QueuedPrompt { id, .. }) => *id,
        _ => return Err("missing queued receipt".into()),
    };
    app.maintenance_prompt_receipt(id, Err("admission refused".into()));
    assert_eq!(
        app.toast().ok_or("missing failure toast")?.message,
        "Failed to send queued messages: admission refused"
    );
    assert_eq!(app.composer.slice.attachments().len(), 2);
    assert_eq!(app.composer.slice.attachments()[0].id.get(), 1);
    assert_eq!(app.composer.slice.attachments()[1].id.get(), 2);
    assert!(app.composer.prompt_buffer.starts_with(
        "attached queued[attachment:2]\n\nfollow-up queued\n\nexisting draft[attachment:1]"
    ));
    assert_eq!(
        app.composer.slice.attachments()[1].attachment.bytes(),
        b"retained bytes"
    );
    assert_eq!(app.selected_agent_tags()[0].name, "worker");
    assert_eq!(app.selected_agent_tags()[0].source.start, 0);
    assert_eq!(app.maintenance_queue_lanes(), (0, 0));
    Ok(())
}

#[test]
fn dequeue_maintenance_input_restores_steering_before_follow_up() {
    let (mut app, _) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    app.handle_paste("after yield");
    app.submit_follow_up_prompt();
    assert_eq!(app.composer.prompt_buffer, "after yield");
    app.clear_prompt_input();
    app.handle_paste("steering");
    app.submit_prompt();
    app.handle_paste("draft");
    app.execute_action(Action::DequeuePrompt);
    assert_eq!(
        app.composer.prompt_buffer,
        "steering\n\nafter yield\n\ndraft"
    );
    assert_eq!(app.maintenance_queue_lanes(), (0, 0));
}

#[test]
fn maintenance_custom_command_expands_at_dispatch_not_capture() {
    let (mut app, intents) = capturing_live_app();
    app.prompt_commands
        .push(harness_core::commands::PromptCommand {
            name: "greet".into(),
            description: String::new(),
            argument_hint: None,
            template: "Hello $1".into(),
        });
    maintenance(&mut app, "manual", true);
    app.handle_paste("/greet friend");
    app.submit_prompt();
    assert_eq!(
        app.composer.prompt_history.last().map(String::as_str),
        Some("/greet friend")
    );
    maintenance(&mut app, "manual", false);
    assert!(
        matches!(intents.lock().unwrap_or_abort().last(), Some(UiIntent::QueuedPrompt { prompt, .. })
        if matches!(prompt.as_ref(), UiIntent::SubmitPrompt { text, .. } if text == "Hello friend"))
    );
}

#[test]
fn maintenance_single_failure_keeps_draft_and_singular_error() {
    let (mut app, intents) = capturing_live_app();
    maintenance(&mut app, "manual", true);
    app.handle_paste("queued");
    app.submit_prompt();
    app.handle_paste("draft");
    maintenance(&mut app, "manual", false);
    let id = intents
        .lock()
        .unwrap_or_abort()
        .last()
        .and_then(|intent| match intent {
            UiIntent::QueuedPrompt { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap_or_abort();
    app.maintenance_prompt_receipt(id, Err("refused".into()));
    assert_eq!(app.composer.prompt_buffer, "queued\n\ndraft");
    assert_eq!(
        app.toast().unwrap_or_abort().message,
        "Failed to send queued message: refused"
    );
}

fn intent_count(intents: &[UiIntent], predicate: impl Fn(&UiIntent) -> bool) -> usize {
    intents.iter().filter(|intent| predicate(intent)).count()
}

#[test]
fn multiline_enter_inserts_newline() {
    // Given: multiline mode with a draft in the focused composer.
    let (mut app, intents) = capturing_live_app();
    app.composer.multiline_mode = true;
    app.handle_paste("alpha");

    // When: Enter is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Then: the newline is inserted without submitting.
    assert_eq!(app.composer.prompt_buffer, "alpha\n");
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

#[test]
fn multiline_shift_enter_submits_once() {
    // Given: multiline mode with a valid draft in the focused composer.
    let (mut app, intents) = capturing_live_app();
    app.composer.multiline_mode = true;
    app.handle_paste("alpha");

    // When: Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    // Then: exactly one prompt submission is emitted.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
    assert_eq!(intents.len(), 1);
}

#[test]
fn multiline_alt_enter_submits_once() {
    // Given: multiline mode with a valid draft in the focused composer.
    let (mut app, intents) = capturing_live_app();
    app.composer.multiline_mode = true;
    app.handle_paste("alpha");

    // When: the terminal-safe explicit send chord is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));

    // Then: exactly one prompt submission is emitted.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
    assert_eq!(intents.len(), 1);
}

#[test]
fn active_turn_submit_steers_and_follow_up_queues() -> Result<(), Box<dyn std::error::Error>> {
    let alt_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT);
    let ctrl_alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::ALT);
    for (multiline, key, steers) in [
        (
            false,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            true,
        ),
        (true, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true),
        (false, alt_i, false),
        (false, ctrl_alt_enter, false),
        (true, ctrl_alt_enter, false),
    ] {
        let (mut app, intents) = capturing_live_app();
        active_turn(&mut app);
        app.composer.multiline_mode = multiline;
        app.handle_paste("draft sentinel");
        app.handle_key(key);

        let intents = intents.lock().map_err(|_| "intent lock poisoned")?;
        let (expected, echo) = if steers {
            (
                matches!(intents.as_slice(), [UiIntent::SteerPrompt { text, .. }] if text == "draft sentinel"),
                ActivityStatus::Done,
            )
        } else {
            (
                matches!(intents.as_slice(), [UiIntent::SubmitPrompt { text, .. }] if text == "draft sentinel"),
                ActivityStatus::Queued,
            )
        };
        assert!(expected, "{multiline} {key:?}: {intents:?}");
        assert_eq!(app.activities.back().ok_or("missing echo")?.status, echo);
        assert_eq!(
            app.runtime_state_activity()
                .ok_or("missing running turn")?
                .request_id,
            "req_active"
        );
        assert_eq!(
            app.activities
                .iter()
                .filter(|activity| activity.status == ActivityStatus::Streaming)
                .count(),
            1
        );
        assert!(app.composer.prompt_buffer.is_empty());
    }
    Ok(())
}

#[test]
fn idle_submit_and_tagged_or_attached_drafts_submit_normally(
) -> Result<(), Box<dyn std::error::Error>> {
    for key in [
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT),
    ] {
        let (mut idle, intents) = capturing_live_app();
        idle.handle_paste("idle sentinel");
        idle.handle_key(key);
        assert!(
            matches!(intents.lock().map_err(|_| "intent lock poisoned")?.as_slice(),
            [UiIntent::SubmitPrompt { text, .. }] if text == "idle sentinel")
        );
    }

    for query in ["@main", "@expl", "@guide"] {
        let (mut app, intents) = capturing_live_app();
        active_turn(&mut app);
        app.set_file_mention_collaborators_for_test(
            PathBuf::from("/virtual/workspace"),
            vec!["main.rs".into()],
            123,
        );
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("build", "mock:model-1")
                .with_available_models(vec![
                    ModelOption::from_model_ref("build", "mock:model-1"),
                    ModelOption::from_model_ref("explore", "mock:model-1"),
                ])
                .with_switchable_profiles(vec!["build".into()])
                .with_mcp_resources(vec![McpResourceOption {
                    name: "Docs Guide".into(),
                    uri: "mcp://docs/guide".into(),
                    mime: "text/markdown".into(),
                    description: None,
                }]),
        );
        for ch in query.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let intents = intents.lock().map_err(|_| "intent lock poisoned")?;
        let [UiIntent::SubmitPrompt {
            selected_file_tags,
            selected_agent_tags,
            selected_resource_tags,
            ..
        }] = intents.as_slice()
        else {
            return Err(
                format!("tagged draft did not submit normally: {query}: {intents:?}").into(),
            );
        };
        assert_eq!(
            selected_file_tags.len() + selected_agent_tags.len() + selected_resource_tags.len(),
            1
        );
        assert_eq!(
            app.activities.back().ok_or("missing echo")?.status,
            ActivityStatus::Queued
        );
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("attached.txt");
    std::fs::write(&path, "attachment sentinel")?;
    let ingestor = crate::attachment_lifecycle::AttachmentIngestor::new(
        crate::attachment_lifecycle::AttachmentPolicy::new(temp.path())?,
    );
    let attachment = ingestor.ingest_file(
        &path,
        &crate::attachment_lifecycle::CancellationToken::new(),
    )?;
    let (mut app, intents) = capturing_live_app();
    active_turn(&mut app);
    app.handle_paste("attachment prompt");
    app.composer_attach(crate::composer_atoms::AttachmentId::new(1), attachment)?;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(intents.lock().map_err(|_| "intent lock poisoned")?.as_slice(),
        [UiIntent::SubmitPrompt { attachments, .. }] if attachments.len() == 1)
    );
    assert!(app.composer.slice.attachments().is_empty());
    app.handle_paste("next unattached draft");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(intents.lock().map_err(|_| "intent lock poisoned")?.last(), Some(UiIntent::SteerPrompt { text, .. }) if text == "next unattached draft")
    );
    Ok(())
}

#[test]
fn interrupt_returns_queued_input_to_the_editor_ahead_of_the_draft(
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _intents) = capturing_live_app();
    active_turn(&mut app);
    app.handle_paste("steer me");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_paste("current draft");
    let (tx, rx) = crate::live_update_channel();
    tx.send(crate::LiveUpdate::RestoreQueuedInput(vec![
        "queued follow-up".into(),
        "steer me".into(),
    ]))
    .map_err(|_| "live update channel closed")?;
    crate::runtime_live_updates::drain_live_updates(&mut app, &rx);

    assert_eq!(
        app.composer.prompt_buffer,
        "queued follow-up\n\nsteer me\n\ncurrent draft"
    );
    assert!(app.activities.iter().all(|activity| activity
        .user_message
        .as_ref()
        .is_none_or(|message| message.text != "steer me")));
    assert!(app.toast().is_some());
    Ok(())
}

#[test]
fn coordinator_turn_stays_steerable_after_its_request_finishes_for_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    // The coordinator queues agent turns under the agent's id, which owns the task.
    let (mut app, intents) = capturing_live_app();
    for (seq, payload) in [
        EventV1::RunStarted(harness_core::event::RunStartedEvent {
            run_name: "tool turn".into(),
            workspace_root: "/workspace".into(),
        }),
        EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
            request_id: "turn-5".into(),
            text: "run a tool".into(),
        }),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "turn-5".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("app-tests".into()),
            metadata: None,
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider-1".into(),
            provider_id: "mock".into(),
            model_id: "p0-04-model".into(),
            prompt_summary: "run a tool".into(),
            request_digest: "digest-tool-turn".into(),
            metadata: None,
        }),
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: "provider-1".into(),
            finish_reason: "tool_use".into(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ]
    .into_iter()
    .enumerate()
    {
        app.ingest_event(envelope(seq as u64 + 1, "turn-5", payload));
    }
    assert!(app.active_turn_in_progress());

    app.handle_paste("steer during tools");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(intents.lock().map_err(|_| "intent lock poisoned")?.as_slice(),
        [UiIntent::SteerPrompt { text, .. }] if text == "steer during tools")
    );

    // The turn's next tool call belongs to the turn, not to the steering echo.
    app.ingest_event(envelope(
        7,
        "turn-5",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-after-steer".into(),
            tool_id: "bash".into(),
            args_summary: r#"{"command":"cargo test"}"#.into(),
            args_digest: "digest-after-steer".into(),
            metadata: None,
        }),
    ));
    let owner = app
        .activities
        .iter()
        .find(|activity| {
            activity
                .tool_calls
                .iter()
                .any(|tool| tool.tool_call_id == "tool-after-steer")
        })
        .ok_or("tool call was dropped")?;
    assert_eq!(owner.request_id, "turn-5");
    Ok(())
}

#[test]
fn active_turn_cancel_replace_interrupts_before_submitting() {
    // Given: an active turn and a valid draft.
    let (mut app, intents) = capturing_live_app();
    active_turn(&mut app);
    app.handle_paste("replace this");

    // When: Ctrl+Enter is pressed.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    // Then: the active task is interrupted before exactly one submission.
    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(intents.len(), 2);
    assert!(matches!(
        intents.first(),
        Some(UiIntent::InterruptSession { .. })
    ));
    assert!(matches!(
        intents.get(1),
        Some(UiIntent::SubmitPrompt { .. })
    ));
    assert_eq!(
        intent_count(&intents, |intent| {
            matches!(intent, UiIntent::SubmitPrompt { .. })
        }),
        1
    );
}

#[test]
fn cancel_replace_empty_draft_emits_no_intents() {
    // Given: an active turn and an empty draft.
    let (mut app, intents) = capturing_live_app();
    active_turn(&mut app);

    // When: Ctrl+Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    // Then: neither interrupt nor submission is emitted and the draft stays empty.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert!(app.composer.prompt_buffer.is_empty());
}

#[test]
fn cancel_replace_disconnected_configured_profile_preserves_draft_and_error() {
    // Given: a configured profile with no connected provider and a draft.
    let (mut app, intents) = capturing_live_app();
    app.set_launch_metadata(LaunchMetadata::disconnected("configured"));
    app.handle_paste("keep this draft");

    // When: Ctrl+Shift+Enter is pressed.
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    // Then: the draft remains, the disconnect error is visible, and no intent is emitted.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    assert!(app.status_banner.is_some());
}

#[test]
fn replay_multiline_actions_remain_read_only() {
    // Given: replay mode with a focused multiline draft.
    let (mut app, intents) = capturing_live_app();
    app.replay_mode = true;
    app.focus = Focus::Prompt;
    app.composer.multiline_mode = true;
    app.composer.prompt_buffer = "replay draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.len();

    // When: every P0-04 submission action is pressed.
    for key in [
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
    ] {
        app.handle_key(key);
    }

    // Then: replay emits no runtime intent and preserves its draft.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.composer.prompt_buffer, "replay draft");
}

#[test]
fn multiline_getter_badge_and_queue_state_are_visible() {
    // Given: multiline mode and an existing queued prompt.
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_ref(
        "p0-04-model",
        "mock:p0-04-model",
    ));
    app.composer.multiline_mode = true;
    let idle = render_text(&app, 100, 30);
    assert!(idle.contains("Alt+Enter:send"), "{idle}");
    assert!(!idle.contains(" Enter:send"), "{idle}");
    active_turn(&mut app);
    app.handle_paste("draft");
    app.queued_prompt_count = 1;
    assert!(app.has_live_turn_activity());

    // When: the visible composer state is queried and rendered.
    let rendered = render_text(&app, 100, 30);

    // Then: the getter and existing queue state agree with the visible state.
    assert!(app.composer.composer_multiline_mode());
    assert_eq!(app.queued_prompt_count, 1);
    assert!(!rendered.contains("queued 1"));
    assert_eq!(rendered.matches("MULTILINE").count(), 1);
    assert!(!rendered.contains(" · multiline"));
    assert!(
        !rendered.contains("Enter:queue"),
        "multiline footer must not advertise Enter as queue\n{rendered}"
    );
    assert!(
        rendered.contains("Enter:newline"),
        "multiline footer missing newline action\n{rendered}"
    );
    assert!(
        rendered.contains("Alt+Enter:steer"),
        "multiline footer missing steer action\n{rendered}"
    );
    assert!(rendered.contains("Alt+i:follow-up"), "{rendered}");
    assert!(rendered.contains("Alt+r:replace"));
}
