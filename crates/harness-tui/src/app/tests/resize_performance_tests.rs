use std::collections::VecDeque;
use std::hint::black_box;
use std::time::Instant;

use ratatui::backend::TestBackend;

use super::*;

const TRANSCRIPT_BLOCK_COUNT: usize = 10_000;
const WARMUP_RESIZE_COUNT: usize = 10;
const MEASURED_RESIZE_COUNT: usize = 100;
const NARROW_COLUMNS: u16 = 80;
const WIDE_COLUMNS: u16 = 160;
const VIEWPORT_ROWS: u16 = 40;
const P95_LIMIT_US: u128 = 33_000;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[cfg(debug_assertions)]
fn require_release_profile() -> TestResult {
    Err("P1-04 must run with --release".into())
}

#[cfg(not(debug_assertions))]
fn require_release_profile() -> TestResult {
    Ok(())
}

fn settled_assistant_block(index: usize) -> ActivityEntry {
    let sequence = u64::try_from(index).unwrap_or_abort();
    ActivityEntry {
        request_id: format!("resize-perf-{index}"),
        profile_label: "performance".to_string(),
        model_id: "resize-model".to_string(),
        provider_id: "benchmark".to_string(),
        status: ActivityStatus::Done,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: format!(
            "block {index:05}: 界e\u{301}🙂 stable detached resize anchor content spanning columns"
        ),
        first_delta_mono_ms: Some(sequence),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: sequence,
        last_seq: sequence,
        first_mono_ms: sequence,
        last_mono_ms: sequence,
        request_started_mono_ms: None,
        revision: 0,
    }
}

pub(super) fn perf_resize_to_render_p95_stays_within_one_frame_and_preserves_detached_anchor(
) -> TestResult {
    require_release_profile()?;
    assert_eq!(std::env::consts::OS, "linux", "P1-04 requires Linux");
    assert_eq!(std::env::consts::ARCH, "x86_64", "P1-04 requires x86_64");

    // Given: exactly 10,000 settled assistant blocks and a detached logical/display-column anchor.
    let mut app = AppState::new_live(None, false, None);
    app.activities = (0..TRANSCRIPT_BLOCK_COUNT)
        .map(settled_assistant_block)
        .collect::<VecDeque<_>>();
    let initial_area = Rect::new(0, 0, NARROW_COLUMNS, VIEWPORT_ROWS);
    app.set_frame_area(initial_area);
    let backend = TestBackend::new(initial_area.width, initial_area.height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| render_app(frame, &app))?;
    let max_scroll = app.transcript_view.last_transcript_max_scroll.get();
    assert!(max_scroll > 0, "10,000 blocks must overflow the viewport");
    app.set_transcript_scroll_for_test(max_scroll / 2);
    terminal.draw(|frame| render_app(frame, &app))?;
    let anchor_before = app
        .transcript_view
        .measured_anchor
        .get()
        .ok_or("detached transcript must expose a logical/display-column anchor")?;

    for iteration in 0..WARMUP_RESIZE_COUNT {
        let columns = if iteration % 2 == 0 {
            WIDE_COLUMNS
        } else {
            NARROW_COLUMNS
        };
        let area = Rect::new(0, 0, columns, VIEWPORT_ROWS);
        app.set_frame_area(area);
        terminal.resize(area)?;
        terminal.draw(|frame| render_app(frame, &app))?;
        assert_eq!(
            app.transcript_view.measured_anchor.get(),
            Some(anchor_before)
        );
    }

    // When: 100 hot resize-to-render operations alternate between 80 and 160 columns.
    let mut samples_us = Vec::with_capacity(MEASURED_RESIZE_COUNT);
    for iteration in 0..MEASURED_RESIZE_COUNT {
        let columns = if iteration % 2 == 0 {
            WIDE_COLUMNS
        } else {
            NARROW_COLUMNS
        };
        let area = Rect::new(0, 0, columns, VIEWPORT_ROWS);
        let started = Instant::now();
        app.set_frame_area(area);
        terminal.resize(area)?;
        terminal.draw(|frame| render_app(frame, &app))?;
        black_box(terminal.backend().buffer());
        samples_us.push(started.elapsed().as_micros());
        assert_eq!(
            app.transcript_view.measured_anchor.get(),
            Some(anchor_before)
        );
    }

    // Then: emit the complete machine-readable sample set and enforce one 33 ms frame at p95.
    let mut sorted_us = samples_us.clone();
    sorted_us.sort_unstable();
    let p95_index = (MEASURED_RESIZE_COUNT * 95).div_ceil(100) - 1;
    let p95_us = sorted_us[p95_index];
    println!(
        "{}",
        serde_json::json!({
            "benchmark": "p1_04_resize_to_render",
            "platform": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH },
            "profile": "release",
            "transcript_blocks": TRANSCRIPT_BLOCK_COUNT,
            "warmup_resizes": WARMUP_RESIZE_COUNT,
            "measured_resizes": MEASURED_RESIZE_COUNT,
            "columns": [NARROW_COLUMNS, WIDE_COLUMNS],
            "samples_us": samples_us,
            "p95_us": p95_us,
            "threshold_us": P95_LIMIT_US,
            "anchor_preserved": true
        })
    );
    assert!(
        p95_us <= P95_LIMIT_US,
        "resize-to-render p95 {p95_us} us exceeded {P95_LIMIT_US} us"
    );
    Ok(())
}
