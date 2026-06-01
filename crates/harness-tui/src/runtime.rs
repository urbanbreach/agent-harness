use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use harness_core::event::EventEnvelopeV1;
use ratatui::buffer::Buffer;
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{
    set_pending_live_prompt_draft, AppState, LaunchMetadata, SessionHistoryEntry, ToastVariant,
    TogglesConfig, UiIntent,
};
use crate::event::{self, poll};
use crate::event_log;
use crate::ui;

const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const LIVE_UPDATE_DRAIN_MAX_PER_FRAME: usize = 16;
const LIVE_UPDATE_DRAIN_MAX_DURATION: Duration = Duration::from_millis(8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LiveUpdateDrainState {
    changed: bool,
    disconnected: bool,
    budget_exhausted: bool,
}

#[derive(Clone, Debug, Default)]
struct PreservedTerminalSession {
    active: bool,
    keyboard_enhancements_enabled: bool,
    mouse_capture_enabled: bool,
    bracketed_paste_enabled: bool,
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
    SessionHistory(Vec<SessionHistoryEntry>),
    ContinueSession {
        run_id: String,
        run_dir: PathBuf,
        prompt_draft: String,
    },
    OperatorNotice {
        message: String,
        level: OperatorNoticeLevel,
    },
    AuthBackendResult {
        success: bool,
    },
    AuthProviderCatalogRefreshed {
        launch_metadata: Box<LaunchMetadata>,
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
        prompt_history_path: Option<PathBuf>,
        onboarding_required: bool,
        update_rx: Receiver<LiveUpdate>,
    },
    Replay {
        run_dir: PathBuf,
        events: Vec<EventEnvelopeV1>,
    },
    Live {
        run_dir: PathBuf,
        historical_events: Vec<EventEnvelopeV1>,
        session_history_entries: Vec<SessionHistoryEntry>,
        prompt_history_path: Option<PathBuf>,
        update_rx: Receiver<LiveUpdate>,
        compact_session_supported: bool,
    },
}

pub struct TuiOptions {
    pub mode: TuiMode,
    pub exit_on_finish: bool,
    pub on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    pub keybindings: Option<std::collections::BTreeMap<String, String>>,
    pub toggles: Option<TogglesConfig>,
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
        toggles,
        preserve_terminal_on_exit,
    } = options;

    let (mut app, mut live_updates) = match mode {
        TuiMode::Startup {
            session_history_entries,
            prompt_history_path,
            onboarding_required,
            update_rx,
        } => {
            let mut app = AppState::new_startup_with_prompt_history_path(
                session_history_entries,
                on_ui_intent,
                prompt_history_path,
            );
            app.set_onboarding_required(onboarding_required);
            app.should_quit = exit_on_finish;
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            (app, Some(update_rx))
        }
        TuiMode::Replay { run_dir, events } => {
            let mut app = AppState::new_replay(run_dir, events);
            if let Some(on_ui_intent) = on_ui_intent {
                app.enable_replay_navigation_handoff(on_ui_intent);
            }
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
            session_history_entries,
            prompt_history_path,
            update_rx,
            compact_session_supported,
        } => {
            let mut app = AppState::new_live_with_session_history_and_prompt_history_path(
                Some(run_dir),
                exit_on_finish,
                on_ui_intent,
                session_history_entries,
                prompt_history_path,
            );
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

    if let Some(toggles) = toggles {
        app.set_toggles_config(toggles);
    }

    let preserved_terminal = recover_mutex_lock(preserved_terminal_session()).clone();
    let reusing_terminal = preserved_terminal.active;
    let mut keyboard_enhancements_enabled = preserved_terminal.keyboard_enhancements_enabled;
    let mut mouse_capture_enabled = preserved_terminal.mouse_capture_enabled;
    let mut bracketed_paste_enabled = preserved_terminal.bracketed_paste_enabled;

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

            crossterm::execute!(stdout, EnableBracketedPaste)
                .context("failed to enable bracketed paste before launching TUI")?;
            bracketed_paste_enabled = true;

            crossterm::execute!(stdout, EnableMouseCapture)
                .context("failed to enable mouse capture before launching TUI")?;
            mouse_capture_enabled = true;
            Ok(())
        })();

        if let Err(err) = setup_result {
            if mouse_capture_enabled {
                let _ = crossterm::execute!(stdout, DisableMouseCapture);
            }
            if bracketed_paste_enabled {
                let _ = crossterm::execute!(stdout, DisableBracketedPaste);
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
        let mut next_animation_tick = Instant::now() + ACTIVE_POLL_INTERVAL;

        loop {
            let mut live_updates_pending = false;
            if let Some(update_rx) = live_updates.as_ref() {
                let drain_state = drain_live_updates(&mut app, update_rx);
                redraw_requested |= drain_state.changed;
                if drain_state.disconnected {
                    live_updates = None;
                }
                if drain_state.budget_exhausted {
                    live_updates_pending = true;
                }
            }

            if app.take_reload_requested() {
                if let Some(run_dir) = app.session_path.clone() {
                    match event_log::load_events_from_run_dir(&run_dir) {
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
            if animation_active {
                let now = Instant::now();
                if now >= next_animation_tick {
                    app.advance_transcript_animation_phase();
                    next_animation_tick = next_animation_tick_after_frame(now);
                    redraw_requested = true;
                    continue;
                }
            } else {
                next_animation_tick = next_animation_tick_after_frame(Instant::now());
            }

            let event = if live_updates_pending {
                poll(Duration::ZERO)?
            } else {
                poll(poll_timeout(
                    animation_active,
                    live_updates.is_some(),
                    Instant::now(),
                    next_animation_tick,
                ))?
            };

            if event.is_none() && animation_active && Instant::now() >= next_animation_tick {
                app.advance_transcript_animation_phase();
                next_animation_tick = next_animation_tick_after_frame(Instant::now());
                redraw_requested = true;
                continue;
            }

            if let Some(event) = event {
                let event_changed = match event {
                    event::TuiEvent::Key(key) => {
                        let size = terminal.size()?;
                        let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                        app.set_frame_area(frame_area);
                        app.handle_key(key);
                        true
                    }
                    event::TuiEvent::Paste(text) => {
                        let size = terminal.size()?;
                        let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                        app.set_frame_area(frame_area);
                        app.handle_paste(&text);
                        true
                    }
                    event::TuiEvent::Mouse(mouse) => {
                        if !mouse_event_requires_handling(mouse.kind, app.slash_visible) {
                            continue;
                        }

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
                        )
                    }
                    event::TuiEvent::Resize(_, _) => true,
                };
                if event_changed {
                    redraw_requested = true;
                }
            }
        }
        Ok(())
    })();

    if run_result.is_ok() && preserve_terminal_on_exit {
        *recover_mutex_lock(preserved_terminal_session()) = PreservedTerminalSession {
            active: true,
            keyboard_enhancements_enabled,
            mouse_capture_enabled,
            bracketed_paste_enabled,
            buffer: Some(terminal.current_buffer_mut().clone()),
        };
        return run_result;
    }

    *recover_mutex_lock(preserved_terminal_session()) = PreservedTerminalSession::default();
    teardown_terminal_session(
        terminal.backend_mut(),
        keyboard_enhancements_enabled,
        mouse_capture_enabled,
        bracketed_paste_enabled,
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
        preserved.bracketed_paste_enabled,
    )
}

fn teardown_terminal_session(
    writer: &mut impl std::io::Write,
    keyboard_enhancements_enabled: bool,
    mouse_capture_enabled: bool,
    bracketed_paste_enabled: bool,
) -> Result<()> {
    crossterm::terminal::disable_raw_mode()
        .context("failed to disable terminal raw mode after TUI")?;

    if mouse_capture_enabled {
        crossterm::execute!(writer, DisableMouseCapture)
            .context("failed to disable mouse capture after TUI")?;
    }
    if bracketed_paste_enabled {
        crossterm::execute!(writer, DisableBracketedPaste)
            .context("failed to disable bracketed paste after TUI")?;
    }
    if keyboard_enhancements_enabled {
        crossterm::execute!(writer, PopKeyboardEnhancementFlags)
            .context("failed to pop keyboard enhancement flags after TUI")?;
    }
    crossterm::execute!(writer, crossterm::terminal::LeaveAlternateScreen)
        .context("failed to leave alternate screen after TUI")
}

pub fn run_tui() -> Result<()> {
    let (_tx, rx) = mpsc::channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: PathBuf::from("."),
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx: rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    })
}

fn poll_timeout(
    animation_active: bool,
    live_updates_connected: bool,
    now: Instant,
    next_animation_tick: Instant,
) -> Duration {
    if animation_active {
        return next_animation_tick.saturating_duration_since(now);
    }

    if live_updates_connected {
        ACTIVE_POLL_INTERVAL
    } else {
        IDLE_POLL_INTERVAL
    }
}

fn next_animation_tick_after_frame(frame_completed_at: Instant) -> Instant {
    // Animation cadence must be independent from redraw cadence. Mouse movement can request
    // redraws continuously, so only animation advancement should move this deadline forward.
    frame_completed_at + ACTIVE_POLL_INTERVAL
}

fn mouse_event_requires_handling(_kind: MouseEventKind, _slash_visible: bool) -> bool {
    true
}

fn drain_live_updates(
    app: &mut AppState,
    update_rx: &Receiver<LiveUpdate>,
) -> LiveUpdateDrainState {
    let mut state = LiveUpdateDrainState::default();

    let mut drained = 0usize;
    let drain_started_at = Instant::now();

    loop {
        if drained >= LIVE_UPDATE_DRAIN_MAX_PER_FRAME
            || (drained > 0 && drain_started_at.elapsed() >= LIVE_UPDATE_DRAIN_MAX_DURATION)
        {
            state.budget_exhausted = true;
            break;
        }

        match update_rx.try_recv() {
            Ok(LiveUpdate::Event(event)) => {
                drained += 1;
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
                drained += 1;
                if app.status_banner.as_deref() != Some(status.as_str()) {
                    app.set_status_banner(Some(status));
                    state.changed = true;
                }
            }
            Ok(LiveUpdate::SessionHistory(entries)) => {
                drained += 1;
                app.set_session_history_entries(entries);
                state.changed = true;
            }
            Ok(LiveUpdate::ContinueSession {
                run_id,
                run_dir,
                prompt_draft,
            }) => {
                drained += 1;
                set_pending_live_prompt_draft(Some(prompt_draft));
                app.emit_ui_intent(UiIntent::ContinueSession { run_id, run_dir });
                app.should_quit = true;
                state.changed = true;
            }
            Ok(LiveUpdate::OperatorNotice { message, level }) => {
                drained += 1;
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
            Ok(LiveUpdate::AuthBackendResult { success }) => {
                drained += 1;
                app.apply_auth_backend_result(success);
                state.changed = true;
            }
            Ok(LiveUpdate::AuthProviderCatalogRefreshed { launch_metadata }) => {
                drained += 1;
                app.apply_auth_provider_catalog_refresh(*launch_metadata);
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
    use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

    #[test]
    fn poll_timeout_blocks_when_idle_and_live_updates_are_gone() {
        let now = Instant::now();
        assert_eq!(
            poll_timeout(false, false, now, now + ACTIVE_POLL_INTERVAL),
            IDLE_POLL_INTERVAL
        );
        assert_eq!(
            poll_timeout(false, true, now, now + ACTIVE_POLL_INTERVAL),
            ACTIVE_POLL_INTERVAL
        );
    }

    #[test]
    fn poll_timeout_tracks_next_animation_tick() {
        let now = Instant::now();
        let next_tick = now + Duration::from_millis(42);

        assert_eq!(
            poll_timeout(true, false, now, next_tick),
            Duration::from_millis(42)
        );
        assert_eq!(
            poll_timeout(true, true, next_tick, next_tick),
            Duration::ZERO
        );
    }

    #[test]
    fn next_animation_tick_after_slow_frame_remains_throttled() {
        let started_at = Instant::now();
        let slow_frame_completed_at = started_at + ACTIVE_POLL_INTERVAL * 3;
        let next_tick = next_animation_tick_after_frame(slow_frame_completed_at);

        assert_eq!(
            poll_timeout(true, false, slow_frame_completed_at, next_tick),
            ACTIVE_POLL_INTERVAL
        );
    }

    #[test]
    fn plain_mouse_movement_reaches_hover_handling() {
        assert!(mouse_event_requires_handling(MouseEventKind::Moved, false));
        assert!(mouse_event_requires_handling(MouseEventKind::Moved, true));
        assert!(mouse_event_requires_handling(
            MouseEventKind::ScrollDown,
            false
        ));
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
                budget_exhausted: false,
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
                budget_exhausted: false,
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
                budget_exhausted: false,
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
                budget_exhausted: false,
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

    #[test]
    fn drain_live_updates_applies_session_history_refresh() {
        let entry = SessionHistoryEntry {
            run_dir: PathBuf::from("/tmp/session-history-refresh"),
            catalog: SessionCatalogEntry {
                run_id: "session-history-refresh".to_string(),
                run_name: Some("session history refresh".to_string()),
                status: Some(RunStatus::Finished),
                last_updated_at: Some("2026-05-04T00:00:00Z".to_string()),
                workspace_root: Some("/workspace".to_string()),
                profile_preset: Some("worker".to_string()),
                provider_model: Some("mock:model".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: true,
                resume_disabled_reason: None,
                artifact_count: 0,
                child_session_count: 0,
                parent_session_id: None,
            },
        };
        let (tx, rx) = mpsc::channel();
        tx.send(LiveUpdate::SessionHistory(vec![entry.clone()]))
            .expect("send session history");

        let mut app = AppState::default();
        let state = drain_live_updates(&mut app, &rx);

        assert_eq!(
            state,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
                budget_exhausted: false,
            }
        );
        assert_eq!(app.session_history_entries, vec![entry]);
    }

    #[test]
    fn drain_live_updates_applies_auth_backend_result_to_onboarding() {
        let (tx, rx) = mpsc::channel();
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_onboarding_step_for_test(crate::app::OnboardingStep::CodexDevice);
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        tx.send(LiveUpdate::AuthBackendResult { success: true })
            .expect("send auth result");

        let state = drain_live_updates(&mut app, &rx);

        assert_eq!(
            state,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
                budget_exhausted: false,
            }
        );
        assert_eq!(
            app.onboarding_screen().expect("success screen").step,
            crate::app::OnboardingStep::LoginSuccess
        );
    }

    #[test]
    fn drain_live_updates_yields_after_frame_budget() {
        let (tx, rx) = mpsc::channel();
        for index in 0..=LIVE_UPDATE_DRAIN_MAX_PER_FRAME {
            tx.send(LiveUpdate::Status(format!("status {index}")))
                .expect("send status update");
        }

        let mut app = AppState::default();
        let first = drain_live_updates(&mut app, &rx);

        assert_eq!(
            first,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
                budget_exhausted: true,
            }
        );
        assert_eq!(
            app.status_banner.as_deref(),
            Some(format!("status {}", LIVE_UPDATE_DRAIN_MAX_PER_FRAME - 1).as_str())
        );

        let second = drain_live_updates(&mut app, &rx);
        assert_eq!(
            second,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
                budget_exhausted: false,
            }
        );
        assert_eq!(
            app.status_banner.as_deref(),
            Some(format!("status {LIVE_UPDATE_DRAIN_MAX_PER_FRAME}").as_str())
        );
    }
}
