use std::time::Instant;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStatus, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::render_test::render_to_string;
use ratatui::layout::Rect;

#[test]
fn s1_s7_transcript_perf_scenarios_print_and_assert() {
    let rows = vec![
        s1_render_key_ignores_non_transcript_events(),
        s2_large_tool_output_reuses_warm_layout(),
        s3_streaming_delta_rebuilds_only_changed_section(),
        s4_many_small_deltas_rebuild_one_section_per_drain(),
        s5_resize_reuses_cached_width_measurements(),
        s6_selection_snapshot_is_cached_for_long_transcript(),
        s7_scroll_while_streaming_preserves_scroll_and_rebuilds_one_section(),
    ];

    assert_eq!(
        rows.len(),
        7,
        "S1-S7 transcript perf scenarios must all run"
    );
    for row in rows {
        println!(
            "{} {} elapsed={}ms details={}",
            row.label, row.name, row.elapsed_ms, row.details
        );
    }
}

fn s1_render_key_ignores_non_transcript_events() -> PerfRow {
    let started = Instant::now();
    let mut app = long_transcript_app(3);
    let before = app.transcript_render_cache_key_for_test();
    app.ingest_event(envelope(
        10_000,
        "agent-s1",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: "agent-s1".to_string(),
            profile: "worker".to_string(),
            parent_agent_id: None,
        }),
    ));

    assert_eq!(
        before,
        app.transcript_render_cache_key_for_test(),
        "S1 non-transcript events must not invalidate the transcript render key"
    );
    PerfRow::new(
        "S1",
        "render key ignores non-transcript events",
        started,
        "key_stable=true",
    )
}

fn s2_large_tool_output_reuses_warm_layout() -> PerfRow {
    let mut app = AppState::new_live(None, false, None);
    ingest_turn(&mut app, 1, "request-s2", "summarize tool output");
    app.ingest_event(envelope(
        10,
        "request-s2",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-s2".to_string(),
            tool_id: "bash".to_string(),
            args_summary: r#"{"command":"cat huge.log"}"#.to_string(),
            args_digest: "args-s2".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        11,
        "tool-s2",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool-s2".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("0123456789abcdef ".repeat(2_000)),
            output_digest: Some("output-s2".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    AppState::reset_transcript_perf_counters_for_test();
    let started = Instant::now();
    render(&app, 140, 42);
    let cold_builds = AppState::transcript_layout_section_build_count_for_test();
    render(&app, 140, 42);
    let warm_builds = AppState::transcript_layout_section_build_count_for_test() - cold_builds;

    assert!(
        (1..=2).contains(&cold_builds),
        "S2 cold full-frame render should measure only the tool section, allowing scrollbar prepass"
    );
    assert_eq!(
        warm_builds, 0,
        "S2 warm render should reuse cached tool layout"
    );
    PerfRow::new(
        "S2",
        "large tool output warm layout reuse",
        started,
        &format!("cold_builds={cold_builds},warm_builds={warm_builds}"),
    )
}

fn s3_streaming_delta_rebuilds_only_changed_section() -> PerfRow {
    let mut app = long_transcript_app(500);
    AppState::reset_transcript_perf_counters_for_test();
    render(&app, 159, 48);
    let warmed = AppState::transcript_layout_section_build_count_for_test();

    let started = Instant::now();
    app.ingest_event(provider_delta(
        20_000,
        "request-perf-499",
        " streaming delta",
    ));
    render(&app, 159, 48);
    let rebuilt = AppState::transcript_layout_section_build_count_for_test() - warmed;

    assert!(
        (500..=1_000).contains(&warmed),
        "S3 warmup should measure each section once per full-frame layout pass"
    );
    assert!(
        (1..=2).contains(&rebuilt),
        "S3 one streaming delta should rebuild only the changed section, allowing scrollbar prepass; rebuilt={rebuilt}"
    );
    PerfRow::new(
        "S3",
        "streaming assistant output",
        started,
        &format!("warmed={warmed},rebuilt={rebuilt}"),
    )
}

fn s4_many_small_deltas_rebuild_one_section_per_drain() -> PerfRow {
    let mut app = long_transcript_app(1);
    AppState::reset_transcript_perf_counters_for_test();
    render(&app, 100, 32);
    let warmed = AppState::transcript_layout_section_build_count_for_test();

    let started = Instant::now();
    for idx in 0..20 {
        app.ingest_event(provider_delta(
            30_000 + idx,
            "request-perf-0",
            if idx % 2 == 0 { " x" } else { " y" },
        ));
        render(&app, 100, 32);
    }
    let rebuilt = AppState::transcript_layout_section_build_count_for_test() - warmed;

    assert!(
        (1..=2).contains(&warmed),
        "S4 warmup should measure one section per full-frame layout pass"
    );
    assert!(
        (20..=40).contains(&rebuilt),
        "S4 each drained delta should rebuild only one section per full-frame layout pass; rebuilt={rebuilt}"
    );
    PerfRow::new(
        "S4",
        "many small provider deltas",
        started,
        &format!("deltas=20,rebuilt={rebuilt}"),
    )
}

fn s5_resize_reuses_cached_width_measurements() -> PerfRow {
    let app = long_transcript_app(500);
    AppState::reset_transcript_perf_counters_for_test();

    let started = Instant::now();
    render(&app, 159, 48);
    let after_wide = AppState::transcript_layout_section_build_count_for_test();
    render(&app, 80, 48);
    let after_narrow = AppState::transcript_layout_section_build_count_for_test();
    render(&app, 159, 48);
    let after_wide_again = AppState::transcript_layout_section_build_count_for_test();

    assert!(
        (500..=1_000).contains(&after_wide),
        "S5 first wide render should measure each section per full-frame layout pass"
    );
    assert!(
        (500..=1_000).contains(&(after_narrow - after_wide)),
        "S5 new narrow width should measure each section per full-frame layout pass"
    );
    assert_eq!(
        after_wide_again, after_narrow,
        "S5 returning to a cached width should not rebuild sections"
    );
    PerfRow::new(
        "S5",
        "wide narrow wide resize",
        started,
        &format!(
            "wide={after_wide},narrow_delta={},wide_again_delta={}",
            after_narrow - after_wide,
            after_wide_again - after_narrow
        ),
    )
}

fn s6_selection_snapshot_is_cached_for_long_transcript() -> PerfRow {
    let mut app = long_transcript_app(500);
    let area = Rect::new(0, 0, 140, 48);
    app.set_frame_area_for_test(area);
    render(&app, area.width, area.height);
    app.set_transcript_selection_for_test(0, 0, 12, 40);
    AppState::reset_transcript_perf_counters_for_test();

    let started = Instant::now();
    render(&app, area.width, area.height);
    render(&app, area.width, area.height);
    let builds = AppState::transcript_selection_cache_build_count_for_test();

    assert_eq!(
        builds, 1,
        "S6 repeated selection renders should reuse one snapshot"
    );
    PerfRow::new(
        "S6",
        "selection over long transcript",
        started,
        &format!("selection_snapshot_builds={builds}"),
    )
}

fn s7_scroll_while_streaming_preserves_scroll_and_rebuilds_one_section() -> PerfRow {
    let mut app = long_transcript_app(500);
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = 120;
    AppState::reset_transcript_perf_counters_for_test();
    render(&app, 159, 48);
    let warmed = AppState::transcript_layout_section_build_count_for_test();

    let started = Instant::now();
    app.ingest_event(provider_delta(
        40_000,
        "request-perf-499",
        " scroll while streaming",
    ));
    render(&app, 159, 48);
    let rebuilt = AppState::transcript_layout_section_build_count_for_test() - warmed;

    assert!(
        (1..=2).contains(&rebuilt),
        "S7 streaming while scrolled should rebuild only the changed section; rebuilt={rebuilt}"
    );
    assert!(
        !app.transcript_view.follow_mode,
        "S7 manual scroll should remain out of follow mode"
    );
    assert_eq!(
        app.transcript_view.transcript_scroll, 120,
        "S7 manual scroll offset should be preserved"
    );
    PerfRow::new(
        "S7",
        "scroll while streaming",
        started,
        &format!(
            "rebuilt={rebuilt},follow_mode={},transcript_scroll={}",
            app.transcript_view.follow_mode, app.transcript_view.transcript_scroll
        ),
    )
}

struct PerfRow {
    label: &'static str,
    name: &'static str,
    elapsed_ms: u128,
    details: String,
}

impl PerfRow {
    fn new(label: &'static str, name: &'static str, started: Instant, details: &str) -> Self {
        let elapsed_ms = started.elapsed().as_millis();
        assert!(
            elapsed_ms <= 10_000,
            "{label} {name} exceeded the integration-test perf guard: {elapsed_ms}ms"
        );
        Self {
            label,
            name,
            elapsed_ms,
            details: details.to_string(),
        }
    }
}

fn long_transcript_app(turns: usize) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    for idx in 0..turns {
        let seq = u64::try_from(idx).expect("fixture index fits u64") * 10 + 1;
        let request_id = format!("request-perf-{idx}");
        ingest_turn(
            &mut app,
            seq,
            &request_id,
            &format!(
                "assistant reply {idx} {}",
                "keeps wrapping stable across the transcript cache ".repeat(2)
            ),
        );
    }
    app
}

fn ingest_turn(app: &mut AppState, seq: u64, request_id: &str, text: &str) {
    app.ingest_event(envelope(seq, request_id, provider_started(request_id)));
    app.ingest_event(envelope(
        seq + 1,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.to_string(),
            delta: text.to_string(),
        }),
    ));
}

fn provider_delta(seq: u64, request_id: &str, delta: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.to_string(),
            delta: delta.to_string(),
        }),
    )
}

fn provider_started(request_id: &str) -> EventV1 {
    EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
        request_id: request_id.to_string(),
        provider_id: "mock".to_string(),
        model_id: "mock-model".to_string(),
        prompt_summary: "prompt".to_string(),
        request_digest: format!("digest-{request_id}"),
        metadata: None,
    })
}

fn render(app: &AppState, width: u16, height: u16) {
    let text = render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        harness_tui::ui::render_app(frame, app);
    });
    std::hint::black_box(text);
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_perf_transcript_{seq:04}"),
        seq,
        run_id: "run_perf_transcript".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-perf".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some(request_id.to_string()),
        payload,
    }
}
