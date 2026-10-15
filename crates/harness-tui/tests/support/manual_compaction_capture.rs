use crossterm::{
    cursor::{Hide, Show},
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits};
use harness_core::event::RuntimeEvent;
use harness_tui::keybindings::KeyBinding;
use harness_tui::{
    app::{AppState, LaunchMetadata, ModelOption},
    theme::ColorLevel,
    ui::render_app,
    CompactionCommandKind, CompactionCommandOutcome,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use serde::Deserialize;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

type Failure = Box<dyn std::error::Error>;

/// One line of the capture stream. Exactly one of the actions is set per line.
#[derive(Deserialize)]
struct CaptureInput {
    at_ms: u64,
    event: Option<RuntimeEvent>,
    frame_id: Option<usize>,
    /// A chord such as `ctrl+alt+k`, delivered through the real key handler.
    key: Option<String>,
    /// Characters typed one key at a time into whatever has focus.
    text: Option<String>,
    /// The typed outcome of a manual maintenance command.
    outcome: Option<CommandOutcome>,
}

#[derive(Deserialize)]
struct CommandOutcome {
    /// `compact`, `handoff` or `shake`.
    kind: String,
    /// `done`, `warning`, `cancelled` or `failed`.
    result: String,
    message: Option<String>,
    tokens_after: Option<u32>,
}

impl CommandOutcome {
    fn parts(self) -> Result<(CompactionCommandKind, CompactionCommandOutcome), Failure> {
        let kind = match self.kind.as_str() {
            "compact" => CompactionCommandKind::Compact,
            "handoff" => CompactionCommandKind::Handoff,
            "shake" => CompactionCommandKind::Shake,
            other => return Err(format!("unknown command kind {other}").into()),
        };
        let text = self.message.clone().unwrap_or_default();
        let outcome = match self.result.as_str() {
            "done" => CompactionCommandOutcome::Done {
                message: self.message,
                tokens_after: self.tokens_after,
                saved_path: None,
                rebuild: self.tokens_after.is_some(),
            },
            "warning" => CompactionCommandOutcome::Warning(text),
            "cancelled" => CompactionCommandOutcome::Cancelled,
            "failed" => CompactionCommandOutcome::Failed(text),
            other => return Err(format!("unknown command result {other}").into()),
        };
        Ok((kind, outcome))
    }
}

struct RestoreTerminal(std::fs::File);
impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = execute!(self.0, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn model(name: &str, window: u32) -> ModelOption {
    let mut option = ModelOption::from_model_ref("default", &format!("mock:{name}"));
    option.model_limits = ResolvedModelLimits::from_values(
        Some(window),
        Some(window),
        Some(8_192),
        ModelLimitProvenance::default(),
    );
    option
}

/// A live shell with a known 200K window, two switchable models, and a project config
/// under the capture's isolated home so settings writes never touch real configuration.
fn fixture_app() -> Result<AppState, Failure> {
    let mut app = AppState::new_live(None, false, Some(Arc::new(|_| {})));
    app.set_startup_logo_capabilities_for_evidence(
        ColorLevel::Ansi256,
        harness_tui::theme::GlyphMode::Preferred,
    );
    app.set_reduced_motion_for_evidence(std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some());
    app.freeze_animation_clock();
    app.startup_mode = false;
    let active = model("default", 200_000);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&active)
            .with_mode_label("Local fixture")
            .with_available_models(vec![active.clone(), model("small", 128_000)]),
    );
    let home = std::env::var_os("HARNESS_HOME").ok_or("HARNESS_HOME is not set")?;
    let project = std::path::PathBuf::from(home).join("project");
    std::fs::create_dir_all(&project)?;
    app.bind_settings_project_config(
        project.join("harness.jsonc"),
        false,
        false,
        false,
        false,
        false,
    );
    app.configure_collapsed_history();
    Ok(app)
}

fn key_event(chord: &str) -> Result<KeyEvent, Failure> {
    let binding: KeyBinding = chord.parse()?;
    Ok(KeyEvent::new(binding.code, binding.modifiers))
}

pub(crate) fn run() -> Result<(), Failure> {
    let Some(path) = std::env::var_os("HARNESS_TUI_COMPACTION_STREAM") else {
        return Ok(());
    };
    let reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut tty = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
    enable_raw_mode()?;
    let _restore = RestoreTerminal(tty.try_clone()?);
    execute!(tty, EnterAlternateScreen, Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(tty))?;
    let mut app = fixture_app()?;
    let mut at_ms = 0;
    for line in reader.lines() {
        let input: CaptureInput = serde_json::from_str(&line?)?;
        let delta = input
            .at_ms
            .checked_sub(at_ms)
            .ok_or("capture clock moved backwards")?;
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(delta));
        at_ms = input.at_ms;
        if let Some(event) = input.event {
            app.ingest_runtime_event(event);
        }
        if let Some((kind, outcome)) = input.outcome.map(CommandOutcome::parts).transpose()? {
            app.apply_compaction_command_outcome(kind, outcome);
        }
        if input.key.is_some() || input.text.is_some() {
            // Key actions read the measured viewport, so lay the transcript out first.
            app.set_frame_area(terminal.size()?.into());
            terminal.draw(|frame| render_app(frame, &app))?;
            for character in input.text.iter().flat_map(|text| text.chars()) {
                app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
            }
            if let Some(chord) = &input.key {
                app.handle_key(key_event(chord)?);
            }
        }
        let _ = app.refresh_motion_for_evidence();
        if let Some(id) = input.frame_id {
            app.set_frame_area(terminal.size()?.into());
            // The first pass measures a changed transcript; the second places the viewport.
            terminal.draw(|frame| render_app(frame, &app))?;
            terminal.draw(|frame| render_app(frame, &app))?;
            write!(
                terminal.backend_mut(),
                "\u{1b}]0;Harness compaction frame {id}\u{7}"
            )?;
            terminal.backend_mut().flush()?;
        }
    }
    Ok(())
}
