//! Whole-frame behavioral oracle captured before replacing the TUI.
//! Only the pinned original source may record expectations. Candidate runs compare.
use std::{fs, path::PathBuf, process::Command};

use crossterm::event::{KeyCode as K, KeyModifiers as M};
use harness_tui::{
    theme_family::{serialize_choice, ThemeChoice},
    ui::render_app,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};
use serde_json::{json, Value};

#[path = "support/rewrite_journey.rs"]
mod journey;
#[path = "support/rewrite_workflows.rs"]
mod workflows;
use journey::{Journey, Result};

const BASE: &str = "1bb0f98988670a5f4b48cdf749b455a79cfdaa82";
const GOLDEN: &str = "tests/fixtures/tui-reference.cells.jsonl";

struct Recorder {
    frames: Vec<Value>,
    output: Option<PathBuf>,
    area: Rect,
}

impl Recorder {
    fn frame(&mut self, name: &str, journey: &mut Journey) -> Result {
        journey.app.set_frame_area(self.area);
        let mut terminal = Terminal::new(TestBackend::new(self.area.width, self.area.height))?;
        let interaction = journey.app.transcript_interaction_snapshot();
        terminal.draw(|frame| render_app(frame, &journey.app))?;
        assert_eq!(
            interaction,
            journey.app.transcript_interaction_snapshot(),
            "paint changed interaction state: {name}"
        );
        let first = terminal.backend().buffer().clone();
        terminal.draw(|frame| render_app(frame, &journey.app))?;
        let buffer = terminal.backend().buffer().clone();
        let cursor = terminal
            .backend()
            .cursor_visible()
            .then(|| terminal.backend().cursor_position());
        assert!(
            first == buffer,
            "first paint differs from settled paint: {name}"
        );
        journey.app.set_frame_area(self.area);
        assert_eq!(
            interaction,
            journey.app.transcript_interaction_snapshot(),
            "repeated frame preparation changed interaction state: {name}"
        );
        terminal.draw(|frame| render_app(frame, &journey.app))?;
        assert_eq!(
            &buffer,
            terminal.backend().buffer(),
            "repeated frame preparation changed paint: {name}"
        );
        let id = format!(
            "{name}-{}x{}-reduced-0ms",
            self.area.width, self.area.height
        );
        self.frames.push(json!({
            "id": id, "cursor": cursor.map(|p| (p.x, p.y)), "cells": cells(&buffer),
            "inputs": std::mem::take(&mut journey.inputs),
            "intents": *journey.intents.lock().unwrap_or_else(|e| e.into_inner()),
        }));
        if let Some(output) = &self.output {
            let mut bytes = Vec::new();
            {
                let mut ansi = Terminal::with_options(
                    CrosstermBackend::new(&mut bytes),
                    TerminalOptions {
                        viewport: Viewport::Fixed(self.area),
                    },
                )?;
                ansi.draw(|frame| render_app(frame, &journey.app))?;
            }
            fs::write(output.join(format!("{id}.ansi")), bytes)?;
        }
        Ok(())
    }

    fn menus(&mut self) -> Result {
        for command in [
            "help",
            "sessions",
            "models",
            "agents",
            "mcps",
            "toggles",
            "auth",
            "connect",
            "settings",
            "view-plan",
            "worktree",
            "dashboard",
            "usage",
            "extensions",
            "tree",
            "fork",
            "clone",
            "rewind",
            "import",
        ] {
            let mut journey = Journey::new(false);
            journey.text(&format!("/{command}"));
            self.frame(&format!("slash-{command}"), &mut journey)?;
            journey.key(K::Enter, M::NONE);
            self.frame(&format!("dialog-{command}"), &mut journey)?;
            journey.key(K::Down, M::NONE);
            self.frame(&format!("selected-{command}"), &mut journey)?;
            if command == "dashboard" {
                journey.key(K::Char('d'), M::NONE);
                self.frame("dashboard-details", &mut journey)?;
            }
        }
        for (name, query) in [
            ("memory", "Memory"),
            ("worktree", "New Session in Worktree"),
            ("stash", "Stash list"),
        ] {
            let mut journey = Journey::new(false);
            journey.key(K::Char('p'), M::CONTROL);
            journey.text(query);
            self.frame(&format!("palette-{name}"), &mut journey)?;
            journey.key(K::Enter, M::NONE);
            self.frame(&format!("opened-{name}"), &mut journey)?;
        }
        let mut j = Journey::new(false);
        j.key(K::Char('x'), M::CONTROL);
        j.key(K::Char('t'), M::NONE);
        self.frame("opened-theme", &mut j)?;
        j.key(K::Down, M::NONE);
        self.frame("theme-preview", &mut j)?;
        j.key(K::Esc, M::NONE);
        self.frame("theme-cancelled", &mut j)?;
        let mut j = Journey::new(true);
        j.key(K::Tab, M::NONE);
        j.key(K::Down, M::NONE);
        j.key(K::Down, M::NONE);
        j.key(K::Enter, M::NONE);
        self.frame("opened-changelog", &mut j)?;
        Ok(())
    }

    fn composer(&mut self) -> Result {
        let mut j = Journey::new(true);
        self.frame("startup", &mut j)?;
        j.text("draft 界 e\u{301} 👩‍💻");
        j.key(K::Left, M::NONE);
        j.key(K::Backspace, M::NONE);
        self.frame("editing-unicode", &mut j)?;
        j.key(K::Char('z'), M::CONTROL);
        self.frame("editing-undo", &mut j)?;
        j.key(K::Enter, M::SHIFT);
        j.text("second line");
        self.frame("editing-multiline", &mut j)?;
        j.paste("pasted 界\nthird line");
        self.frame("editing-paste", &mut j)?;
        j.key(K::Char('p'), M::CONTROL);
        self.frame("draft-palette", &mut j)?;
        j.key(K::Esc, M::NONE);
        self.frame("draft-restored", &mut j)?;
        j.key(K::Esc, M::NONE);
        self.frame("draft-clear-confirm", &mut j)?;
        j.key(K::Esc, M::NONE);
        self.frame("draft-cleared", &mut j)?;
        j.text("submit fixture");
        j.key(K::Enter, M::NONE);
        self.frame("draft-submitted", &mut j)?;
        Ok(())
    }

    fn session(&mut self) -> Result {
        let mut j = Journey::new(false);
        self.frame("empty-live", &mut j)?;
        j.start("turn", "Inspect the terminal fixture")?;
        self.frame("request-started", &mut j)?;
        j.live(
            "provider_reasoning_delta",
            json!({"request_id": "turn", "delta": "Synthetic reasoning fixture."}),
        )?;
        self.frame("reasoning", &mut j)?;
        let message = "## Fixture\n\n**Bold** and `code`, 界 e\u{301} 👩‍💻.\n\n- first\n- second\n\n```rust\nfn main() {\n    println!(\"ready\");\n}\n```\n\n| Name | State |\n| --- | --- |\n| fixture | ready |\n";
        j.live(
            "provider_text_delta",
            json!({"request_id": "turn", "delta": message}),
        )?;
        self.frame("stream-markdown", &mut j)?;
        j.text("queued fixture");
        j.key(K::Enter, M::NONE);
        self.frame("queued-prompt", &mut j)?;
        let before_queue_navigation = self.frames.last().cloned().ok_or("missing queued frame")?;
        j.key(K::Up, M::NONE);
        self.frame("queued-entry-navigation", &mut j)?;
        j.key(K::Esc, M::NONE);
        self.frame("queued-entry-return", &mut j)?;
        let after_queue_navigation = self.frames.last().ok_or("missing return frame")?;
        for field in ["cells", "cursor", "intents"] {
            assert_eq!(
                before_queue_navigation[field], after_queue_navigation[field],
                "queued return changed {field}"
            );
        }
        j.finish("turn", message)?;
        self.frame("completed-turn", &mut j)?;
        for index in 0..30 {
            let id = format!("turn-{index}");
            j.start(&id, &format!("Prompt {index:02}"))?;
            j.finish(
                &id,
                &format!("Response {index:02}: keep the detached viewport stable."),
            )?;
        }
        self.frame("long-history", &mut j)?;
        j.key(K::PageUp, M::NONE);
        self.frame("detached-history", &mut j)?;
        j.start("append", "New work below the detached viewport")?;
        self.frame("detached-append", &mut j)?;
        let initial = self.area;
        self.area.width = if initial.width > 80 { 80 } else { 120 };
        self.frame(&format!("detached-resize-from-{}", initial.width), &mut j)?;
        self.area = initial;
        j.key(K::Tab, M::NONE);
        j.key(K::Char('/'), M::NONE);
        j.text("Response 12");
        self.frame("transcript-search", &mut j)?;
        j.key(K::Esc, M::NONE);
        j.event(
            "run_failed",
            json!({"error": "Synthetic failure: terminal fixture"}),
        )?;
        self.frame("run-failed", &mut j)?;
        Ok(())
    }

    fn permissions(&mut self) -> Result {
        for question in [false, true] {
            let prefix = if question { "question" } else { "permission" };
            let mut j = Journey::new(false);
            j.text("keep this draft");
            j.key(K::Char('p'), M::CONTROL);
            j.permission(question)?;
            self.frame(&format!("{prefix}-over-palette"), &mut j)?;
            j.key(K::Down, M::NONE);
            j.key(K::Tab, M::NONE);
            self.frame(&format!("{prefix}-selection"), &mut j)?;
            j.key(K::Esc, M::NONE);
            self.frame(&format!("{prefix}-parked"), &mut j)?;
            j.key(K::Tab, M::NONE);
            j.key(K::Char('z'), M::NONE);
            j.text("fixture answer");
            self.frame(&format!("{prefix}-feedback"), &mut j)?;
            j.event(
                "permission_resolved",
                json!({"permission_id": "permission", "decision": "deny", "reason": "fixture"}),
            )?;
            self.frame(&format!("{prefix}-resolved"), &mut j)?;
        }
        Ok(())
    }
}

// Run-length encoding preserves every blank cell, grapheme, color, and modifier.
fn cells(buffer: &Buffer) -> Vec<Value> {
    cell_runs(buffer.content.iter().map(|cell| {
        json!([
            cell.symbol(),
            format!("{:?}", cell.fg),
            format!("{:?}", cell.bg),
            cell.modifier.bits(),
            format!("{:?}", cell.diff_option),
            format!("{:?}", cell.underline_color)
        ])
    }))
}

fn cell_runs(cells: impl Iterator<Item = Value>) -> Vec<Value> {
    let mut runs: Vec<Value> = Vec::new();
    for value in cells {
        if let Some(last) = runs.last_mut().filter(|last| last[1] == value) {
            last[0] = json!(last[0].as_u64().unwrap_or(0) + 1);
        } else {
            runs.push(json!([1, value]));
        }
    }
    runs
}

fn cells_with_documented_gap_correction(frame: &Value) -> Result<Value> {
    let bottom = match frame["id"].as_str() {
        Some("detached-history-40x24-reduced-0ms") => 17,
        Some("detached-append-40x24-reduced-0ms") => 15,
        _ => return Ok(frame["cells"].clone()),
    };
    let mut cells = Vec::new();
    for run in frame["cells"].as_array().ok_or("missing reference cells")? {
        cells.extend(std::iter::repeat_n(
            run[1].clone(),
            usize::try_from(run[0].as_u64().ok_or("invalid cell run")?)?,
        ));
    }
    // R8: retain the blank row that the reference's second paint skipped.
    // Move only the recorded transcript body down one row; chrome, scrollbar,
    // styles, cursor, input and intents still compare against the frozen original.
    for row in (3..bottom).rev() {
        for column in 2..37 {
            cells[row * 40 + column] = cells[(row - 1) * 40 + column].clone();
        }
    }
    Ok(json!(cell_runs(cells.into_iter())))
}

#[test]
fn recorded_terminal_journeys_match_reference_cells_and_intents() -> Result {
    // Nextest gives this test its own process. Keep filesystem discovery off the checkout.
    let workspace = tempfile::tempdir()?;
    std::env::set_current_dir(workspace.path())?;
    let output = std::env::var_os("HARNESS_TUI_REFERENCE_FRAMES").map(PathBuf::from);
    if let Some(path) = &output {
        fs::create_dir_all(path)?;
    }
    let original = original_source()?;
    let mut r = Recorder {
        frames: Vec::new(),
        output,
        area: Rect::default(),
    };
    for (width, height) in [(40, 24), (80, 24), (120, 40), (160, 50)] {
        r.area = Rect::new(0, 0, width, height);
        r.composer()?;
        r.session()?;
        r.permissions()?;
        r.menus()?;
        r.working_permissions()?;
        r.populated_dialogs(std::path::Path::new("."))?;
        r.mentions()?;
        for theme in [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::Auto] {
            let mut j = Journey::new(true);
            j.app.restore_theme_choice(&serialize_choice(theme)?)?;
            r.frame(&format!("theme-{}", theme.label()), &mut j)?;
        }
    }
    r.area = Rect::new(0, 0, 120, 40);
    r.disk_sessions(std::path::Path::new("."))?;
    if let Some(output) = &r.output {
        fs::write(output.join("cells.json"), serde_json::to_vec(&r.frames)?)?;
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("HARNESS_TUI_RECORD_REFERENCE").is_some() {
        assert!(
            original,
            "only the pinned original implementation can record reference cells"
        );
        fs::write(
            path,
            r.frames
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )?;
    } else {
        let expected = fs::read_to_string(path)?;
        let expected = expected
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(
            expected.len(),
            r.frames.len(),
            "reference matrix is incomplete"
        );
        for (expected, actual) in expected.iter().zip(&r.frames) {
            let mut expected = expected.clone();
            expected["cells"] = cells_with_documented_gap_correction(&expected)?;
            for field in ["id", "cursor", "cells", "inputs", "intents"] {
                assert!(
                    expected[field] == actual[field],
                    "reference drift in {}: {field}",
                    actual["id"]
                );
            }
        }
    }
    Ok(())
}

fn original_source() -> Result<bool> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    Ok(Command::new("git")
        .current_dir(root)
        .args([
            "diff",
            "--quiet",
            BASE,
            "--",
            "crates/harness-tui/src",
            "crates/harness-core/src",
            "Cargo.lock",
        ])
        .status()?
        .success())
}
