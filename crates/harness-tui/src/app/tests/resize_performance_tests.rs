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
const P95_LIMIT_US: u128 = 8_333;

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

// Run each scenario in a fresh nextest process to isolate allocator/cache residency.
#[test]
fn perf_interactive_resources_under_load() -> TestResult {
    use crate::terminal::{FrameOutput, FrameOutputBackend, FrameSubmission};
    use harness_core::event::{LiveEventEnvelope, LiveEventV1, RuntimeEvent};

    require_release_profile()?;
    let scenario = std::env::var("HARNESS_PERF_SCENARIO").unwrap_or_else(|_| "scroll".into());
    let count: usize = std::env::var("HARNESS_PERF_HISTORY")
        .unwrap_or_else(|_| "10000".into())
        .parse()?;
    let frames: usize = std::env::var("HARNESS_PERF_FRAMES")
        .unwrap_or_else(|_| "120".into())
        .parse()?;
    assert!(frames > 0);
    let mut app = if scenario == "startup" {
        AppState::new_startup(Vec::new(), None)
    } else {
        AppState::new_live(None, false, None)
    };
    if scenario != "startup" {
        // Stress retained history without the normal 200 kB eviction changing its size.
        app.memory_caps.max_transcript_chars = usize::MAX;
        app.activities = (0..count).map(settled_assistant_block).collect();
    }
    if matches!(
        scenario.as_str(),
        "stream" | "stream-events" | "code" | "tool"
    ) {
        app.ingest_event(provider_started(100_000, "perf-live", "benchmark", "model"));
        if scenario == "code" {
            app.activities
                .back_mut()
                .ok_or("missing activity")?
                .transcript_text = "```rust\n".to_string();
        }
        if scenario == "tool" {
            app.ingest_event(shell_requested(
                100_001,
                "perf-live",
                "perf-tool",
                r#"{"command":"printf output"}"#,
            ));
            app.activities
                .back_mut()
                .ok_or("missing activity")?
                .tool_calls[0]
                .status = ToolCallDisplayStatus::Running;
        }
    }
    if scenario == "stream-events" {
        let history = (0..count).map(|index| {
            provider_started(
                u64::try_from(index).unwrap_or_abort(),
                &format!("resize-perf-{index}"),
                "benchmark",
                "model",
            )
        });
        app.events.splice(..0, history);
    }
    let (mut output, writer, receiver) = FrameOutput::bounded(1);
    let mut terminal = Terminal::with_options(
        FrameOutputBackend::new(writer),
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 160, 48)),
        },
    )?;
    let cold = Instant::now();
    app.set_frame_area(Rect::new(0, 0, 160, 48));
    output.begin_frame()?;
    terminal.draw(|frame| render_app(frame, &app))?;
    output.finish_frame()?;
    receiver.write_next(&mut std::io::sink())?;
    let _ = output.take_acknowledgements();
    let cold_us = cold.elapsed().as_micros();
    let max_scroll = app.transcript_view.last_transcript_max_scroll.get();
    if matches!(
        scenario.as_str(),
        "scroll" | "resize" | "selection" | "hover"
    ) {
        app.set_transcript_scroll_for_test(max_scroll / 2);
    }
    if scenario == "selection" {
        app.transcript_view.transcript_selection = Some(crate::ui::TranscriptSelection {
            anchor: crate::ui::TranscriptSelectionCell {
                row: max_scroll / 2,
                column: 1,
            },
            focus: crate::ui::TranscriptSelectionCell {
                row: max_scroll / 2 + 2,
                column: 20,
            },
        });
    }
    let step = |app: &mut AppState,
                terminal: &mut Terminal<FrameOutputBackend>,
                output: &mut FrameOutput,
                index: usize|
     -> TestResult {
        output.begin_frame()?;
        match scenario.as_str() {
            "startup" => {
                app.advance_wall_clock_for_motion_evidence(std::time::Duration::from_millis(8))
            }
            "static" => {}
            "scroll" | "selection" => {
                if index % 100 < 50 {
                    app.scroll_page_up(1);
                } else {
                    app.scroll_page_down(1);
                }
            }
            "hover" => {
                app.handle_mouse(
                    MouseEvent {
                        kind: MouseEventKind::Moved,
                        column: 20,
                        row: 3 + u16::try_from(index % 30)?,
                        modifiers: KeyModifiers::NONE,
                    },
                    Rect::new(0, 0, 160, 48),
                    None,
                    None,
                    None,
                );
            }
            "typing" => {
                app.handle_key(key(if index % 2 == 0 {
                    KeyCode::Char('x')
                } else {
                    KeyCode::Backspace
                }));
            }
            "stream" | "stream-events" | "code" => {
                app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
                    event_id: format!("perf-delta-{index}"),
                    run_id: "run_app_tests".into(),
                    mono_ms: 100_002 + u64::try_from(index)?,
                    ts: None,
                    actor: EventActor::new(ActorKind::System, None),
                    correlation_id: Some("perf-live".into()),
                    causation_id: None,
                    stream_key: None,
                    payload: LiveEventV1::ProviderTextDelta {
                        request_id: "perf-live".into(),
                        delta: if scenario == "code" {
                            format!("fn value_{index}() -> u64 {{ 42 }}\n")
                        } else {
                            " **token** 界e\u{301}🙂".into()
                        },
                    },
                })))
            }
            "tool" => {
                let tool = &mut app
                    .activities
                    .back_mut()
                    .ok_or("missing activity")?
                    .tool_calls[0];
                tool.output_summary = Some(format!(
                    "{}output line {index}",
                    "synthetic output\n".repeat(100),
                ));
                tool.last_seq += 1;
                app.bump_transcript_render_epoch();
            }
            "resize" => {
                let area = Rect::new(0, 0, if index % 2 == 0 { 80 } else { 160 }, 48);
                app.set_frame_area(area);
                terminal.resize(area)?;
            }
            _ => return Err(format!("unknown performance scenario: {scenario}").into()),
        }
        black_box(app.motion_plan());
        terminal.draw(|frame| render_app(frame, app))?;
        if matches!(output.finish_frame()?, FrameSubmission::Accepted(_)) {
            receiver.write_next(&mut std::io::sink())?;
        }
        let _ = output.take_acknowledgements();
        Ok(())
    };
    for index in 0..10 {
        step(&mut app, &mut terminal, &mut output, index)?;
    }
    let before = process_resources()?;
    let bytes_before = output.metrics().bytes_submitted;
    let started = Instant::now();
    let mut samples_us = Vec::with_capacity(frames);
    for index in 10..frames + 10 {
        let frame_start = Instant::now();
        step(&mut app, &mut terminal, &mut output, index)?;
        samples_us.push(frame_start.elapsed().as_micros());
    }
    let wall_us = started.elapsed().as_micros();
    let after = process_resources()?;
    if matches!(
        scenario.as_str(),
        "scroll" | "typing" | "stream" | "stream-events" | "code" | "tool" | "resize"
    ) {
        assert!(
            output.metrics().bytes_submitted > bytes_before,
            "workload must change the terminal"
        );
    }
    let mut sorted = samples_us.clone();
    sorted.sort_unstable();
    let report = serde_json::json!({
        "benchmark": "interactive_resources", "scenario": scenario, "history": count,
        "frames": frames, "columns": 160, "rows": 48, "cold_us": cold_us,
        "p50_us": sorted[(frames * 50).div_ceil(100) - 1],
        "p95_us": sorted[(frames * 95).div_ceil(100) - 1],
        "p99_us": sorted[(frames * 99).div_ceil(100) - 1],
        "wall_us": wall_us, "cpu_ticks": after.0 - before.0,
        "bytes_submitted": output.metrics().bytes_submitted - bytes_before,
        "rss_before_kib": before.1, "rss_after_kib": after.1, "peak_rss_kib": after.2,
        "samples_us": samples_us,
    });
    println!("{report}");
    if let Some(directory) = std::env::var_os("HARNESS_PERF_ARTIFACT_DIR") {
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{scenario}-{count}.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    terminal.backend_mut().prepare_for_terminal_drop();
    Ok(())
}

fn process_resources() -> TestResult<(u64, u64, u64)> {
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or("invalid proc stat")?
        .1
        .split_whitespace()
        .collect();
    let cpu = fields[11].parse::<u64>()? + fields[12].parse::<u64>()?;
    let status = std::fs::read_to_string("/proc/self/status")?;
    let kib = |key| -> TestResult<u64> {
        Ok(status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .ok_or("missing proc resource")?
            .split_whitespace()
            .next()
            .ok_or("missing resource value")?
            .parse()?)
    };
    Ok((cpu, kib("VmRSS:")?, kib("VmHWM:")?))
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

    // Then: emit the complete sample set and enforce the 120 Hz frame budget at p95.
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
