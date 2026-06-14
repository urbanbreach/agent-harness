use super::*;

use std::sync::{Arc, Mutex};

fn transcript_activity(index: usize, text: &str) -> ActivityEntry {
    ActivityEntry {
        request_id: format!("req_nav_{index:02}"),
        revision: 1,
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(UserMessageSubmittedEvent {
            request_id: format!("req_nav_{index:02}"),
            text: format!("User message {index}"),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: text.to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: index as u64,
        last_seq: index as u64,
        first_mono_ms: index as u64,
        last_mono_ms: index as u64,
    }
}

fn transcript_navigation_app() -> AppState {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/harness-sessions/run_nav")),
        false,
        None,
    );
    app.activities = std::collections::VecDeque::from(vec![
        transcript_activity(1, "First assistant reply"),
        transcript_activity(2, "Second assistant reply"),
        transcript_activity(3, "Third assistant reply"),
        transcript_activity(4, "Fourth assistant reply"),
    ]);
    app.focus = Focus::Details;
    app.selected_activity_index = 0;
    app
}

pub(super) fn transcript_message_jumps_use_cached_measured_section_top_rows() {
    let mut app = transcript_navigation_app();
    let frame_area = Rect::new(0, 0, 100, 20);
    app.set_frame_area(frame_area);
    AppState::reset_transcript_perf_counters_for_test();

    let _rendered = render_debug(&app, frame_area.width, frame_area.height);
    let section_top_rows = crate::ui::transcript_message_top_rows(&app, frame_area);
    assert!(
        section_top_rows.len() >= 4,
        "test fixture should expose four measured message sections"
    );
    let warmed_builds = AppState::transcript_layout_section_build_count_for_test();
    app.handle_key(key_with_modifiers(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.transcript_view.transcript_scroll,
        app.transcript_view
            .last_transcript_max_scroll
            .get()
            .saturating_sub(section_top_rows[0])
    );
    assert!(!app.transcript_view.follow_mode);

    app.handle_key(key_with_modifiers(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert!(app.transcript_view.follow_mode);

    app.handle_key(key_with_modifiers(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(
        app.transcript_view.transcript_scroll,
        app.transcript_view
            .last_transcript_max_scroll
            .get()
            .saturating_sub(*section_top_rows.last().expect("last section"))
    );
    assert!(!app.transcript_view.follow_mode);

    assert_eq!(
        AppState::transcript_layout_section_build_count_for_test() - warmed_builds,
        0,
        "message jumps should reuse cached measured layout sections"
    );
}

pub(super) fn transcript_copy_message_session_and_export_emit_exact_outputs() {
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        sink.lock()
            .expect("lock copied transcript")
            .push(text.to_string());
        Ok(())
    })));

    let emitted = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = Arc::clone(&emitted);
    let mut app = transcript_navigation_app();
    app.on_ui_intent = Some(Arc::new(move |intent| {
        intent_sink
            .lock()
            .expect("lock emitted intents")
            .push(intent);
    }));
    app.selected_activity_index = 1;

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Char('y')));
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key_with_modifiers(KeyCode::Char('y'), KeyModifiers::SHIFT));
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Char('x')));

    let copied = copied.lock().expect("lock copied values").clone();
    assert_eq!(
        copied.first().map(String::as_str),
        Some("Second assistant reply")
    );
    assert!(
        copied
            .get(1)
            .is_some_and(|text| text.contains("User message 1")
                && text.contains("Second assistant reply")
                && text.contains("Fourth assistant reply")),
        "copy session should use a plain-text transcript rendering"
    );
    assert_eq!(
        emitted.lock().expect("lock emitted intents").as_slice(),
        &[UiIntent::ExportSession {
            session: "run_nav".to_string(),
            output: PathBuf::from("/tmp/harness-sessions/run_nav.export.json"),
        }]
    );
    assert_eq!(
        app.toast().map(|toast| toast.message.as_str()),
        Some("Exporting session to /tmp/harness-sessions/run_nav.export.json")
    );

    crate::clipboard::set_copy_override(None);
}

pub(super) fn transcript_scrollbar_toggle_persists_and_replay_blocks_live_export() {
    let mut app = transcript_navigation_app();
    assert!(app.transcript_view.transcript_scrollbar_visible());

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Char('z')));
    assert!(!app.transcript_view.transcript_scrollbar_visible());

    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Char('z')));
    assert!(app.transcript_view.transcript_scrollbar_visible());

    let emitted = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = Arc::clone(&emitted);
    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    replay.on_ui_intent = Some(Arc::new(move |intent| {
        intent_sink
            .lock()
            .expect("lock emitted replay intents")
            .push(intent);
    }));
    replay.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    replay.handle_key(key(KeyCode::Char('x')));

    assert!(
        emitted
            .lock()
            .expect("lock replay emitted intents")
            .is_empty(),
        "replay mode must not emit live export intents"
    );
}

pub(super) fn transcript_navigation_edges_clamp_empty_and_stale_rows_without_rebuilds() {
    let mut empty = AppState::new_live(
        Some(PathBuf::from("/tmp/harness-sessions/run_empty")),
        false,
        None,
    );
    AppState::reset_transcript_perf_counters_for_test();

    assert!(!empty.jump_to_first_message());
    assert!(!empty.jump_to_last_message());
    assert!(!empty.jump_to_previous_message());
    assert!(!empty.jump_to_next_message());
    assert!(!empty.jump_to_last_user_message());
    assert_eq!(empty.selected_activity_index, 0);
    assert_eq!(
        AppState::transcript_layout_section_build_count_for_test(),
        0,
        "empty navigation must not rebuild transcript sections"
    );

    let mut stale = transcript_navigation_app();
    stale
        .transcript_view
        .set_transcript_message_top_rows(vec![8]);
    stale.transcript_view.last_transcript_max_scroll.set(40);

    assert!(stale.jump_to_last_user_message());
    assert_eq!(
        stale.selected_activity_index, 0,
        "stale cached message rows should clamp to the last measured row"
    );
    assert_eq!(stale.transcript_view.transcript_scroll, 32);
    stale
        .transcript_view
        .set_transcript_message_top_rows(Vec::new());
    assert!(!stale.jump_to_next_message());
}
