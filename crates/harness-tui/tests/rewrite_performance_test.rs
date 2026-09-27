//! Release measurements through the same public event/input/render boundaries on both builds.
use std::{cell::Cell, collections::VecDeque, fs, io::Write, time::Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{EventEnvelopeV1, RuntimeEvent};
use harness_tui::ui::render_app;
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};
use serde_json::json;

#[path = "support/rewrite_journey.rs"]
mod journey;
use journey::{envelope, Journey, Result};

struct Output<'a>(&'a Cell<usize>);
impl Write for Output<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.set(self.0.get() + bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn resources() -> Result<serde_json::Value> {
    let status = fs::read_to_string("/proc/self/status")?;
    let stat = fs::read_to_string("/proc/self/stat")?;
    let fields = stat
        .rsplit_once(')')
        .ok_or("invalid proc stat")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let memory = |key: &str| -> Result<u64> {
        Ok(status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .ok_or("missing memory metric")?
            .split_whitespace()
            .next()
            .ok_or("empty metric")?
            .parse()?)
    };
    Ok(
        json!({"cpu_ticks": fields[11].parse::<u64>()? + fields[12].parse::<u64>()?,
        "rss_kib": memory("VmRSS:")?, "peak_rss_kib": memory("VmHWM:")?}),
    )
}

fn history(turns: usize, turn_tasks: bool) -> Result<Vec<EventEnvelopeV1>> {
    let mut result = Vec::with_capacity(turns * if turn_tasks { 6 } else { 4 });
    for index in 0..turns {
        let request = format!("history-{index}");
        for (kind, data) in [
            (
                "user_message_submitted",
                json!({"request_id": request, "text": format!("Prompt {index:05}")}),
            ),
            (
                "provider_request_started",
                json!({"request_id": request, "provider_id": "mock",
                "model_id": "reference", "prompt_summary": "Fixture", "request_digest": "fixture", "metadata": null}),
            ),
            (
                "assistant_message_finished",
                json!({"request_id": request, "tool_call_count": 0,
                "parts": [{"kind": "text", "text": format!("Response {index:05}: 界 e\u{301} 👩‍💻 retained history.")}],
                "provenance": null, "assistant_message": null}),
            ),
            (
                "provider_request_finished",
                json!({"request_id": request, "finish_reason": "stop",
                "output_digest": "fixture", "usage": null, "metadata": null}),
            ),
        ] {
            let seq = u64::try_from(result.len())? + 1;
            let mut value = envelope(seq, kind, data);
            value["correlation_id"] = json!(request);
            result.push(serde_json::from_value(value)?);
            if turn_tasks && kind == "user_message_submitted" {
                let mut value = envelope(
                    u64::try_from(result.len())? + 1,
                    "task_scheduled",
                    json!({"task_id": request, "queue_key": "provider_model:mock:reference",
                        "state": "started", "metadata": null}),
                );
                value["correlation_id"] = json!(request);
                result.push(serde_json::from_value(value)?);
            }
        }
        if turn_tasks {
            let mut value = envelope(
                u64::try_from(result.len())? + 1,
                "task_completed",
                json!({"task_id": request, "result_summary": "Fixture completed",
                    "result_digest": "fixture", "metadata": {"task_scope": "agent_turn"}}),
            );
            value["correlation_id"] = json!(request);
            result.push(serde_json::from_value(value)?);
        }
    }
    Ok(result)
}

#[test]
#[allow(
    clippy::cognitive_complexity,
    clippy::assertions_on_constants,
    reason = "one serial workload fixture; deliberately rejects debug measurements at runtime"
)]
fn perf_rewrite_public_boundary_workloads() -> Result {
    assert!(
        !cfg!(debug_assertions),
        "performance evidence requires --release"
    );
    let scenario = std::env::var("HARNESS_REWRITE_SCENARIO").unwrap_or_else(|_| "resize".into());
    let count: usize = std::env::var("HARNESS_REWRITE_HISTORY")
        .unwrap_or_else(|_| "1000".into())
        .parse()?;
    let frames: usize = std::env::var("HARNESS_REWRITE_FRAMES")
        .unwrap_or_else(|_| "200".into())
        .parse()?;
    assert!(frames >= 100, "p99 needs at least 100 samples");
    let workspace = tempfile::tempdir()?;
    std::env::set_current_dir(workspace.path())?;
    let events = history(count, scenario == "settle")?;
    let event_count = events.len();
    let construction = Instant::now();
    let mut j = Journey::new(scenario == "startup");
    if count > 0 {
        j.app.replace_events(events);
    }
    let construction_us = construction.elapsed().as_micros();
    let mut updates = VecDeque::new();
    if scenario == "stream" {
        j.seq = u64::try_from(count * 4)?;
        j.start("turn", "Stream synthetic text")?;
        for index in 0..frames + 10 {
            updates.push_back(serde_json::from_value::<RuntimeEvent>(
                json!({"delivery": "live",
                "event": envelope(u64::try_from(index)?, "provider_text_delta",
                    json!({"request_id": "turn", "delta": " **token** 界 e\u{301} 👩‍💻"}))}),
            )?);
        }
    }
    let mut durable_updates = if scenario == "settle" {
        history(count + frames + 10, true)?
            .into_iter()
            .skip(event_count)
            .collect::<VecDeque<_>>()
    } else {
        VecDeque::new()
    };
    let mut area = Rect::new(0, 0, 160, 48);
    let output_bytes = Cell::new(0);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(Output(&output_bytes)),
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )?;
    let cold = Instant::now();
    j.app.set_frame_area(area);
    terminal.draw(|frame| render_app(frame, &j.app))?;
    let cold_us = cold.elapsed().as_micros();
    if matches!(scenario.as_str(), "scroll" | "resize") {
        j.app.scroll_page_up(24);
    }
    let mut samples_us = Vec::with_capacity(frames);
    let mut before = serde_json::Value::Null;
    let mut bytes_before = 0;
    for index in 0..frames + 10 {
        if index == 10 {
            before = resources()?;
            bytes_before = output_bytes.get();
        }
        let start = Instant::now();
        match scenario.as_str() {
            "startup" | "idle" => {}
            "typing" => j.app.handle_key(KeyEvent::new(
                if index % 2 == 0 {
                    KeyCode::Char('x')
                } else {
                    KeyCode::Backspace
                },
                KeyModifiers::NONE,
            )),
            "stream" => j
                .app
                .ingest_runtime_event(updates.pop_front().ok_or("missing stream event")?),
            "settle" => {
                for _ in 0..6 {
                    j.app
                        .ingest_event(durable_updates.pop_front().ok_or("missing durable event")?);
                }
            }
            "scroll" => {
                if index % 40 < 20 {
                    j.app.scroll_page_up(1);
                } else {
                    j.app.scroll_page_down(1);
                }
            }
            "resize" => {
                area.width = if index % 2 == 0 { 80 } else { 160 };
                terminal.resize(area)?;
            }
            _ => return Err(format!("unknown workload: {scenario}").into()),
        }
        j.app.set_frame_area(area);
        terminal.draw(|frame| render_app(frame, &j.app))?;
        if index >= 10 {
            samples_us.push(start.elapsed().as_micros());
        }
    }
    let after = resources()?;
    let bytes = output_bytes.get() - bytes_before;
    let visible = screen(&mut j, area)?;
    if scenario == "stream" {
        assert!(
            visible.contains("token"),
            "stream output never became visible"
        );
    }
    if scenario == "settle" {
        assert!(durable_updates.is_empty(), "durable updates were dropped");
        assert_eq!(j.app.canonical_projection_error(), None);
        assert_eq!(
            j.app.canonical_projection_generation(),
            u64::try_from(usize::from(count > 0) + 3 * (frames + 10))?
        );
        assert!(
            visible.contains(&format!("Response {:05}", count + frames + 9)),
            "last settled response must be visible: {visible}"
        );
    }
    let mut oldest = String::new();
    if count > 0 {
        j.app.scroll_goto_top();
        oldest = screen(&mut j, area)?;
        if count <= 2000 {
            assert!(oldest.contains("Prompt 00000"), "history was discarded");
        }
    }
    if !matches!(scenario.as_str(), "startup" | "idle") {
        assert!(bytes > 0, "workload must produce terminal output");
    }
    let mut sorted = samples_us.clone();
    sorted.sort_unstable();
    let report = json!({"schema": "tui-rewrite-public-perf-v1", "scenario": scenario,
        "history_turns": count, "history_events": event_count, "frames": frames,
        "construction_us": construction_us, "cold_us": cold_us, "samples_us": samples_us,
        "p50_us": sorted[frames * 50 / 100 - 1], "p95_us": sorted[frames * 95 / 100 - 1],
        "p99_us": sorted[frames * 99 / 100 - 1], "before": before, "after": after,
        "bytes": bytes, "visible": visible, "oldest": oldest,
        "boundary": "public event/input handlers, frame preparation, render_app, Ratatui diff, Crossterm encoding to counting sink"});
    if let Some(path) = std::env::var_os("HARNESS_REWRITE_PERF_OUT") {
        fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    println!("{report}");
    Ok(())
}

fn screen(journey: &mut Journey, area: Rect) -> Result<String> {
    journey.app.set_frame_area(area);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))?;
    terminal.draw(|frame| render_app(frame, &journey.app))?;
    Ok(terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n"))
}
