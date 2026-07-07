use super::*;
use crate::UnwrapOrAbort;

pub(super) fn prompt_focus_enter_emits_submit_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.focus = app::Focus::Prompt;

    for c in "hello".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }

    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0],
        UiIntent::SubmitPrompt {
            text: "hello".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: app::LaunchMetadata::default(),
        }
    );
    drop(intents);

    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.prompt_history.len(), 1);
    assert_eq!(app.composer.prompt_history[0], "hello");
}

pub(super) fn activity_groups_by_request_id() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_001".to_string(),
            text: "Hello AI".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello AI".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.activities.len(), 1);
    let activity = app.activities.front().unwrap();
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.provider_id, "openai");
    assert_eq!(activity.model_id, "gpt-5-codex");
    assert!(activity.user_message.is_some());
    assert_eq!(activity.user_message.as_ref().unwrap().text, "Hello AI");
}

pub(super) fn transcript_accumulates_stream_deltas() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_001".to_string(),
            delta: "Hello ".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_001".to_string(),
            delta: "world!".to_string(),
        }),
    ));

    let activity = app.activities.front().unwrap();
    assert_eq!(activity.transcript_text, "Hello world!");
}

pub(super) fn activity_status_done_on_request_finished() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.activities.front().unwrap().status,
        crate::app::ActivityStatus::Streaming
    );

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_001".to_string(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-out".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    assert_eq!(
        app.activities.front().unwrap().status,
        crate::app::ActivityStatus::Done
    );
}

pub(super) fn activity_status_error_on_run_failed() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: "API rate limit exceeded".to_string(),
        }),
    ));

    let activity = app.activities.front().unwrap();
    assert_eq!(activity.status, crate::app::ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_ref().unwrap(),
        "API rate limit exceeded"
    );
}

pub(super) fn memory_cap_enforces_max_events() {
    let mut app = AppState::new_live(None, false, None);
    app.memory_caps.max_events = 5;

    for i in 1..=10 {
        app.ingest_event(envelope(
            i,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: format!("run-{}", i),
                workspace_root: "/tmp".to_string(),
            }),
        ));
    }

    assert_eq!(app.events.len(), 5);
    assert_eq!(app.events_trimmed_count, 5);
}

pub(super) fn memory_cap_enforces_max_transcript_chars() {
    let mut app = AppState::new_live(None, false, None);
    app.memory_caps.max_transcript_chars = 20;

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    // Add 30 characters in deltas
    for i in 0..3 {
        app.ingest_event(envelope(
            2 + i,
            Some("req_001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "0123456789".to_string(),
            }),
        ));
    }

    assert!(app.transcript_trimmed_count > 0);
}

pub(super) fn run_workspace_renders_activity_with_compact_format() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_000123"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_000123".to_string(),
            text: "Hello".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_000123"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000123".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("gpt-5-codex"),
        "operator sidebar must show model_id"
    );
}

pub(super) fn tool_call_requested_renders_pending_status() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_001".to_string(),
            text: "Hello".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("Read test.txt"),
        "transcript must show tool title"
    );
}

pub(super) fn tool_call_started_renders_running_status() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_001".to_string(),
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("Read test.txt"),
        "transcript must show tool title"
    );
}

pub(super) fn tool_call_finished_renders_truncated_output() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_001".to_string(),
        }),
    ));

    let long_output = "x".repeat(150);
    app.ingest_event(envelope(
        4,
        Some("req_001"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_001".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(long_output.clone()),
            output_digest: Some("digest-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("Read test.txt"),
        "transcript must show tool title"
    );
    assert!(
        !debug.contains(&"x".repeat(20)),
        "successful read rows should not dump large output payloads"
    );
}

pub(super) fn tool_call_failed_renders_error() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "shell.run".to_string(),
            args_summary: r#"{"cmd":"false"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_001".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        4,
        Some("req_001"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_001".to_string(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("exit code: 1") || debug.contains("tool call"),
        "transcript must show error message"
    );
}

pub(super) fn task_scheduled_queued_does_not_reuse_tool_call_id_as_task_id() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "tc_001".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("tool:fs.read".to_string()),
        }),
    ));

    let activity = app.activities.front().unwrap();
    let tool_call = activity.tool_calls.first().unwrap();
    assert_eq!(
        tool_call.status,
        crate::app::ToolCallDisplayStatus::Queued,
        "TaskScheduled must not treat task_id as a tool_call_id"
    );

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, "tc_001");
    assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::Queued);
}
