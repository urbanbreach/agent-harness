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
        if width >= 48 {
            assert!(rows.iter().any(|row| row.contains("(ctrl+c to cancel)")));
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
        "Press Ctrl+c to cancel the turn"
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
    assert!(app.transcript_view.compaction_details_expanded);
    app.handle_key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(!app.transcript_view.compaction_details_expanded);
    assert!(intents.lock().unwrap_or_abort().is_empty());
}
