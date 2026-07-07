use super::super::*;
use crate::UnwrapOrAbort;
use harness_core::event::UserMessageSubmittedEvent;

#[test]
fn queued_runtime_status_without_pending_assistant_does_not_render_user_badge_or_footer() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        ActivityEntry {
            request_id: "request-complete".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-complete".to_string(),
                text: "completed turn".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "done".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            revision: 0,
        },
        ActivityEntry {
            request_id: "request-queued-followup".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Queued,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-queued-followup".to_string(),
                text: "follow up after the active turn finished".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 2,
            last_seq: 2,
            first_mono_ms: 2,
            last_mono_ms: 2,
            revision: 0,
        },
    ]);
    app.transcript_view.selected_activity_index = 1;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        lines.iter().all(|line| !line.contains(" QUEUED ")),
        "runtime queued status alone should not show the Harness queued badge: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("Assistant · gpt-5.4-mini · queued")),
        "runtime queued status alone should not render a queued assistant footer: {lines:#?}"
    );
}

#[test]
fn streaming_turn_with_own_user_message_does_not_render_queued_badge() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-started-followup".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-started-followup".to_string(),
            text: "follow up now running".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        lines.iter().all(|line| !line.contains(" QUEUED ")),
        "a turn should not mark its own user message as queued once it is the active assistant turn: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Assistant · gpt-5.4-mini · active")),
        "streaming follow-up should keep the active assistant footer: {lines:#?}"
    );
}

#[test]
fn queued_user_followup_keeps_active_footer_on_streaming_turn() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        ActivityEntry {
            request_id: "request-active".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-active".to_string(),
                text: "active turn".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            revision: 0,
        },
        ActivityEntry {
            request_id: "request-queued-followup".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Queued,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-queued-followup".to_string(),
                text: "follow up while the current turn is still running".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 2,
            last_seq: 2,
            first_mono_ms: 2,
            last_mono_ms: 2,
            revision: 0,
        },
    ]);
    app.transcript_view.selected_activity_index = 1;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        lines
            .iter()
            .any(|line| line.contains("Assistant · gpt-5.4-mini · active")),
        "active assistant footer should stay on the in-flight turn: {lines:#?}"
    );
    assert!(
        lines.iter().any(|line| line.contains(" QUEUED ")),
        "queued follow-up should still show the user badge: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("Assistant · gpt-5.4-mini · queued")),
        "queued follow-up should not steal the assistant footer: {lines:#?}"
    );
}

#[test]
fn transcript_wrapping_respects_display_width_for_wide_glyphs() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-wide-wrap".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-wide-wrap".to_string(),
            text: "wrap this diff body".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: "漢字🙂漢字🙂漢字🙂 漢字🙂漢字🙂漢字🙂 漢字🙂漢字🙂漢字🙂".to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 20);
    assert!(
        lines
            .iter()
            .filter(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.contains('漢') || span.content.contains('🙂'))
            })
            .all(|line| line.width() <= 20),
        "transcript rows should honor visible width: {:#?}",
        transcript_test_line_texts(lines)
    );
}

#[test]
fn transcript_selection_snapshot_cache_reuses_repeated_hit_tests() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "req_selection_cache".to_string(),
        profile_label: "default".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(UserMessageSubmittedEvent {
            request_id: "req_selection_cache".to_string(),
            text: "Select this".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: "Cache this selection text across repeated hit tests".to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let area = Rect::new(0, 0, 140, 40);
    let snapshot = transcript_selection_debug_snapshot(&app, area).unwrap_or_abort();
    let row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("selection text"))
        .unwrap_or_abort();
    let column = snapshot.rows[row].find("selection").unwrap_or_abort();

    reset_transcript_selection_cache_metrics_for_test();

    for offset in 0..6 {
        assert!(transcript_selection_cell(
            &app,
            area,
            snapshot.viewport.x + u16::try_from(column + offset).unwrap_or_abort(),
            snapshot.viewport.y + u16::try_from(row).unwrap_or_abort(),
        )
        .is_some());
    }

    assert_eq!(transcript_selection_cache_build_count_for_test(), 1);
}

#[test]
fn startup_lifecycle_text_participates_in_selection_copy() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    let area = Rect::new(0, 0, 100, 24);
    let snapshot = transcript_selection_debug_snapshot(&app, area).unwrap_or_abort();
    let purpose = app.theme().live_shell.startup.new_session_purpose;
    assert!(!snapshot.rows.iter().any(|line| line.contains(purpose)));
    assert!(!snapshot.rows.iter().any(|line| line.contains("Launch:")));
    assert!(!snapshot.rows.iter().any(|line| line.contains("Provider")));
    let row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("███████╗"))
        .unwrap_or_abort();

    let hit = transcript_selection_cell(
        &app,
        area,
        snapshot.viewport.x,
        snapshot.viewport.y + u16::try_from(row).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    assert_eq!(hit.row, row);

    let copied = transcript_selection_text(
        &app,
        area,
        TranscriptSelection {
            anchor: TranscriptSelectionCell { row, column: 0 },
            focus: TranscriptSelectionCell {
                row,
                column: usize::from(snapshot.viewport.width.saturating_sub(1)),
            },
        },
    )
    .unwrap_or_abort();
    assert!(copied.contains("███████╗"));
}

#[test]
fn live_empty_state_text_participates_in_selection_copy() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let area = Rect::new(0, 0, 100, 24);
    let snapshot = transcript_selection_debug_snapshot(&app, area).unwrap_or_abort();
    let value_prop = app.theme().live_shell.empty_state.value_prop;
    let row = snapshot
        .rows
        .iter()
        .position(|line| line.contains(value_prop))
        .unwrap_or_abort();

    assert!(transcript_selection_cell(
        &app,
        area,
        snapshot.viewport.x,
        snapshot.viewport.y + u16::try_from(row).unwrap_or_abort(),
    )
    .is_some());

    let copied = transcript_selection_text(
        &app,
        area,
        TranscriptSelection {
            anchor: TranscriptSelectionCell { row, column: 0 },
            focus: TranscriptSelectionCell {
                row,
                column: usize::from(snapshot.viewport.width.saturating_sub(1)),
            },
        },
    )
    .unwrap_or_abort();
    assert_eq!(copied, value_prop);
}

#[test]
fn live_empty_state_wrapped_examples_participate_in_selection_copy() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let area = Rect::new(0, 0, 80, 24);
    let snapshot = transcript_selection_debug_snapshot(&app, area).unwrap_or_abort();
    let prompts = &app.theme().live_shell.empty_state.example_prompts;
    let first_example_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains(prompts[0].prompt))
        .unwrap_or_abort();
    let wrapped_example_row = first_example_row.saturating_add(1);
    assert!(
        snapshot.rows[wrapped_example_row].contains(prompts[1].prompt)
            || snapshot.rows[wrapped_example_row].contains(prompts[2].prompt)
            || snapshot.rows[wrapped_example_row].contains("review")
            || snapshot.rows[wrapped_example_row].contains("latest"),
        "wrapped examples row should carry visible prompt text: {:?}",
        snapshot.rows[wrapped_example_row]
    );

    let copied = transcript_selection_text(
        &app,
        area,
        TranscriptSelection {
            anchor: TranscriptSelectionCell {
                row: wrapped_example_row,
                column: 0,
            },
            focus: TranscriptSelectionCell {
                row: wrapped_example_row,
                column: usize::from(snapshot.viewport.width.saturating_sub(1)),
            },
        },
    )
    .unwrap_or_abort();
    assert!(
        copied.contains(prompts[1].prompt)
            || copied.contains(prompts[2].prompt)
            || copied.contains("review")
            || copied.contains("latest"),
        "copied: {copied:?}"
    );
}
