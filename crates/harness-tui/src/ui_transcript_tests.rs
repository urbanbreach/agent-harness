use super::*;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderStreamDeltaEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};

#[test]
fn transcript_test_activity_helper_has_required_defaults() {
    let entry = transcript_section_model_test_activity(
        "request-helper",
        ActivityStatus::Done,
        "assistant reply",
    );

    assert_eq!(entry.request_id, "request-helper");
    assert_eq!(entry.status, ActivityStatus::Done);
    assert_eq!(entry.transcript_text, "assistant reply");
    assert!(entry.tool_calls.is_empty());
    assert_eq!(entry.first_seq, 1);
    assert_eq!(entry.last_seq, 1);
}

#[test]
fn transcript_test_tool_call_helper_has_queued_defaults() {
    let tool_call = transcript_section_model_test_tool_call("tool-helper", "fs.read");

    assert_eq!(tool_call.tool_call_id, "tool-helper");
    assert_eq!(tool_call.tool_id, "fs.read");
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);
    assert!(tool_call.output_summary.is_none());
    assert!(tool_call.artifact_refs.is_empty());
}

#[test]
fn transcript_test_line_texts_joins_spans() {
    let texts = transcript_test_line_texts(vec![
        Line::from(vec![Span::raw("hello"), Span::raw(" world")]),
        Line::from(vec![Span::raw("again")]),
    ]);

    assert_eq!(texts, vec!["hello world", "again"]);
}

#[test]
fn transcript_section_model_preserves_activity_order() {
    exact_test_transcript_section_model_preserves_activity_order();
}

#[test]
fn transcript_section_model_keeps_nested_tool_and_error_blocks() {
    exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks();
}

#[cfg(test)]
#[test]
fn transcript_reasoning_precedes_answer_and_tool_rows() {
    exact_test_transcript_reasoning_precedes_answer_and_tool_rows();
}

#[test]
fn transcript_follow_mode_uses_measured_surface_heights() {
    exact_test_transcript_follow_mode_uses_measured_surface_heights();
}

#[test]
fn failed_tool_cards_parse_legacy_error_copy() {
    super::failed_tool_cards_parse_legacy_error_copy();
}

#[test]
fn failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators() {
    super::failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators();
}

#[test]
fn denied_tool_cards_use_denied_subtitle() {
    super::denied_tool_cards_use_denied_subtitle();
}

#[test]
fn denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon() {
    super::denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon();
}

#[test]
fn generic_failed_tool_messages_do_not_split_arbitrary_prefixes() {
    super::generic_failed_tool_messages_do_not_split_arbitrary_prefixes();
}

#[test]
fn failed_tool_cards_fallback_when_error_details_are_missing() {
    super::failed_tool_cards_fallback_when_error_details_are_missing();
}

#[test]
fn transcript_pending_permission_stays_after_last_activity() {
    exact_test_transcript_pending_permission_stays_after_last_activity();
}

#[test]
fn transcript_layout_cache_reuses_measurement_when_animation_frame_changes() {
    let mut app = AppState::default();
    let mut activity = ActivityEntry {
        request_id: "request-streaming-cache".to_string(),
        revision: 1,
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
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
    };
    activity.thinking_text = "checking cache invalidation".to_string();
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;

    let initial_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("⠋ Thinking: checking cache invalidation")));

    let initial_key = app.transcript_render_cache_key();
    reset_transcript_layout_measurement_counter_for_test();
    app.advance_transcript_animation_phase();
    let updated_key = app.transcript_render_cache_key();
    assert_eq!(
        initial_key, updated_key,
        "animation-only changes must not change the transcript render key"
    );

    let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(updated_lines
        .iter()
        .any(|line| line.contains("⠙ Thinking: checking cache invalidation")));
    assert_eq!(
        transcript_layout_section_build_count_for_test(),
        0,
        "animation-only changes must repaint decoration without remeasuring transcript sections"
    );
}

#[test]
fn transcript_queued_turn_renders_footer_spinner_before_completion() {
    let mut app = AppState::default();
    let mut activity =
        transcript_section_model_test_activity("request-queued-footer", ActivityStatus::Queued, "");
    activity.provider_id = "default".to_string();
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let footer_line = lines
        .iter()
        .find(|line| line.contains("Default") && line.contains("queued"))
        .expect("queued footer line");

    assert!(
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            .iter()
            .any(|frame| footer_line.contains(frame)),
        "queued footer should show a spinner frame\n{footer_line}"
    );
    assert!(footer_line.contains("Default"));
    assert!(footer_line.contains("queued"));
}

#[test]
fn transcript_layout_cache_invalidates_when_theme_changes() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-theme-cache".to_string(),
        revision: 1,
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-theme-cache".to_string(),
            text: "theme-sensitive prompt".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: "reply".to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.selected_activity_index = 0;

    let initial_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let initial_surface = initial_layout.sections[0].surfaces[0].surface;

    let mut alternate_theme = *app.theme();
    alternate_theme.surface.panel = Color::Rgb(0x22, 0x33, 0x44);
    app.set_theme_for_test(alternate_theme);

    let updated_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let updated_surface = updated_layout.sections[0].surfaces[0].surface;

    assert_ne!(initial_surface, updated_surface);
    assert_eq!(updated_surface, alternate_theme.surface.panel);
}

#[test]
fn wrap_measurement_property_holds() {
    let theme = Theme::default();
    let mut app = transcript_cache_fixture_app(4);
    app.activities[1].transcript_text =
        "supercalifragilisticexpialidocious-supercalifragilisticexpialidocious ".repeat(3);
    app.activities[2].transcript_text =
        "plain words wrap before the edge and keep measured rows stable ".repeat(6);

    for width in [28, 80, 159] {
        let layout = uncached_transcript_layout_for_width(&app, &theme, width);
        for (section_index, section) in layout.sections.iter().enumerate() {
            for (surface_index, surface) in section.surfaces.iter().enumerate() {
                assert_eq!(
                    surface.height,
                    surface.lines.len(),
                    "prewrapped surface rows should match measured rows at width {width}, section {section_index}, surface {surface_index}"
                );
                let content_width = usize::from(transcript_surface_content_width(
                    surface.width,
                    surface.show_outer_rail,
                ));
                for line in &surface.lines {
                    assert!(
                        line.width() <= content_width,
                        "wrapped line width {} should fit content width {content_width} at transcript width {width}",
                        line.width()
                    );
                }
            }
        }
    }
}

#[test]
fn transcript_cache_on_off_equivalence() {
    let app = transcript_cache_fixture_app(5);
    let theme = Theme::default();

    for width in [28, 80, 159] {
        let cached = build_measured_transcript_layout_for_width(&app, &theme, width);
        let uncached = uncached_transcript_layout_for_width(&app, &theme, width);

        assert_eq!(cached.total_height, uncached.total_height);
        assert_eq!(
            layout_texts(&cached, &theme),
            layout_texts(&uncached, &theme)
        );
    }
}

#[test]
fn transcript_cache_invalidation_rebuilds_just_changed_section() {
    let theme = Theme::default();
    let mut app = transcript_cache_fixture_app(4);
    reset_transcript_layout_measurement_metrics_for_test();

    let _ = build_measured_transcript_layout_for_width(&app, &theme, 80);
    let warmed_builds = transcript_layout_section_build_count_for_test();
    assert_eq!(warmed_builds, 4);

    ingest_transcript_delta(
        &mut app,
        100,
        "request-transcript-cache-3",
        " changed measured component",
    );
    let _ = build_measured_transcript_layout_for_width(&app, &theme, 80);
    let rebuilt_sections = transcript_layout_section_build_count_for_test() - warmed_builds;

    assert_eq!(
        rebuilt_sections, 1,
        "changing one measured section should rebuild only that section"
    );
}

#[test]
fn perf_long_session_perf_s1_s7_budget_table() {
    let theme = Theme::default();
    let mut rows = Vec::new();

    let s1_app = transcript_cache_fixture_app(500);
    reset_transcript_layout_measurement_metrics_for_test();
    let started = std::time::Instant::now();
    let s1_layout = build_measured_transcript_layout_for_width(&s1_app, &theme, 159);
    let s1_ms = started.elapsed().as_millis();
    std::hint::black_box(s1_layout.total_height);
    let s1_builds = transcript_layout_section_build_count_for_test();
    let warmed_builds = s1_builds;
    let started = std::time::Instant::now();
    let s1_second = build_measured_transcript_layout_for_width(&s1_app, &theme, 159);
    let s1_second_ms = started.elapsed().as_millis();
    std::hint::black_box(s1_second.total_height);
    let s1_second_rebuilds = transcript_layout_section_build_count_for_test() - warmed_builds;
    assert_eq!(s1_second_rebuilds, 0);
    rows.push(transcript_perf_row(
        "S1",
        "500-message conversation, cold render",
        s1_ms,
        5_000,
        serde_json::json!({
            "initial_section_rebuilds": s1_builds,
            "second_render_ms": s1_second_ms,
            "second_render_section_rebuilds": s1_second_rebuilds,
        }),
    ));

    let s2_app = large_tool_output_perf_app();
    reset_transcript_layout_measurement_metrics_for_test();
    let started = std::time::Instant::now();
    let s2_layout = build_measured_transcript_layout_for_width(&s2_app, &theme, 120);
    let s2_ms = started.elapsed().as_millis();
    std::hint::black_box(s2_layout.total_height);
    rows.push(transcript_perf_row(
        "S2",
        "large tool output with artifacts",
        s2_ms,
        5_000,
        serde_json::json!({
            "section_rebuilds": transcript_layout_section_build_count_for_test(),
        }),
    ));

    let mut s3_app = transcript_cache_fixture_app(500);
    reset_transcript_layout_measurement_metrics_for_test();
    let _ = build_measured_transcript_layout_for_width(&s3_app, &theme, 159);
    let before_s3_stream = transcript_layout_section_build_count_for_test();
    let started = std::time::Instant::now();
    for idx in 0..20 {
        ingest_transcript_delta(
            &mut s3_app,
            1_000 + idx,
            "request-transcript-cache-499",
            " streaming delta",
        );
        let layout = build_measured_transcript_layout_for_width(&s3_app, &theme, 159);
        std::hint::black_box(layout.total_height);
    }
    rows.push(transcript_perf_row(
        "S3",
        "streaming assistant output",
        started.elapsed().as_millis(),
        5_000,
        serde_json::json!({
            "streaming_deltas": 20,
            "section_rebuilds": transcript_layout_section_build_count_for_test() - before_s3_stream,
        }),
    ));

    let mut s4_app = transcript_cache_fixture_app(1);
    reset_transcript_layout_measurement_metrics_for_test();
    let started = std::time::Instant::now();
    for batch in 0..125 {
        for delta in 0..16 {
            ingest_transcript_delta(
                &mut s4_app,
                2_000 + (batch * 16) + delta,
                "request-transcript-cache-0",
                " x",
            );
        }
        let layout = build_measured_transcript_layout_for_width(&s4_app, &theme, 80);
        std::hint::black_box(layout.total_height);
    }
    rows.push(transcript_perf_row(
        "S4",
        "many small provider deltas",
        started.elapsed().as_millis(),
        5_000,
        serde_json::json!({
            "deltas": 2_000,
            "drain_batches": 125,
            "section_rebuilds": transcript_layout_section_build_count_for_test(),
        }),
    ));

    let s5_app = transcript_cache_fixture_app(500);
    reset_transcript_layout_measurement_metrics_for_test();
    let started = std::time::Instant::now();
    for width in [159, 80, 159] {
        let layout = build_measured_transcript_layout_for_width(&s5_app, &theme, width);
        std::hint::black_box(layout.total_height);
    }
    rows.push(transcript_perf_row(
        "S5",
        "wide/narrow resize",
        started.elapsed().as_millis(),
        5_000,
        serde_json::json!({
            "widths": [159, 80, 159],
            "section_rebuilds": transcript_layout_section_build_count_for_test(),
        }),
    ));

    let s6_app = transcript_cache_fixture_app(500);
    let started = std::time::Instant::now();
    let snapshot = transcript_selection_debug_snapshot(&s6_app, Rect::new(0, 0, 140, 40))
        .expect("long transcript selection snapshot");
    std::hint::black_box(snapshot.rows.len());
    rows.push(transcript_perf_row(
        "S6",
        "selection over long transcript",
        started.elapsed().as_millis(),
        5_000,
        serde_json::json!({
            "visible_rows": snapshot.rows.len(),
        }),
    ));

    let mut s7_app = transcript_cache_fixture_app(500);
    s7_app.transcript_view.follow_mode = false;
    s7_app.transcript_view.transcript_scroll = 120;
    reset_transcript_layout_measurement_metrics_for_test();
    let _ = build_measured_transcript_layout_for_width(&s7_app, &theme, 159);
    let before_s7 = transcript_layout_section_build_count_for_test();
    let started = std::time::Instant::now();
    ingest_transcript_delta(
        &mut s7_app,
        10_000,
        "request-transcript-cache-499",
        " scroll while streaming",
    );
    let s7_layout = build_measured_transcript_layout_for_width(&s7_app, &theme, 159);
    std::hint::black_box(s7_layout.total_height);
    rows.push(transcript_perf_row(
        "S7",
        "scroll while streaming",
        started.elapsed().as_millis(),
        5_000,
        serde_json::json!({
            "section_rebuilds": transcript_layout_section_build_count_for_test() - before_s7,
            "follow_mode": s7_app.transcript_view.follow_mode,
            "transcript_scroll": s7_app.transcript_view.transcript_scroll,
        }),
    ));

    assert_eq!(rows.len(), 7);
    write_transcript_perf_budget_artifact(&rows);
}

#[test]
fn pending_permission_sections_render_warning_turn_container() {
    let mut app = AppState::default();
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_pending_permission_group".to_string(),
        seq: 1,
        run_id: "run_pending_permission_group".to_string(),
        mono_ms: 0,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Supervisor,
            None,
        ),
        correlation_id: Some("tool_call_pending_permission_group".to_string()),
        causation_id: None,
        stream_key: Some("tool_call_pending_permission_group".to_string()),
        payload: harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: "perm_pending_permission_group".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tool_call_pending_permission_group".to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    });

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert_eq!(lines[0], "Waiting for first turn…");
    assert!(lines
        .iter()
        .all(|line| !line.contains("permission checkpoint")
            && !line.contains("Apply hashline edit to demo.txt")));
}

fn transcript_cache_fixture_app(turns: usize) -> AppState {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(
        (0..turns)
            .map(|idx| {
                let request_id = format!("request-transcript-cache-{idx}");
                let mut activity = transcript_section_model_test_activity(
                    &request_id,
                    ActivityStatus::Done,
                    &format!(
                        "assistant reply {idx} {}",
                        "keeps wrapping measurement stable ".repeat(3)
                    ),
                );
                activity.user_message = Some(UserMessageSubmittedEvent {
                    request_id,
                    text: format!("user prompt {idx}"),
                });
                activity.first_seq = u64::try_from(idx).expect("fixture index fits") * 2 + 1;
                activity.last_seq = activity.first_seq + 1;
                activity
            })
            .collect::<Vec<_>>(),
    );
    app.selected_activity_index = turns.saturating_sub(1);
    app
}

fn uncached_transcript_layout_for_width(
    app: &AppState,
    theme: &Theme,
    width: u16,
) -> MeasuredTranscriptLayout {
    let sections = build_transcript_sections(app);
    measure_transcript_layout_without_cache(
        &sections,
        theme,
        width,
        theme.surface.shell,
        build_transcript_render_surfaces,
    )
}

fn layout_texts(layout: &MeasuredTranscriptLayout, theme: &Theme) -> Vec<String> {
    transcript_test_line_texts(transcript_layout_lines(layout, theme, 0))
}

fn ingest_transcript_delta(app: &mut AppState, seq: u64, request_id: &str, delta: &str) {
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_transcript_cache_{seq:04}"),
        seq,
        run_id: "run_transcript_cache".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(
            ActorKind::Worker,
            Some("agent_transcript_cache".to_string()),
        ),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some(request_id.to_string()),
        payload: EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.to_string(),
            delta: delta.to_string(),
        }),
    });
}

fn transcript_perf_row(
    scenario: &str,
    name: &str,
    elapsed_ms: u128,
    budget_ms: u128,
    details: serde_json::Value,
) -> serde_json::Value {
    assert!(
        elapsed_ms <= budget_ms,
        "{scenario} transcript perf budget exceeded: {elapsed_ms}ms > {budget_ms}ms"
    );

    serde_json::json!({
        "scenario": scenario,
        "name": name,
        "elapsed_ms": u64::try_from(elapsed_ms).expect("elapsed milliseconds fit u64"),
        "budget_ms": u64::try_from(budget_ms).expect("budget milliseconds fit u64"),
        "details": details,
    })
}

fn large_tool_output_perf_app() -> AppState {
    let mut app = AppState::default();
    let mut activity = transcript_section_model_test_activity(
        "request-transcript-cache-large-tool",
        ActivityStatus::Done,
        "Summarized the generated tool artifacts.",
    );
    activity.user_message = Some(UserMessageSubmittedEvent {
        request_id: "request-transcript-cache-large-tool".to_string(),
        text: "produce a large tool output and keep artifacts visible".to_string(),
    });

    for idx in 0..5 {
        let mut tool_call = transcript_section_model_test_tool_call(
            &format!("tool-call-large-output-{idx}"),
            "bash",
        );
        tool_call.status = ToolCallDisplayStatus::Succeeded;
        tool_call.args_summary = format!(
            r#"{{"command":"printf large-output-{idx}","description":"large output {idx}"}}"#
        );
        tool_call.output_summary = Some(format!(
            "large output {idx}: {}",
            "0123456789abcdef ".repeat(3_000)
        ));
        tool_call.output_digest = Some(format!("digest-large-output-{idx}"));
        tool_call.artifact_refs = vec![crate::app::ToolArtifactEntry {
            path: format!("artifacts/tool-large-output-{idx}.txt"),
            digest: Some(format!("artifact-digest-large-output-{idx}")),
        }];
        tool_call.first_seq = u64::try_from(idx).expect("fixture index fits") + 10;
        tool_call.last_seq = tool_call.first_seq + 1;
        activity.tool_calls.push(tool_call);
    }

    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;
    app
}

fn write_transcript_perf_budget_artifact(rows: &[serde_json::Value]) {
    let Ok(artifact_dir) = std::env::var("HARNESS_PERF_ARTIFACT_DIR") else {
        return;
    };
    let artifact_root = std::path::PathBuf::from(artifact_dir);
    std::fs::create_dir_all(&artifact_root).expect("create transcript perf artifact directory");
    let timestamp_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_millis();
    let artifact = serde_json::json!({
        "schema_version": "harness-tui-transcript-long-session-perf-v1",
        "timestamp_unix_ms": u64::try_from(timestamp_unix_ms).expect("timestamp milliseconds fit u64"),
        "rows": rows,
        "provenance": {
            "command_hint": "scripts/test-lanes.sh perf",
            "artifact_root_env": artifact_root,
        },
    });
    let artifact_path = artifact_root.join("transcript-long-session-budget-table.json");
    let encoded = serde_json::to_string_pretty(&artifact).expect("encode transcript perf artifact");
    std::fs::write(artifact_path, encoded).expect("write transcript perf artifact");
}

#[test]
fn streaming_reasoning_keeps_active_thinking_label() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-streaming-reasoning-label",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "still working".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(lines
        .iter()
        .any(|line| line.contains("Thinking: still working")));
    assert!(lines
        .iter()
        .all(|line| !line.contains("Thought still working")));
}

#[test]
fn user_message_surface_restores_assistant_footer_when_timestamps_are_visible() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-user-padding".to_string(),
        revision: 1,
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-user-padding".to_string(),
            text: "hello".to_string(),
        }),
        user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
        request_data: None,
        thinking_text: String::new(),
        transcript_text: "reply".to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "show timestamps".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(!lines.iter().any(|line| line.contains("› You")));
    assert!(lines
        .iter()
        .all(|line| !(line.starts_with('┃') && line.contains("09:45"))));
    assert!(lines.iter().all(|line| !line.contains("Assistant ·")));
    assert!(lines.iter().any(|line| line.contains("hello")));
    let reply_row = lines
        .iter()
        .position(|line| line.contains("reply"))
        .expect("assistant reply row");
    let footer_row = lines
        .iter()
        .position(|line| {
            line.contains("▣ · 0ms")
                && line.contains("Default · gpt-5.4-mini")
                && line.contains("openai")
        })
        .unwrap_or_else(|| panic!("assistant footer row should render below messages: {lines:#?}"));
    assert!(
        footer_row > reply_row,
        "assistant footer should render below the assistant message body: {lines:#?}"
    );
}

#[test]
fn reasoning_summary_renders_as_nested_inset_block() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-inset",
        ActivityStatus::Done,
        "answer",
    );
    entry.thinking_text = "Matching harness response spacing".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(lines
        .iter()
        .any(|line| line.contains("Matching harness response spacing")));
    assert!(lines
        .iter()
        .any(|line| line.contains("Thought: Matching harness response spacing · 0ms")));
    assert!(lines.iter().all(|line| !line.contains("Thinking:")));
    assert!(lines.iter().any(|line| line.is_empty()));
    assert!(lines.iter().any(|line| line.contains("answer")));
}

#[test]
fn fenced_code_blocks_render_frameless_with_highlighting() {
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-code-panel",
            ActivityStatus::Done,
            "Before\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nAfter",
        )]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        100,
    ));

    let function_row = lines
        .iter()
        .find(|line| line.contains("fn main()"))
        .expect("function row");
    let println_row = lines
        .iter()
        .find(|line| line.contains("println!(\"hi\")"))
        .expect("println row");

    assert!(lines.iter().any(|line| line.contains("Before")));
    assert!(
        !function_row.contains('┃') && !println_row.contains('┃'),
        "fenced code should keep syntax-highlighted content in flow without a nested frame\n{lines:#?}"
    );
    assert!(lines.iter().any(|line| line.contains("After")));
}

#[test]
fn transcript_turn_sections_keep_exactly_one_blank_row_between_sections() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity("request-a", ActivityStatus::Done, "first"),
        transcript_section_model_test_activity("request-b", ActivityStatus::Done, "second"),
    ]);
    app.selected_activity_index = 1;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);

    assert_eq!(layout.sections.len(), 2);
    assert_eq!(layout.sections[0].leading_gap_height, 0);
    assert_eq!(layout.sections[1].leading_gap_height, 1);
}

#[test]
fn assistant_tool_surfaces_keep_same_trailing_gap_as_text_boxes() {
    let mut activity = transcript_section_model_test_activity(
        "request-shell-alignment",
        ActivityStatus::Done,
        "I’ll run a harmless shell command.",
    );
    activity.user_message = Some(UserMessageSubmittedEvent {
        request_id: "request-shell-alignment".to_string(),
        text: "test out some tools".to_string(),
    });

    let mut shell_call = transcript_section_model_test_tool_call("tc-shell-alignment", "bash");
    shell_call.args_summary = r#"{"command":"printf 'bash smoke test ok\n'","description":"Run harmless shell smoke test"}"#.to_string();
    shell_call.status = ToolCallDisplayStatus::Succeeded;
    shell_call.output_summary = Some("bash smoke test ok".to_string());
    activity.tool_calls.push(shell_call);

    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);
    let surfaces = &layout.sections[0].surfaces;
    let user_surface = surfaces
        .iter()
        .find(|surface| surface.show_outer_rail)
        .expect("user surface");
    let tool_surface = surfaces
        .iter()
        .find(|surface| {
            transcript_test_line_texts(surface.lines.clone())
                .iter()
                .any(|line| line.contains("bash smoke test ok"))
        })
        .expect("assistant tool surface");
    let tool_lines = transcript_test_line_texts(tool_surface.lines.clone());
    let tool_interactions = tool_surface
        .interaction_rows
        .as_ref()
        .expect("tool surface interaction rows");

    assert_eq!(user_surface.width, 78);
    assert_eq!(tool_surface.width, user_surface.width);
    let title_row = tool_lines
        .iter()
        .position(|line| line.contains("# Run harmless shell smoke test"))
        .expect("title line");
    let command_row = tool_lines
        .iter()
        .position(|line| line.contains("$ printf 'bash smoke test ok"))
        .expect("command line");
    let output_row = tool_lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            (!line.contains("$ printf") && line.contains("bash smoke test ok")).then_some(index)
        })
        .expect("output line");
    let title_column = tool_lines[title_row]
        .find("# Run harmless shell smoke test")
        .expect("title column");
    let command_column = tool_lines[command_row]
        .find("$ printf 'bash smoke test ok")
        .expect("command column");
    let output_column = tool_lines[output_row]
        .find("bash smoke test ok")
        .expect("output column");
    assert_eq!(title_column, command_column);
    assert_eq!(output_column, command_column);
    assert_eq!(tool_interactions[title_row], None);
    assert_eq!(tool_interactions[command_row], None);
    assert_eq!(tool_interactions[output_row], None);
    assert!(
        tool_lines.iter().any(|line| line.starts_with(&format!(
            "{TRANSCRIPT_COMMAND_TOOL_INDENT}{HARNESS_SPLIT_RAIL_GLYPH}"
        )) && line.contains("bash smoke test ok")),
        "command card rail should align with transcript text box edge\n{tool_lines:#?}"
    );
    assert!(
        tool_surface
            .lines
            .iter()
            .all(|line| line.width() <= usize::from(tool_surface.width)),
        "tool card lines should be built for the same visual width as the rendered surface"
    );

    let area = Rect::new(0, 0, 100, 30);
    let snapshot = transcript_selection_debug_snapshot(&app, area)
        .expect("shell command card selection snapshot");
    let title_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("# Run harmless shell smoke test"))
        .expect("selectable title row");
    let output_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("bash smoke test ok"))
        .expect("selectable output row");
    let copied = transcript_selection_text(
        &app,
        area,
        TranscriptSelection {
            anchor: TranscriptSelectionCell {
                row: title_row,
                column: 0,
            },
            focus: TranscriptSelectionCell {
                row: output_row,
                column: usize::from(snapshot.viewport.width.saturating_sub(1)),
            },
        },
    )
    .expect("shell command card selection copies text");
    assert!(
        copied.starts_with("# Run harmless shell smoke test"),
        "copied shell card text should skip visual rail/padding: {copied:?}"
    );
    assert!(copied.contains("$ printf 'bash smoke test ok"));
    assert!(copied.contains("bash smoke test ok"));
    assert!(!copied.contains(HARNESS_SPLIT_RAIL_GLYPH));
}

#[test]
fn block_tool_cards_align_rail_with_message_box_edge() {
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tc-card-alignment".to_string(),
        child_session_id: None,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "todo.write".to_string(),
            title: "1 of 1 todos completed".to_string(),
            subtitle: None,
            path_metadata: None,
            icon: Some("☑"),
            status: ToolCallDisplayStatus::Succeeded,
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: vec![
            TranscriptToolCallDetailBlock::TodoList {
                items: vec![TranscriptTodoItem {
                    content: "Align tool boxes with messages".to_string(),
                    status: ui_tool_question_todo::TranscriptTodoStatus::Pending,
                }],
            },
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content: "--- a/demo.txt\n+++ b/demo.txt\n@@ -1 +1 @@\n-old\n+new\n"
                    .to_string(),
                fallback_path: Some("demo.txt".to_string()),
                force_stacked: false,
                show_file_header: true,
            },
        ],
        expanded: true,
    };

    let lines = transcript_test_line_texts(
        append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel).lines,
    );

    for marker in [
        "1 of 1 todos completed",
        "[ ] Align tool boxes with messages",
        "+ new",
    ] {
        let row = lines
            .iter()
            .find(|line| line.contains(marker))
            .unwrap_or_else(|| panic!("missing marker {marker:?}\n{lines:#?}"));
        assert!(
            row.starts_with(TRANSCRIPT_RAIL_GLYPH),
            "tool card row should align its rail with the message box edge: {row:?}"
        );
    }
}

#[test]
fn assistant_tool_surface_spacing_matches_shell_rhythm() {
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantBody),
            TranscriptRenderSurfaceKind::AssistantTool,
        ),
        0,
        "assistant text should hand off to tool rows without an extra blank terminal row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantTool),
            TranscriptRenderSurfaceKind::AssistantBody,
        ),
        1,
        "tool rows should still leave a single section break before the next assistant text block"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantReasoning),
            TranscriptRenderSurfaceKind::AssistantBody,
        ),
        0,
        "reasoning-to-body spacing is carried by the nested blank row inside the body surface"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantTool),
            TranscriptRenderSurfaceKind::AssistantReasoning,
        ),
        0,
        "tool-to-reasoning spacing is carried by the reasoning block itself so the rendered gap stays single-row"
    );
}

#[test]
fn assistant_body_to_footer_gap_is_one_row() {
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantBody),
            TranscriptRenderSurfaceKind::AssistantFooter,
        ),
        1,
        "assistant body text should be separated from the footer by exactly one blank terminal row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantReasoning),
            TranscriptRenderSurfaceKind::AssistantFooter,
        ),
        1,
        "assistant reasoning block should be separated from the footer by exactly one blank terminal row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantTool),
            TranscriptRenderSurfaceKind::AssistantFooter,
        ),
        1,
        "assistant tool surface should be separated from the footer by exactly one blank terminal row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantError),
            TranscriptRenderSurfaceKind::AssistantFooter,
        ),
        1,
        "assistant error surface should be separated from the footer by exactly one blank terminal row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantCommandTool),
            TranscriptRenderSurfaceKind::AssistantFooter,
        ),
        1,
        "assistant command tool surface should be separated from the footer by exactly one blank terminal row"
    );
}

#[test]
fn reasoning_to_answer_transition_uses_single_blank_row() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-gap",
        ActivityStatus::Done,
        "answer",
    );
    entry.thinking_text = "reasoning".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let reasoning_row = lines
        .iter()
        .position(|line| line.contains("reasoning"))
        .expect("reasoning row");
    let answer_row = lines
        .iter()
        .position(|line| line.contains("answer"))
        .expect("answer row");

    assert_eq!(
        answer_row,
        reasoning_row + 2,
        "reasoning and answer should be separated by exactly one blank terminal row\n{lines:#?}"
    );
    assert!(lines[reasoning_row + 1].is_empty());
}

#[test]
fn streaming_reasoning_spinner_uses_deterministic_braille_frames() {
    let mut app = AppState::default();
    let mut activity = ActivityEntry {
        request_id: "request-streaming-spinner".to_string(),
        revision: 1,
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
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
    };
    activity.thinking_text = "streaming thought".to_string();
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;

    let first = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    app.advance_transcript_animation_phase();
    let second = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        first
            .iter()
            .any(|line| line.contains("⠋ Thinking: streaming thought")),
        "first frame should use the first reasoning spinner frame: {first:#?}"
    );
    assert!(
        second
            .iter()
            .any(|line| line.contains("⠙ Thinking: streaming thought")),
        "second frame should use the second reasoning spinner frame: {second:#?}"
    );
}

#[path = "ui_transcript_lifecycle_tests.rs"]
mod lifecycle_tests;
