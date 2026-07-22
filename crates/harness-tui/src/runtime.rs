// allow: SIZE_OK — TUI runtime loop (poll interval + event dispatch + terminal resize + shutdown handling)
use crate::UnwrapOrAbort;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::cursor::MoveTo;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, MouseButton, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate};
use harness_core::event::EventEnvelopeV1;
use ratatui::buffer::Buffer;
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{
    set_pending_live_prompt_draft, AppState, LaunchMetadata, SessionHistoryEntry, ToastVariant,
    TogglesConfig, UiIntent,
};
use crate::event::{self, poll};
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

/// Explicit model of terminal features the TUI may enable or rely on.
///
/// Interactive I/O features (keyboard enhancement, paste, mouse, alt-screen) are
/// enabled only when setup succeeds. Static probes (truecolor, OSC52 suitability)
/// come from the environment and never force setup. Restore undoes only what was
/// successfully enabled and always attempts fail-safe raw-mode/alt-screen exit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalCapabilityState {
    pub keyboard_enhancement: bool,
    pub truecolor: bool,
    pub bracketed_paste: bool,
    pub mouse_capture: bool,
    pub osc52_clipboard: bool,
    pub alternate_screen: bool,
    pub focus_reporting: bool,
}

/// Ordered restore steps derived from capability state (pure; no I/O).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalTeardownPlan {
    pub disable_raw_mode: bool,
    pub disable_mouse_capture: bool,
    pub disable_bracketed_paste: bool,
    pub disable_focus_change: bool,
    pub pop_keyboard_enhancement: bool,
    pub leave_alternate_screen: bool,
}

impl TerminalCapabilityState {
    pub const fn absent() -> Self {
        Self {
            keyboard_enhancement: false,
            truecolor: false,
            bracketed_paste: false,
            mouse_capture: false,
            osc52_clipboard: false,
            alternate_screen: false,
            focus_reporting: false,
        }
    }

    /// Full capability-present fixture for tests (not used as a live default).
    pub const fn present() -> Self {
        Self {
            keyboard_enhancement: true,
            truecolor: true,
            bracketed_paste: true,
            mouse_capture: true,
            osc52_clipboard: true,
            alternate_screen: true,
            focus_reporting: true,
        }
    }

    /// Static environment probes that do not require terminal I/O setup.
    pub fn from_environment() -> Self {
        let mut caps = Self::absent();
        caps.truecolor = truecolor_from_colorterm(std::env::var("COLORTERM").ok().as_deref());
        caps.osc52_clipboard = std::io::IsTerminal::is_terminal(&std::io::stdout());
        caps
    }

    /// Fail-safe restore plan: always leave alt-screen/raw mode; reverse only
    /// optional features that were successfully enabled during setup.
    pub const fn teardown_plan(self) -> TerminalTeardownPlan {
        TerminalTeardownPlan {
            disable_raw_mode: true,
            disable_mouse_capture: self.mouse_capture,
            disable_bracketed_paste: self.bracketed_paste,
            disable_focus_change: self.focus_reporting,
            pop_keyboard_enhancement: self.keyboard_enhancement,
            leave_alternate_screen: true,
        }
    }
}

fn truecolor_from_colorterm(value: Option<&str>) -> bool {
    value
        .map(str::to_ascii_lowercase)
        .is_some_and(|lower| lower.contains("truecolor") || lower.contains("24bit"))
}

/// Merge static env probes with interactive setup success flags.
fn apply_interactive_setup_results(
    base: TerminalCapabilityState,
    keyboard_ok: bool,
    paste_ok: bool,
    mouse_ok: bool,
    alt_screen_ok: bool,
    focus_ok: bool,
) -> TerminalCapabilityState {
    TerminalCapabilityState {
        keyboard_enhancement: keyboard_ok,
        truecolor: base.truecolor,
        bracketed_paste: paste_ok,
        mouse_capture: mouse_ok,
        osc52_clipboard: base.osc52_clipboard,
        alternate_screen: alt_screen_ok,
        focus_reporting: focus_ok,
    }
}

#[derive(Clone, Debug, Default)]
struct PreservedTerminalSession {
    active: bool,
    capabilities: TerminalCapabilityState,
    buffer: Option<Buffer>,
}

fn recover_mutex_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) struct TerminalRestoreGuard {
    capabilities: TerminalCapabilityState,
    restored: bool,
}

impl TerminalRestoreGuard {
    pub(crate) fn new(capabilities: TerminalCapabilityState) -> Self {
        Self {
            capabilities,
            restored: false,
        }
    }

    pub(crate) fn mark_restored(&mut self) {
        self.restored = true;
    }

    #[cfg(test)]
    pub(crate) fn restored(&self) -> bool {
        self.restored
    }

    #[cfg(test)]
    pub(crate) fn capabilities(&self) -> TerminalCapabilityState {
        self.capabilities
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        let mut stdout = std::io::stdout();
        let _ = teardown_terminal_session(&mut stdout, self.capabilities);
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
            update_rx,
        } => {
            let mut app = AppState::new_startup_with_prompt_history_path(
                session_history_entries,
                on_ui_intent,
                prompt_history_path,
            );
            app.should_quit = exit_on_finish;
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            (app, Some(update_rx))
        }
        TuiMode::Replay { run_dir, events } => {
            let mut app = AppState::new_replay(run_dir, events);
            // Replay workspace authority comes exclusively from replayed RunStarted events.
            // The CWD-based workspace root provider must never substitute missing event authority.
            app.disable_cwd_workspace_root_provider();
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
            let crash_report = harness_core::crash_recovery::inspect_previous_crash(&run_dir);
            let mut app = AppState::new_live_with_session_history_and_prompt_history_path(
                Some(run_dir.clone()),
                exit_on_finish,
                on_ui_intent,
                session_history_entries,
                prompt_history_path,
            );
            app.set_compact_session_supported(compact_session_supported);
            // Shared pending slot (also used by Replay) packs freeze-aligned context window
            // for PTY/reference helpers without expanding TuiMode.
            if let Some(launch_metadata) = take_pending_replay_launch_metadata() {
                app.set_launch_metadata(launch_metadata);
            }
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            for event in historical_events {
                app.ingest_historical_event(event);
            }
            if let Some(message) = crash_report.recovery_message {
                let banner = match crash_report.recovery_action {
                    Some(action) => {
                        let run_id = run_dir
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("session");
                        format!("{message} Action: {}", action.operator_hint(run_id))
                    }
                    None => message,
                };
                app.set_status_banner(Some(banner));
            }
            (app, Some(update_rx))
        }
    };

    if let Some(toggles) = toggles {
        app.set_toggles_config(toggles);
    }

    app.maybe_set_no_provider_banner();

    let preserved_terminal = recover_mutex_lock(preserved_terminal_session()).clone();
    let reusing_terminal = preserved_terminal.active;
    let mut capabilities = if reusing_terminal {
        preserved_terminal.capabilities
    } else {
        TerminalCapabilityState::from_environment()
    };

    if !reusing_terminal {
        crossterm::terminal::enable_raw_mode().context("failed to enable terminal raw mode")?;
    }
    let mut stdout = std::io::stdout();
    if !reusing_terminal {
        let mut keyboard_ok = false;
        let mut paste_ok = false;
        let mut mouse_ok = false;
        let mut alt_screen_ok = false;
        let mut focus_ok = false;
        let setup_result = (|| -> Result<()> {
            crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
                .context("failed to enter alternate screen before launching TUI")?;
            alt_screen_ok = true;

            if crossterm::execute!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )
            .is_ok()
            {
                keyboard_ok = true;
            }

            crossterm::execute!(stdout, EnableBracketedPaste)
                .context("failed to enable bracketed paste before launching TUI")?;
            paste_ok = true;

            crossterm::execute!(stdout, EnableMouseCapture)
                .context("failed to enable mouse capture before launching TUI")?;
            mouse_ok = true;

            if crossterm::execute!(stdout, EnableFocusChange).is_ok() {
                focus_ok = true;
            }
            Ok(())
        })();

        capabilities = apply_interactive_setup_results(
            capabilities,
            keyboard_ok,
            paste_ok,
            mouse_ok,
            alt_screen_ok,
            focus_ok,
        );

        if let Err(err) = setup_result {
            let _ = teardown_terminal_session(&mut stdout, capabilities);
            return Err(err);
        }
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    if reusing_terminal {
        // Physical alternate-screen cells from the previous shell survive across
        // handoff. Best-effort clear: some PTY backends reject Clear(All) while
        // still accepting normal draws. Always reset ratatui buffers so undrawn
        // regions cannot ghost startup welcome text into the next shell.
        let _ = crossterm::execute!(terminal.backend_mut(), Clear(ClearType::All), MoveTo(0, 0));
        let size = terminal
            .size()
            .context("failed to read terminal size for handoff clear")?;
        let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
        *terminal.current_buffer_mut() = Buffer::empty(area);
    }

    let mut restore_guard = TerminalRestoreGuard::new(capabilities);

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

            if redraw_requested {
                let size = terminal.size()?;
                let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                app.set_frame_area(frame_area);
                crossterm::queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
                terminal.draw(|frame| ui::render_app(frame, &app))?;
                crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
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
            capabilities,
            buffer: None,
        };
        restore_guard.mark_restored();
        return run_result;
    }

    *recover_mutex_lock(preserved_terminal_session()) = PreservedTerminalSession::default();
    teardown_terminal_session(terminal.backend_mut(), capabilities)?;
    restore_guard.mark_restored();

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
    teardown_terminal_session(&mut stdout, preserved.capabilities)
}

fn teardown_terminal_session(
    writer: &mut impl std::io::Write,
    capabilities: TerminalCapabilityState,
) -> Result<()> {
    let plan = capabilities.teardown_plan();

    if plan.disable_raw_mode {
        crossterm::terminal::disable_raw_mode()
            .context("failed to disable terminal raw mode after TUI")?;
    }

    if plan.disable_mouse_capture {
        crossterm::execute!(writer, DisableMouseCapture)
            .context("failed to disable mouse capture after TUI")?;
    }
    if plan.disable_bracketed_paste {
        crossterm::execute!(writer, DisableBracketedPaste)
            .context("failed to disable bracketed paste after TUI")?;
    }
    if plan.disable_focus_change {
        crossterm::execute!(writer, DisableFocusChange)
            .context("failed to disable focus change reporting after TUI")?;
    }
    if plan.pop_keyboard_enhancement {
        crossterm::execute!(writer, PopKeyboardEnhancementFlags)
            .context("failed to pop keyboard enhancement flags after TUI")?;
    }
    if plan.leave_alternate_screen {
        crossterm::execute!(writer, crossterm::terminal::LeaveAlternateScreen)
            .context("failed to leave alternate screen after TUI")?;
    }
    Ok(())
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
    use crate::UnwrapOrAbort;
    use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

    #[test]
    fn poll_timeout_blocks_when_idle_and_live_updates_are_gone() {
        // arrange
        // act
        // assert
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
        // arrange
        // act
        // assert
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
        // arrange
        // act
        // assert
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
        // arrange
        // act
        // assert
        assert!(mouse_event_requires_handling(MouseEventKind::Moved, false));
        assert!(mouse_event_requires_handling(MouseEventKind::Moved, true));
        assert!(mouse_event_requires_handling(
            MouseEventKind::ScrollDown,
            false
        ));
    }

    #[test]
    fn drain_live_updates_marks_disconnect_once() {
        // arrange
        // act
        // assert
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
        // arrange
        // act
        // assert
        let mut app = AppState::default();
        assert!(!app.has_active_animations());

        app.set_toast_for_test("Copied", ToastVariant::Info);

        assert!(app.has_active_animations());
    }

    #[test]
    fn drain_live_updates_routes_operator_notice_to_toast() {
        // arrange
        // act
        // assert
        let (tx, rx) = mpsc::channel();
        tx.send(LiveUpdate::OperatorNotice {
            message: "manual compaction skipped: need at least two completed turns".to_string(),
            level: OperatorNoticeLevel::Info,
        })
        .unwrap_or_abort();

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
        // arrange
        // act
        // assert
        let (tx, rx) = mpsc::channel();
        tx.send(LiveUpdate::OperatorNotice {
            message: "manual compaction failed: boom".to_string(),
            level: OperatorNoticeLevel::Error,
        })
        .unwrap_or_abort();

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
        // arrange
        // act
        // assert
        let entry = SessionHistoryEntry {
            run_dir: PathBuf::from("/tmp/session-history-refresh"),
            catalog: SessionCatalogEntry {
                run_id: "session-history-refresh".into(),
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
            .unwrap_or_abort();

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
    fn drain_live_updates_applies_auth_backend_result_to_status_banner() {
        // arrange
        // act
        // assert
        let (tx, rx) = mpsc::channel();
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_status_banner(Some(
            "No provider connected. Run `harness auth login` in a terminal or use /connect to set up a provider."
                .to_string(),
        ));
        tx.send(LiveUpdate::AuthBackendResult { success: true })
            .unwrap_or_abort();

        let state = drain_live_updates(&mut app, &rx);

        assert_eq!(
            state,
            LiveUpdateDrainState {
                changed: true,
                disconnected: false,
                budget_exhausted: false,
            }
        );
        assert_eq!(app.status_banner, None);
    }

    #[test]
    fn drain_live_updates_yields_after_frame_budget() {
        // arrange
        // act
        // assert
        let (tx, rx) = mpsc::channel();
        for index in 0..=LIVE_UPDATE_DRAIN_MAX_PER_FRAME {
            tx.send(LiveUpdate::Status(format!("status {index}")))
                .unwrap_or_abort();
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

    #[test]
    fn terminal_restore_guard_marks_restored_on_normal_teardown() {
        // arrange
        let mut guard = TerminalRestoreGuard::new(TerminalCapabilityState::present());
        assert!(!guard.restored());
        // act
        guard.mark_restored();
        // assert
        assert!(guard.restored());
    }

    #[test]
    fn terminal_capability_state_absent_disables_all_features() {
        // arrange
        let caps = TerminalCapabilityState::absent();
        // assert
        assert_eq!(
            caps,
            TerminalCapabilityState {
                keyboard_enhancement: false,
                truecolor: false,
                bracketed_paste: false,
                mouse_capture: false,
                osc52_clipboard: false,
                alternate_screen: false,
                focus_reporting: false,
            }
        );
        // act
        let plan = caps.teardown_plan();
        // assert
        assert!(plan.disable_raw_mode);
        assert!(!plan.disable_mouse_capture);
        assert!(!plan.disable_bracketed_paste);
        assert!(!plan.disable_focus_change);
        assert!(!plan.pop_keyboard_enhancement);
        assert!(plan.leave_alternate_screen);
    }

    #[test]
    fn terminal_capability_state_present_enables_core_features() {
        // arrange — present path (kitty keyboard / truecolor / paste / mouse / OSC52)
        let present = TerminalCapabilityState::present();
        // assert present detection
        assert!(present.keyboard_enhancement);
        assert!(present.truecolor);
        assert!(present.bracketed_paste);
        assert!(present.mouse_capture);
        assert!(present.osc52_clipboard);
        assert!(present.alternate_screen);
        assert!(present.focus_reporting);

        // act — present teardown restores only features that were enabled
        let present_plan = present.teardown_plan();
        // assert present teardown
        assert!(present_plan.disable_raw_mode);
        assert!(present_plan.disable_mouse_capture);
        assert!(present_plan.disable_bracketed_paste);
        assert!(present_plan.disable_focus_change);
        assert!(present_plan.pop_keyboard_enhancement);
        assert!(present_plan.leave_alternate_screen);

        // arrange — absent fallback path (no enhanced capabilities)
        let absent = TerminalCapabilityState::absent();
        // assert absent detection (safe degradation)
        assert!(!absent.keyboard_enhancement);
        assert!(!absent.truecolor);
        assert!(!absent.bracketed_paste);
        assert!(!absent.mouse_capture);
        assert!(!absent.osc52_clipboard);
        assert!(!absent.alternate_screen);
        assert!(!absent.focus_reporting);

        // act — absent teardown must not disable features that were never enabled
        let absent_plan = absent.teardown_plan();
        // assert absent fallback teardown
        assert!(absent_plan.disable_raw_mode);
        assert!(!absent_plan.disable_mouse_capture);
        assert!(!absent_plan.disable_bracketed_paste);
        assert!(!absent_plan.pop_keyboard_enhancement);
        assert!(absent_plan.leave_alternate_screen);

        // arrange — partial capability path (keyboard+paste+alt ok; mouse failed)
        let partial = apply_interactive_setup_results(
            TerminalCapabilityState {
                truecolor: true,
                osc52_clipboard: false,
                ..TerminalCapabilityState::absent()
            },
            true,
            true,
            false,
            true,
            false,
        );
        // assert partial: only successfully applied features are sticky
        assert!(partial.keyboard_enhancement);
        assert!(partial.truecolor);
        assert!(partial.bracketed_paste);
        assert!(!partial.mouse_capture);
        assert!(!partial.osc52_clipboard);
        assert!(partial.alternate_screen);
        assert!(!partial.focus_reporting);
        let partial_plan = partial.teardown_plan();
        assert!(partial_plan.pop_keyboard_enhancement);
        assert!(!partial_plan.disable_mouse_capture);
        assert!(partial_plan.disable_bracketed_paste);
    }

    #[test]
    fn terminal_restore_guard_carries_capability_state() {
        // arrange
        let caps = apply_interactive_setup_results(
            TerminalCapabilityState {
                truecolor: true,
                osc52_clipboard: false,
                ..TerminalCapabilityState::absent()
            },
            true,
            true,
            false,
            true,
            false,
        );
        // act
        let guard = TerminalRestoreGuard::new(caps);
        // assert
        assert_eq!(guard.capabilities(), caps);
        assert!(guard.capabilities().keyboard_enhancement);
        assert!(guard.capabilities().bracketed_paste);
        assert!(!guard.capabilities().mouse_capture);
        assert!(guard.capabilities().alternate_screen);
        assert!(guard.capabilities().truecolor);
        assert!(!guard.capabilities().osc52_clipboard);
    }

    #[test]
    fn partial_setup_failure_keeps_only_successfully_enabled_capabilities() {
        // arrange
        let base = TerminalCapabilityState {
            truecolor: true,
            osc52_clipboard: true,
            ..TerminalCapabilityState::absent()
        };
        // act
        let caps = apply_interactive_setup_results(base, true, true, false, true, false);

        // assert
        assert!(caps.keyboard_enhancement);
        assert!(caps.bracketed_paste);
        assert!(!caps.mouse_capture);
        assert!(caps.alternate_screen);
        assert!(caps.truecolor);
        assert!(caps.osc52_clipboard);

        // act
        let plan = caps.teardown_plan();
        // assert
        assert!(plan.pop_keyboard_enhancement);
        assert!(plan.disable_bracketed_paste);
        assert!(!plan.disable_mouse_capture);
        assert!(plan.leave_alternate_screen);
        assert!(plan.disable_raw_mode);
    }

    #[test]
    fn truecolor_detection_accepts_colorterm_truecolor_and_24bit() {
        // arrange (no setup needed)
        // act (function calls below)
        // assert
        assert!(truecolor_from_colorterm(Some("truecolor")));
        assert!(truecolor_from_colorterm(Some("24bit")));
        assert!(truecolor_from_colorterm(Some("TRUECOLOR")));
        assert!(!truecolor_from_colorterm(Some("")));
        assert!(!truecolor_from_colorterm(Some("xterm-256color")));
        assert!(!truecolor_from_colorterm(None));
    }

    #[test]
    fn terminal_restore_guard_mark_restored_skips_drop_teardown_path() {
        // arrange
        let mut guard = TerminalRestoreGuard::new(TerminalCapabilityState::present());
        // act
        guard.mark_restored();
        // assert
        assert!(guard.restored());
        drop(guard);
    }
}
