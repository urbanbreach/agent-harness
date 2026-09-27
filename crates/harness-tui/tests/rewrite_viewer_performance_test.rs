//! Opt-in Linux release diagnostics for the production full-screen viewer.
use std::{cell::Cell, fs, io::Write, time::Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::{app::Focus, ui::render_app};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal, TerminalOptions, Viewport};
use serde_json::json;

#[path = "support/rewrite_journey.rs"]
mod journey;
use journey::{Journey, Result};

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

fn screen(buffer: &ratatui::buffer::Buffer) -> String {
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[allow(
    clippy::assertions_on_constants,
    clippy::cognitive_complexity,
    reason = "one opt-in public viewer workload, with a release-only measurement guard"
)]
fn perf_rewrite_viewer_public_boundary() -> Result {
    assert!(
        !cfg!(debug_assertions),
        "performance evidence requires --release"
    );
    let scenario = std::env::var("HARNESS_VIEWER_SCENARIO").unwrap_or_else(|_| "scroll".into());
    let lines: usize = std::env::var("HARNESS_VIEWER_LINES")
        .unwrap_or_else(|_| "2000".into())
        .parse()?;
    let frames: usize = std::env::var("HARNESS_VIEWER_FRAMES")
        .unwrap_or_else(|_| "200".into())
        .parse()?;
    assert!(
        lines >= 1000 && frames >= 100,
        "need a large viewer and at least 100 samples"
    );
    let workspace = tempfile::tempdir()?;
    std::env::set_current_dir(workspace.path())?;
    let construction = Instant::now();
    let mut j = Journey::new(false);
    j.start("turn", "Inspect recorded output")?;
    let output = (0..lines)
        .map(|i| format!("line_{i:05} retained output 界 e\u{301} 👩‍💻 alpha beta gamma delta epsilon zeta eta theta iota kappa lambda"))
        .collect::<Vec<_>>()
        .join("\n");
    j.event("tool_call_requested", json!({"tool_call_id":"viewer-output", "tool_id":"bash",
        "args_summary":"{\"command\":\"printf recorded_output\"}", "args_digest":"fixture", "metadata":null}))?;
    j.event(
        "provider_request_finished",
        json!({"request_id":"turn", "finish_reason":"tool_calls",
        "output_digest":null,"usage":null,"metadata":null}),
    )?;
    j.event("tool_call_started", json!({"tool_call_id":"viewer-output"}))?;
    j.event(
        "tool_call_finished",
        json!({"tool_call_id":"viewer-output", "status":"succeeded",
        "output_summary":output,"output_digest":null,
        "output_json":{"stdout":output,"stderr":"","exit_code":0},"metadata":null}),
    )?;
    // Input receipts are not part of application memory; retain only the app's copies.
    j.inputs.clear();
    drop(output);
    let construction_us = construction.elapsed().as_micros();
    let mut area = Rect::new(0, 0, 120, 40);
    let bytes = Cell::new(0);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(Output(&bytes)),
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )?;
    let opening = Instant::now();
    j.app.set_frame_area(area);
    terminal.draw(|frame| render_app(frame, &j.app))?;
    j.app.focus = Focus::Details;
    assert!(j.app.select_transcript_tool("viewer-output"));
    j.key(KeyCode::Enter, KeyModifiers::NONE);
    j.app.set_frame_area(area);
    terminal.draw(|frame| render_app(frame, &j.app))?;
    let opening_us = opening.elapsed().as_micros();
    assert!(
        j.app.transcript_viewer_mode().is_some(),
        "Enter did not open the viewer"
    );
    if scenario == "search" {
        j.key(KeyCode::Char('/'), KeyModifiers::NONE);
        j.text("line_0050");
    }
    let mut samples_us = Vec::with_capacity(frames);
    let mut phases_us = Vec::with_capacity(frames);
    let mut scroll_rows = Vec::new();
    let mut search_labels = Vec::new();
    let mut resize_output_lines = Vec::new();
    let mut before = serde_json::Value::Null;
    let mut bytes_before = 0;
    for index in 0..frames + 10 {
        if index == 10 {
            before = resources()?;
            bytes_before = bytes.get();
        }
        let start = Instant::now();
        match scenario.as_str() {
            "idle" => {}
            "scroll" => j.app.handle_key(KeyEvent::new(
                if index % 40 < 20 {
                    KeyCode::Char('j')
                } else {
                    KeyCode::Char('k')
                },
                KeyModifiers::CONTROL,
            )),
            "search" => j.app.handle_key(KeyEvent::new(
                if index % 2 == 0 {
                    KeyCode::Char('5')
                } else {
                    KeyCode::Backspace
                },
                KeyModifiers::NONE,
            )),
            "resize" => {
                area.width = if index % 2 == 0 { 80 } else { 120 };
                terminal.resize(area)?;
            }
            _ => return Err(format!("unknown viewer workload: {scenario}").into()),
        }
        let input_us = start.elapsed().as_micros();
        j.app.set_frame_area(area);
        let prepared_us = start.elapsed().as_micros();
        let painted = terminal.draw(|frame| render_app(frame, &j.app))?;
        if index >= 10 {
            let total_us = start.elapsed().as_micros();
            samples_us.push(total_us);
            phases_us.push([input_us, prepared_us - input_us, total_us - prepared_us]);
        }
        if scenario == "scroll" && matches!(index, 10 | 19) {
            scroll_rows.push(
                screen(painted.buffer)
                    .lines()
                    .find(|row| row.contains("line_"))
                    .ok_or("missing visible output row")?
                    .to_owned(),
            );
        }
        if scenario == "search" && matches!(index, 10 | 11) {
            search_labels.push(
                screen(painted.buffer)
                    .lines()
                    .find(|row| row.contains("search:"))
                    .ok_or("missing search input")?
                    .to_owned(),
            );
        }
        if scenario == "resize" && matches!(index, 10 | 11) {
            resize_output_lines.push(
                screen(painted.buffer)
                    .lines()
                    .filter(|row| row.contains("line_"))
                    .count(),
            );
        }
    }
    let after = resources()?;
    let output_bytes = bytes.get() - bytes_before;
    if scenario == "scroll" {
        assert_eq!(scroll_rows.len(), 2);
        assert_ne!(
            scroll_rows[0], scroll_rows[1],
            "scroll input did not move visible content"
        );
    }
    let visible = screen(terminal.draw(|frame| render_app(frame, &j.app))?.buffer);
    if scenario == "search" {
        assert_eq!(search_labels.len(), 2);
        assert!(search_labels[0].contains("search: line_00505"));
        assert!(
            search_labels[1].contains("search: line_0050")
                && !search_labels[1].contains("line_00505")
        );
        let expected = if frames % 2 == 0 {
            "line_00500"
        } else {
            "line_00505"
        };
        assert!(visible.contains(&format!("{expected} retained")));
        j.key(KeyCode::Esc, KeyModifiers::NONE);
    }
    if scenario == "resize" {
        assert_eq!(resize_output_lines.len(), 2);
        assert!(
            resize_output_lines[0] < resize_output_lines[1],
            "narrow resize did not reflow output"
        );
    }
    j.key(KeyCode::End, KeyModifiers::NONE);
    j.app.set_frame_area(area);
    let tail = screen(terminal.draw(|frame| render_app(frame, &j.app))?.buffer);
    assert!(
        tail.contains(&format!("line_{:05}", lines - 1)),
        "viewer discarded its tail"
    );
    j.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(
        j.app.transcript_viewer_mode().is_none(),
        "viewer did not close"
    );
    let mut sorted = samples_us.clone();
    sorted.sort_unstable();
    let report = json!({"schema":"tui-rewrite-viewer-perf-v1", "scenario":scenario,
        "lines":lines,"frames":frames,"construction_us":construction_us,"cold_us":opening_us,
        "samples_us":samples_us,"phase_order":["input","prepare","paint_and_ansi"],"phases_us":phases_us,
        "p50_us":sorted[frames*50/100-1],"p95_us":sorted[frames*95/100-1],
        "p99_us":sorted[frames*99/100-1],"bytes":output_bytes,"before":before,"after":after,
        "visible":visible,"tail":tail,"scroll_rows":scroll_rows,
        "search_labels":search_labels,"resize_output_lines":resize_output_lines});
    if let Some(path) = std::env::var_os("HARNESS_VIEWER_PERF_OUT") {
        fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    println!("{report}");
    Ok(())
}
