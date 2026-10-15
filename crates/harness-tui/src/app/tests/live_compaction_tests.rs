use super::*;
use harness_core::event::{LiveEventEnvelope, LiveEventV1, RuntimeEvent};

fn progress(generation: u64, reason: &str, preview: Option<&str>) -> RuntimeEvent {
    RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: format!("compaction-{generation}"),
        run_id: "test-run".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-alpha".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("agent:agent-alpha".to_string()),
        payload: LiveEventV1::CompactionProgress {
            agent_id: "agent-alpha".to_string(),
            generation,
            trigger_reason: reason.to_string(),
            preview: preview.map(str::to_string),
            stage: None,
        },
    }))
}

#[test]
fn compaction_stream_is_one_row_cancellable_and_generation_fenced() {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            output.lock().unwrap_or_abort().push(intent);
        })),
    );
    app.ingest_event(provider_started(1, "req", "mock", "model"));
    for activity in &mut app.activities {
        activity.status = ActivityStatus::Done;
    }
    for (generation, reason, preview, width) in [
        (1, "manual", "", 100),
        (2, "overflow", "", 100),
        (3, "pre_prompt", "", 100),
        (4, "threshold", "", 100),
        (
            5,
            "manual",
            "Older preview \u{1b}[31m\nKeep the user’s request · 最新 😀 e\u{301}",
            100,
        ),
        (6, "manual", "Older preview · 最新 😀 e\u{301}", 48),
        (7, "manual", "Older preview · 最新 😀 e\u{301}", 32),
    ] {
        app.ingest_runtime_event(progress(generation, reason, Some(preview)));
        assert!(app.live_turn_status_visible());
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap_or_abort();
        terminal
            .draw(|frame| render_app(frame, &app))
            .unwrap_or_abort();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..24)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|row| row.to_lowercase().contains("compacting"))
                .count(),
            1,
            "{rows:?}"
        );
        if width >= 100 {
            assert!(rows.iter().any(|row| row.contains("(Ctrl+C to cancel)")));
        } else if width >= 48 {
            assert!(rows.iter().any(|row| row.contains("Ctrl+C")));
        }
        assert!(rows.iter().all(|row| !row.contains('\u{1b}')));
        if let Ok(dir) = std::env::var("HARNESS_COMPACTION_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).unwrap_or_abort();
            let cells: Vec<_> = (0..24).flat_map(|y| (0..width).map(move |x| {
                let cell = &buffer[(x,y)];
                serde_json::json!({"x": x, "y": y, "text": cell.symbol(), "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg)})
            })).collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("stream-{generation}-{width}.json")),
                serde_json::to_vec(
                    &serde_json::json!({"width": width, "height": 24, "cells": cells}),
                )
                .unwrap_or_abort(),
            )
            .unwrap_or_abort();
        }
    }
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.toast().unwrap_or_abort().message,
        "Press Ctrl+C to cancel the turn"
    );
    app.handle_key(KeyEvent::new_with_kind(
        KeyCode::Esc,
        KeyModifiers::NONE,
        crossterm::event::KeyEventKind::Release,
    ));
    assert!(!intents
        .lock()
        .unwrap_or_abort()
        .iter()
        .any(|intent| matches!(intent, UiIntent::CancelCompaction { .. })));
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(
        matches!(intents.lock().unwrap_or_abort().last(), Some(UiIntent::CancelCompaction { agent_id }) if agent_id == "agent-alpha")
    );
    app.ingest_runtime_event(progress(7, "manual", None));
    app.ingest_runtime_event(progress(6, "manual", Some("late older chunk")));
    app.ingest_runtime_event(progress(7, "manual", Some("late completed chunk")));
    assert!(app.active_compaction().is_none());
    app.ingest_runtime_event(progress(8, "manual", Some("fresh generation")));
    assert!(app.active_compaction().is_some());
    app.replay_mode = true;
    assert!(app.active_compaction().is_none());
}

#[test]
fn compaction_details_shortcut_does_not_change_permission_mode() {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            output.lock().unwrap_or_abort().push(intent);
        })),
    );
    app.handle_key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    app.handle_key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

fn command_app(messages: usize) -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            output.lock().unwrap_or_abort().push(intent);
        })),
    );
    for index in 0..messages {
        let request = format!("message-{index}");
        app.ingest_event(envelope(
            u64::try_from(index).unwrap_or_abort() + 1,
            &request,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request.clone().into(),
                text: "Retain the session contract".into(),
            }),
        ));
    }
    for activity in &mut app.activities {
        activity.status = ActivityStatus::Done;
    }
    (app, intents)
}

fn submit_maintenance_command(app: &mut AppState, text: &str) {
    app.replace_prompt_input(text.to_string());
    app.sync_slash_overlay();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn maintenance_commands_emit_typed_modes_and_preserve_focus() {
    use harness_compaction::{ManualMode, ShakeMode};
    for (command, expected) in [
        (
            "/compact",
            UiIntent::CompactSession {
                instructions: None,
                mode: None,
            },
        ),
        (
            "/compact SOFT keep API notes",
            UiIntent::CompactSession {
                instructions: Some("keep API notes".into()),
                mode: Some(ManualMode::Soft),
            },
        ),
        (
            "/compact remote",
            UiIntent::CompactSession {
                instructions: None,
                mode: Some(ManualMode::Remote),
            },
        ),
        (
            "/compact archive",
            UiIntent::CompactSession {
                instructions: None,
                mode: Some(ManualMode::Archive),
            },
        ),
        (
            "/compact softly keep notes",
            UiIntent::CompactSession {
                instructions: Some("softly keep notes".into()),
                mode: None,
            },
        ),
        ("/handoff", UiIntent::Handoff { focus: None }),
        (
            "/handoff preserve API notes",
            UiIntent::Handoff {
                focus: Some("preserve API notes".into()),
            },
        ),
        (
            "/shake",
            UiIntent::Shake {
                mode: ShakeMode::Elide,
            },
        ),
        (
            "/shake IMAGES",
            UiIntent::Shake {
                mode: ShakeMode::Images,
            },
        ),
        (
            "/shake thinking",
            UiIntent::Shake {
                mode: ShakeMode::Thinking,
            },
        ),
    ] {
        let (mut app, intents) = command_app(2);
        submit_maintenance_command(&mut app, command);
        assert_eq!(
            intents.lock().unwrap_or_abort().as_slice(),
            &[expected],
            "{command}"
        );
        assert!(app.composer.prompt_buffer.is_empty(), "{command}");
    }
}

#[test]
fn maintenance_command_guards_clear_submitted_text_without_emitting_intents() {
    for (messages, command, warning) in [
        (0, "/compact", "Nothing to compact (no messages yet)"),
        (1, "/compact soft", "Nothing to compact (no messages yet)"),
        (0, "/handoff", "Nothing to hand off (no messages yet)"),
        (2, "/compact archive preserve notes", "/compact archive does not take focus instructions (it archives history without an LLM summary)."),
        (2, "/shake images extra", "Unknown /shake mode \"images extra\". Use elide, images, or thinking."),
    ] {
        let (mut app, intents) = command_app(messages);
        submit_maintenance_command(&mut app, command);
        assert!(intents.lock().unwrap_or_abort().is_empty(), "{command}");
        assert_eq!(
            app.transcript_notices().last().map(|notice| notice.text.as_str()),
            Some(warning),
            "{command}"
        );
        assert!(app.composer.prompt_buffer.is_empty(), "{command}");
    }
    let (mut app, intents) = command_app(2);
    app.ingest_runtime_event(progress(1, "manual", Some("retained preview")));
    submit_maintenance_command(&mut app, "/handoff notes");
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.maintenance_queue_lanes(), (1, 0));
    assert!(app.composer.prompt_buffer.is_empty());
    let (mut app, intents) = command_app(2);
    app.ingest_event(provider_started(3, "message-1", "mock", "model"));
    submit_maintenance_command(&mut app, "/handoff notes");
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(
        app.transcript_notices()
            .last()
            .map(|notice| notice.text.as_str()),
        Some("Wait for the current response to finish or abort it before handing off.")
    );
}

#[test]
fn maintenance_autocomplete_accepts_modes_without_executing_them() {
    for (query, completion, description) in [
        ("/compact so", "/compact soft ", "Summarize locally"),
        ("/compact re", "/compact remote ", "provider-native"),
        ("/compact ar", "/compact archive ", "bitmap frames"),
        ("/shake el", "/shake elide ", "tool results"),
        ("/shake im", "/shake images ", "image blocks"),
        ("/shake th", "/shake thinking ", "thinking blocks"),
    ] {
        let (mut app, intents) = command_app(2);
        app.replace_prompt_input(query.into());
        app.sync_slash_overlay();
        assert!(app.slash_visible, "{query}");
        let screen = render_text(&app, 120, 40);
        assert!(screen.contains(description), "{query}: {screen}");
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.composer.prompt_buffer, completion, "{query}");
        assert!(intents.lock().unwrap_or_abort().is_empty(), "{query}");
    }
}
