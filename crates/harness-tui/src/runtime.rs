use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, MouseButton, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use harness_core::event::EventEnvelopeV1;
use ratatui::buffer::Buffer;
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{AppState, LaunchMetadata, SessionHistoryEntry, ToastVariant, UiIntent};
use crate::event::{self, poll};
use crate::ui;

const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LiveUpdateDrainState {
    changed: bool,
    disconnected: bool,
}

#[derive(Clone, Debug, Default)]
struct PreservedTerminalSession {
    active: bool,
    keyboard_enhancements_enabled: bool,
    mouse_capture_enabled: bool,
    buffer: Option<Buffer>,
}

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

fn preserved_terminal_session() -> &'static Mutex<PreservedTerminalSession> {
    static PRESERVED: OnceLock<Mutex<PreservedTerminalSession>> = OnceLock::new();
    PRESERVED.get_or_init(|| Mutex::new(PreservedTerminalSession::default()))
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
    OperatorNotice {
        message: String,
        level: OperatorNoticeLevel,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatorNoticeLevel {
    Info,
    Error,
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
        compact_session_supported: bool,
    },
}

pub struct TuiOptions {
    pub mode: TuiMode,
    pub exit_on_finish: bool,
    pub on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    pub keybindings: Option<std::collections::BTreeMap<String, String>>,
    pub preserve_terminal_on_exit: bool,
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
        preserve_terminal_on_exit,
    } = options;

    let (mut app, mut live_updates) = match mode {
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
            compact_session_supported,
        } => {
            let mut app = AppState::new_live(Some(run_dir), exit_on_finish, on_ui_intent);
            app.set_compact_session_supported(compact_session_supported);
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            for event in historical_events {
                app.ingest_historical_event(event);
            }
            (app, Some(update_rx))
        }
    };

    let preserved_terminal = recover_mutex_lock(preserved_terminal_session()).clone();
    let reusing_terminal = preserved_terminal.active;
    let mut keyboard_enhancements_enabled = preserved_terminal.keyboard_enhancements_enabled;
    let mut mouse_capture_enabled = preserved_terminal.mouse_capture_enabled;

    if !reusing_terminal {
        crossterm::terminal::enable_raw_mode().context("failed to enable terminal raw mode")?;
    }
    let mut stdout = std::io::stdout();
    if !reusing_terminal {
        let setup_result = (|| -> Result<()> {
            crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
                .context("failed to enter alternate screen before launching TUI")?;

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
            let _ = crossterm::execute!(stdout, crossterm::terminal::LeaveAlternateScreen);
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(err);
        }
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    if let Some(buffer) = preserved_terminal.buffer.as_ref() {
        if terminal.current_buffer_mut().area == buffer.area {
            *terminal.current_buffer_mut() = buffer.clone();
        }
    }

    let run_result = (|| -> Result<()> {
        let mut redraw_requested = true;

        loop {
            if let Some(update_rx) = live_updates.as_ref() {
                let drain_state = drain_live_updates(&mut app, update_rx);
                redraw_requested |= drain_state.changed;
                if drain_state.disconnected {
                    live_updates = None;
                }
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
                redraw_requested = true;
            }

            if redraw_requested {
                let size = terminal.size()?;
                let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                app.set_frame_area(frame_area);
                terminal.draw(|frame| ui::render_app(frame, &app))?;
                redraw_requested = false;
            }

            if app.should_quit {
                break;
            }

            let animation_active = app.has_active_animations();
            let event = poll(poll_timeout(animation_active, live_updates.is_some()))?;

            if event.is_none() && animation_active {
                app.advance_transcript_animation_phase();
                redraw_requested = true;
                continue;
            }

            if let Some(event) = event {
                match event {
                    event::TuiEvent::Key(key) => {
                        let size = terminal.size()?;
                        let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                        app.set_frame_area(frame_area);
                        app.handle_key(key)
                    }
                    event::TuiEvent::Mouse(mouse) => {
                        let size = terminal.size()?;
                        let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                        app.set_frame_area(frame_area);
                        let (
                            hovered_wheel_target,
                            clicked_operator_sidebar_section,
                            transcript_scrollbar_hit,
                        ) = match mouse.kind {
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => (
                                ui::hovered_wheel_target(&app, frame_area, mouse.column, mouse.row),
                                None,
                                None,
                            ),
                            MouseEventKind::Down(MouseButton::Left) => (
                                None,
                                ui::operator_sidebar_section_hit_target(
                                    &app,
                                    frame_area,
                                    mouse.column,
                                    mouse.row,
                                ),
                                ui::transcript_scrollbar_hit(
                                    &app,
                                    frame_area,
                                    mouse.column,
                                    mouse.row,
                                ),
                            ),
                            _ => (None, None, None),
                        };
                        app.handle_mouse(
                            mouse,
                            frame_area,
                            hovered_wheel_target,
                            clicked_operator_sidebar_section,
                            transcript_scrollbar_hit,
                        );
                    }
                    event::TuiEvent::Resize(_, _) => {}
                }

                redraw_requested = true;
            }
        }
        Ok(())
    })();

    if run_result.is_ok() && preserve_terminal_on_exit {
        *recover_mutex_lock(preserved_terminal_session()) = PreservedTerminalSession {
            active: true,
            keyboard_enhancements_enabled,
            mouse_capture_enabled,
            buffer: Some(terminal.current_buffer_mut().clone()),
        };
        return run_result;
    }

    *recover_mutex_lock(preserved_terminal_session()) = PreservedTerminalSession::default();
    teardown_terminal_session(
        terminal.backend_mut(),
        keyboard_enhancements_enabled,
        mouse_capture_enabled,
    )?;

    run_result
}

pub fn close_preserved_terminal_session() -> Result<()> {
    let preserved = std::mem::take(&mut *recover_mutex_lock(preserved_terminal_session()));
    if !preserved.active {
        return Ok(());
    }

    // This handoff is intentionally process-global and stdout-backed: the startup launcher
    // preserves the active terminal long enough for the next TUI invocation in the same process
    // to reuse it, and the interactive workflow closes it after the handoff completes or fails.
    let mut stdout = std::io::stdout();
    teardown_terminal_session(
        &mut stdout,
        preserved.keyboard_enhancements_enabled,
        preserved.mouse_capture_enabled,
    )
}

fn teardown_terminal_session(
    writer: &mut impl std::io::Write,
    keyboard_enhancements_enabled: bool,
    mouse_capture_enabled: bool,
) -> Result<()> {
    crossterm::terminal::disable_raw_mode()
        .context("failed to disable terminal raw mode after TUI")?;

    match (mouse_capture_enabled, keyboard_enhancements_enabled) {
        (true, true) => crossterm::execute!(
            writer,
            DisableMouseCapture,
            PopKeyboardEnhancementFlags,
            crossterm::terminal::LeaveAlternateScreen
        )
        .context("failed to leave alternate screen after TUI"),
        (true, false) => crossterm::execute!(
            writer,
            DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        )
        .context("failed to leave alternate screen after TUI"),
        (false, true) => crossterm::execute!(
            writer,
            PopKeyboardEnhancementFlags,
            crossterm::terminal::LeaveAlternateScreen
        )
        .context("failed to leave alternate screen after TUI"),
        (false, false) => crossterm::execute!(writer, crossterm::terminal::LeaveAlternateScreen)
            .context("failed to leave alternate screen after TUI"),
    }
}

pub fn run_tui() -> Result<()> {
    let (_tx, rx) = mpsc::channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: PathBuf::from("."),
            historical_events: Vec::new(),
            update_rx: rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        preserve_terminal_on_exit: false,
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

fn poll_timeout(animation_active: bool, live_updates_connected: bool) -> Duration {
    if animation_active || live_updates_connected {
        ACTIVE_POLL_INTERVAL
    } else {
        IDLE_POLL_INTERVAL
    }
}

fn drain_live_updates(
    app: &mut AppState,
    update_rx: &Receiver<LiveUpdate>,
) -> LiveUpdateDrainState {
    let mut state = LiveUpdateDrainState::default();

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
                app.ingest_event(*event);
                state.changed = true;
            }
            Ok(LiveUpdate::Status(status)) => {
                if app.status_banner.as_deref() != Some(status.as_str()) {
                    app.set_status_banner(Some(status));
                    state.changed = true;
                }
            }
            Ok(LiveUpdate::OperatorNotice { message, level }) => {
                if matches!(level, OperatorNoticeLevel::Error)
                    && app.status_banner.as_deref() != Some(message.as_str())
                {
                    app.set_status_banner(Some(message.clone()));
                }
                app.show_toast(
                    message,
                    match level {
                        OperatorNoticeLevel::Info => ToastVariant::Info,
                        OperatorNoticeLevel::Error => ToastVariant::Error,
                    },
                );
                state.changed = true;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                let disconnected_message = "live event stream disconnected";
                if app.status_banner.as_deref() != Some(disconnected_message) {
                    app.set_status_banner(Some(disconnected_message.to_string()));
                    state.changed = true;
                }
                state.disconnected = true;
                break;
            }
        }
    }

    state
}

fn transient_live_status_banner(status: &str) -> bool {
    let lower = status.to_ascii_lowercase();
    lower.contains("lagged") || lower.contains("replaying")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, ToastVariant};

    #[test]
    fn poll_timeout_blocks_when_idle_and_live_updates_are_gone() {
        assert_eq!(poll_timeout(false, false), IDLE_POLL_INTERVAL);
        assert_eq!(poll_timeout(true, false), ACTIVE_POLL_INTERVAL);
        assert_eq!(poll_timeout(false, true), ACTIVE_POLL_INTERVAL);
    }

    #[test]
    fn drain_live_updates_marks_disconnect_once() {
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let mut app = AppState::default();

        let first = drain_live_updates(&mut app, &rx);
        assert_eq!(
            first,
            LiveUpdateDrainState {
                changed: true,
                disconnected: true,
            }
        );
        assert_eq!(
            app.status_banner.as_deref(),
            Some("live event stream disconnected")
        );

        let second = drain_live_updates(&mut app, &rx);
        assert_eq!(
            second,
            LiveUpdateDrainState {
                changed: false,
                disconnected: true,
            }
        );
    }

    #[test]
    fn app_toast_counts_as_active_animation() {
        let mut app = AppState::default();
        assert!(!app.has_active_animations());

        app.set_toast_for_test("Copied", ToastVariant::Info);

        assert!(app.has_active_animations());
    }

    #[test]
    fn drain_live_updates_routes_operator_notice_to_toast() {
        let (tx, rx) = mpsc::channel();
        tx.send(LiveUpdate::OperatorNotice {
            message: "manual compaction skipped: need at least two completed turns".to_string(),
            level: OperatorNoticeLevel::Info,
        })
        .expect("send operator notice");

        let mut app = AppState::default();
        let state = drain_live_updates(&mut app, &rx);

        assert_eq!(
            state,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
            }
        );
        assert_eq!(app.status_banner.as_deref(), None);
        assert_eq!(
            app.toast()
                .map(|toast| (toast.message.as_str(), toast.variant)),
            Some((
                "manual compaction skipped: need at least two completed turns",
                ToastVariant::Info,
            ))
        );
    }

    #[test]
    fn drain_live_updates_keeps_error_operator_notice_persistent() {
        let (tx, rx) = mpsc::channel();
        tx.send(LiveUpdate::OperatorNotice {
            message: "manual compaction failed: boom".to_string(),
            level: OperatorNoticeLevel::Error,
        })
        .expect("send error operator notice");

        let mut app = AppState::default();
        let state = drain_live_updates(&mut app, &rx);

        assert_eq!(
            state,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
            }
        );
        assert_eq!(
            app.status_banner.as_deref(),
            Some("manual compaction failed: boom")
        );
        assert_eq!(
            app.toast()
                .map(|toast| (toast.message.as_str(), toast.variant)),
            Some(("manual compaction failed: boom", ToastVariant::Error))
        );
    }
}
