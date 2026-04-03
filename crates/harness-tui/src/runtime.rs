use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use harness_core::event::EventEnvelopeV1;
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{AppState, LaunchMetadata, SessionHistoryEntry, UiIntent};
use crate::event::{self, poll};
use crate::ui;

fn recover_mutex_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn pending_replay_launch_metadata() -> &'static Mutex<Option<LaunchMetadata>> {
    static PENDING: OnceLock<Mutex<Option<LaunchMetadata>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

pub fn set_pending_replay_launch_metadata(launch_metadata: Option<LaunchMetadata>) {
    *recover_mutex_lock(pending_replay_launch_metadata()) = launch_metadata;
}

fn take_pending_replay_launch_metadata() -> Option<LaunchMetadata> {
    recover_mutex_lock(pending_replay_launch_metadata()).take()
}

pub enum LiveUpdate {
    Event(Box<EventEnvelopeV1>),
    Status(String),
}

pub enum TuiMode {
    Startup {
        session_history_entries: Vec<SessionHistoryEntry>,
    },
    Replay {
        run_dir: PathBuf,
        events: Vec<EventEnvelopeV1>,
    },
    Live {
        run_dir: PathBuf,
        historical_events: Vec<EventEnvelopeV1>,
        update_rx: Receiver<LiveUpdate>,
    },
}

pub struct TuiOptions {
    pub mode: TuiMode,
    pub exit_on_finish: bool,
    pub on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    pub keybindings: Option<std::collections::BTreeMap<String, String>>,
}

impl TuiOptions {
    fn take_external_keybindings(&mut self) -> Option<std::collections::BTreeMap<String, String>> {
        self.keybindings
            .take()
            .filter(|bindings| !bindings.is_empty())
    }
}

pub fn run_tui_with_options(mut options: TuiOptions) -> Result<()> {
    let keybindings = options.take_external_keybindings();
    let TuiOptions {
        mode,
        exit_on_finish,
        on_ui_intent,
        keybindings: _,
    } = options;

    let (mut app, live_updates) = match mode {
        TuiMode::Startup {
            session_history_entries,
        } => {
            let mut app = AppState::new_startup(session_history_entries, on_ui_intent);
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            (app, None)
        }
        TuiMode::Replay { run_dir, events } => {
            let mut app = AppState::new_replay(run_dir, events);
            if let Some(launch_metadata) = take_pending_replay_launch_metadata() {
                app.set_launch_metadata(launch_metadata);
            }
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            (app, None)
        }
        TuiMode::Live {
            run_dir,
            historical_events,
            update_rx,
        } => {
            let mut app = AppState::new_live(Some(run_dir), exit_on_finish, on_ui_intent);
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            for event in historical_events {
                app.ingest_historical_event(event);
            }
            (app, Some(update_rx))
        }
    };

    crossterm::terminal::enable_raw_mode().context("failed to enable terminal raw mode")?;
    let mut stdout = std::io::stdout();
    let mut entered_alternate_screen = false;
    let mut keyboard_enhancements_enabled = false;
    let mut mouse_capture_enabled = false;

    let setup_result = (|| -> Result<()> {
        crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
            .context("failed to enter alternate screen before launching TUI")?;
        entered_alternate_screen = true;

        if crossterm::execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )
        .is_ok()
        {
            keyboard_enhancements_enabled = true;
        }

        crossterm::execute!(stdout, EnableMouseCapture)
            .context("failed to enable mouse capture before launching TUI")?;
        mouse_capture_enabled = true;
        Ok(())
    })();

    if let Err(err) = setup_result {
        if mouse_capture_enabled {
            let _ = crossterm::execute!(stdout, DisableMouseCapture);
        }
        if keyboard_enhancements_enabled {
            let _ = crossterm::execute!(stdout, PopKeyboardEnhancementFlags);
        }
        if entered_alternate_screen {
            let _ = crossterm::execute!(stdout, crossterm::terminal::LeaveAlternateScreen);
        }
        let _ = crossterm::terminal::disable_raw_mode();
        return Err(err);
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = (|| -> Result<()> {
        loop {
            if let Some(update_rx) = live_updates.as_ref() {
                drain_live_updates(&mut app, update_rx);
            }

            if app.take_reload_requested() {
                if let Some(run_dir) = app.session_path.clone() {
                    match load_events_from_run_dir(&run_dir) {
                        Ok(events) => {
                            app.replace_events(events);
                            app.set_status_banner(None);
                        }
                        Err(err) => {
                            app.set_status_banner(Some(format!("reload failed: {err}")));
                        }
                    }
                } else {
                    app.set_status_banner(Some(
                        "reload requested but no session path is set".to_string(),
                    ));
                }
            }

            terminal.draw(|frame| ui::render_app(frame, &app))?;

            if app.should_quit {
                break;
            }

            if let Some(event) = poll(Duration::from_millis(100))? {
                match event {
                    event::TuiEvent::Key(key) => app.handle_key(key),
                    event::TuiEvent::Mouse(mouse) => {
                        let size = terminal.size()?;
                        let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                        let hovered_wheel_target =
                            ui::hovered_wheel_target(&app, frame_area, mouse.column, mouse.row);
                        app.handle_mouse(mouse, hovered_wheel_target);
                    }
                    event::TuiEvent::Resize(_, _) => {}
                }
            }
        }
        Ok(())
    })();

    crossterm::terminal::disable_raw_mode()
        .context("failed to disable terminal raw mode after TUI")?;
    if keyboard_enhancements_enabled {
        crossterm::execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            PopKeyboardEnhancementFlags,
            crossterm::terminal::LeaveAlternateScreen
        )
        .context("failed to leave alternate screen after TUI")?;
    } else {
        crossterm::execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        )
        .context("failed to leave alternate screen after TUI")?;
    }

    run_result
}

pub fn run_tui() -> Result<()> {
    let (_tx, rx) = mpsc::channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: PathBuf::from("."),
            historical_events: Vec::new(),
            update_rx: rx,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
    })
}

pub fn load_events_from_run_dir(run_dir: &Path) -> Result<Vec<EventEnvelopeV1>> {
    load_events_from_path(&run_dir.join("events.jsonl"))
}

fn load_events_from_path(path: &Path) -> Result<Vec<EventEnvelopeV1>> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("failed to read events file {}", path.display()))?;

    body.lines()
        .map(|line| {
            serde_json::from_str::<EventEnvelopeV1>(line)
                .with_context(|| format!("failed to parse JSONL event from {}", path.display()))
        })
        .collect()
}

fn drain_live_updates(app: &mut AppState, update_rx: &Receiver<LiveUpdate>) {
    loop {
        match update_rx.try_recv() {
            Ok(LiveUpdate::Event(event)) => {
                if app
                    .status_banner
                    .as_deref()
                    .is_some_and(transient_live_status_banner)
                {
                    app.set_status_banner(None);
                }
                app.ingest_event(*event)
            }
            Ok(LiveUpdate::Status(status)) => app.set_status_banner(Some(status)),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                app.set_status_banner(Some("live event stream disconnected".to_string()));
                break;
            }
        }
    }
}

fn transient_live_status_banner(status: &str) -> bool {
    let lower = status.to_ascii_lowercase();
    lower.contains("lagged") || lower.contains("replaying")
}
