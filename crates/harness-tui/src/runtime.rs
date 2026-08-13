// allow: SIZE_OK — TUI runtime loop (poll interval + event dispatch + terminal resize + shutdown handling)
use crate::UnwrapOrAbort;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
    MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use harness_core::event::EventEnvelopeV1;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use crate::app::{AppState, LaunchMetadata, SessionHistoryEntry, TogglesConfig, UiIntent};
use crate::event;
use crate::input::{
    ScrollConfigOverrides, ScrollNormalizer, ScrollNormalizerConfig, ScrollSampleDirection,
    TerminalEnvelope, TerminalIngressReader, TerminalQueue, TerminalReaderStatus,
};
use crate::presentation::{
    CauseId, InteractionId, PresentationCauseKind, PresentationClock, RenderDemand, RenderReason,
};
use crate::runtime_input::{should_apply_live_update, InputPresentation};
use crate::runtime_integration::RuntimeExperience;
use crate::runtime_live_updates::{
    apply_live_update_quantum, live_update_channel, LiveUpdateDrainState, LiveUpdateReceiver,
};
#[cfg(test)]
use crate::runtime_live_updates::{drain_live_updates, LIVE_UPDATE_DRAIN_MAX_PER_FRAME};
use crate::runtime_presentation::{InteractionEventClass, PresentationTelemetrySession};
use crate::runtime_scheduling::{
    SchedulingLiveReadiness, SchedulingReadinessSignal, SchedulingTelemetrySession,
};
use crate::runtime_wait_set::{FrameRuntimeEvent, RuntimeWaitSet, RuntimeWake};
use crate::scheduling::{
    BatchBudget, FairnessTurn, FrameNow, MotionPlan, RuntimeArbiter, RuntimeDecision, RuntimePacer,
    RuntimePacerAction, RuntimeReady, WheelBatch, WheelDirection, WheelSample,
};
use crate::terminal::{
    FrameKind, FrameOutput, FrameOutputBackend, FrameSubmission, Presenter,
    ProductionTerminalSession,
};
use crate::ui;

const FRAME_OUTPUT_QUEUE_CAPACITY: usize = 1;

fn select_runtime_decision(arbiter: &RuntimeArbiter, ready: RuntimeReady) -> RuntimeDecision {
    arbiter.decide(ready)
}

fn record_scheduling_decision(
    session: Option<&mut SchedulingTelemetrySession>,
    interaction_id: Option<&InteractionId>,
    cause_id: Option<&CauseId>,
    live: SchedulingLiveReadiness,
    fairness_yield: bool,
) {
    if let (Some(session), Some(cause_id)) = (session, cause_id) {
        session.record_terminal_ready(
            interaction_id,
            cause_id,
            live,
            fairness_yield,
            Some(crate::scheduling::FLUSH_DEADLINE_MS),
        );
    }
}

fn has_canonical_render_demand(telemetry_enabled: bool, demand: Option<&RenderDemand>) -> bool {
    !telemetry_enabled || demand.is_some()
}

fn refresh_motion_plan(app: &mut AppState) -> MotionPlan {
    app.refresh_motion_state();
    app.motion_plan()
}

fn prioritize_terminal_before_present(
    queue: &mut TerminalQueue,
    pending: &mut Option<TerminalEnvelope>,
    input_priority: &mut bool,
) {
    if !*input_priority && pending.is_none() {
        *pending = queue.try_recv().ok();
        *input_priority = pending.is_some();
    }
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

pub(crate) fn apply_startup_capability_notice(
    app: &mut AppState,
    clipboard_warning_required: bool,
) {
    if app.startup_shell_visible() && clipboard_warning_required && app.status_banner.is_none() {
        app.set_status_banner(Some("Clipboard may be unreachable.".to_owned()));
    }
}

fn is_ssh_session() -> bool {
    ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}

fn truecolor_from_colorterm(value: Option<&str>) -> bool {
    value
        .map(str::to_ascii_lowercase)
        .is_some_and(|lower| lower.contains("truecolor") || lower.contains("24bit"))
}

fn reduced_motion_from_env(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
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
        message: String,
    },
    AuthProviderCatalogRefreshed {
        launch_metadata: Box<LaunchMetadata>,
    },
    PluginLifecycleSummary(harness_core::integrations::PluginLifecycleSummary),
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
        update_rx: LiveUpdateReceiver,
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
        update_rx: LiveUpdateReceiver,
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
    pub skip_alternate_screen: bool,
}

impl TuiOptions {
    fn take_external_keybindings(&mut self) -> Option<std::collections::BTreeMap<String, String>> {
        self.keybindings
            .take()
            .filter(|bindings| !bindings.is_empty())
    }
}

fn render_terminal_frame(
    terminal: &mut Terminal<FrameOutputBackend>,
    output: &mut FrameOutput,
    demand: Option<RenderDemand>,
    render: impl FnOnce(&mut Terminal<FrameOutputBackend>) -> Result<()>,
) -> Result<FrameSubmission> {
    let kind = match demand {
        Some(demand) => output.begin_frame_for(demand)?,
        None => output.begin_frame()?,
    };
    let render_result = (|| -> Result<()> {
        if matches!(kind, FrameKind::FullRepaint) {
            terminal.backend_mut().invalidate_cursor_state();
            terminal
                .clear()
                .context("failed to clear terminal for full repaint")?;
        }
        render(terminal)
    })();
    match render_result {
        Ok(()) => output.finish_frame().map_err(Into::into),
        Err(error) => {
            output.abort_frame();
            Err(error)
        }
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
        skip_alternate_screen,
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
            let starting_session_seed = historical_events.is_empty();
            let mut app = AppState::new_live_with_session_history_and_prompt_history_path(
                Some(run_dir.clone()),
                exit_on_finish,
                on_ui_intent,
                session_history_entries,
                prompt_history_path,
            );
            app.set_starting_session_seed(
                starting_session_seed && app.composer.prompt_buffer.is_empty(),
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

    let mut terminal_session = ProductionTerminalSession::negotiate();
    let mut experience = RuntimeExperience::new();

    let preserved_terminal = recover_mutex_lock(preserved_terminal_session()).clone();
    let reusing_terminal = preserved_terminal.active;
    let mut capabilities = if reusing_terminal {
        preserved_terminal.capabilities
    } else {
        let mut capabilities = TerminalCapabilityState::from_environment();
        capabilities.osc52_clipboard = terminal_session.capabilities.osc52_clipboard;
        capabilities
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
            if !skip_alternate_screen {
                crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)
                    .context("failed to enter alternate screen before launching TUI")?;
                alt_screen_ok = true;
            }

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
        terminal_session.record_setup(
            true,
            capabilities.alternate_screen,
            capabilities.bracketed_paste,
        );
    }

    let clipboard_warning_required =
        crate::terminal::startup_diagnostics::clipboard_warning_required(
            terminal_session.context,
            is_ssh_session(),
        );
    apply_startup_capability_notice(&mut app, clipboard_warning_required);

    app.set_color_level(crate::theme::detect_color_level(
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    ));
    app.set_glyph_mode(terminal_session.matrix.classified_by().glyph_mode());
    let reduced_motion = std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some()
        || reduced_motion_from_env(std::env::var("HARNESS_TUI_REDUCED_MOTION").ok().as_deref());
    app.set_reduced_motion(reduced_motion);
    let mut presentation_session = PresentationTelemetrySession::from_env()
        .context("failed to initialize local presentation telemetry")?;
    let mut scheduling_session = SchedulingTelemetrySession::from_env()
        .context("failed to initialize local scheduling telemetry")?;
    let mut scheduling_readiness = SchedulingReadinessSignal::from_env()
        .context("failed to initialize local scheduling readiness signal")?;
    let presentation_clock = presentation_session
        .as_ref()
        .map_or_else(PresentationClock::new, PresentationTelemetrySession::clock);

    let mut restore_guard = TerminalRestoreGuard::new(capabilities);

    let (mut frame_output, frame_writer, frame_receiver) =
        FrameOutput::bounded_with_clock(FRAME_OUTPUT_QUEUE_CAPACITY, presentation_clock);
    frame_output.require_full_repaint();
    let backend = FrameOutputBackend::new(frame_writer);
    let mut terminal = Terminal::new(backend)?;
    let writer_worker = frame_receiver.spawn(stdout)?;
    let (terminal_reader, mut terminal_ingress) = TerminalIngressReader::spawn(usize::from(
        crate::perf_budgets::QueueBounds::strict().max_input_events,
    ));

    let mut run_result = (|| -> Result<()> {
        let pacing_epoch = Instant::now();
        let mut pacer = RuntimePacer::with_reduced_motion(reduced_motion);
        let scroll_mode = std::env::var("HARNESS_TUI_SCROLL_MODE").ok();
        let scroll_lines = std::env::var("HARNESS_TUI_SCROLL_LINES").ok();
        let scroll_speed = std::env::var("HARNESS_TUI_SCROLL_SPEED").ok();
        let invert_scroll = std::env::var("HARNESS_TUI_INVERT_SCROLL").ok();
        let scroll_overrides = ScrollConfigOverrides::from_values(
            scroll_mode.as_deref(),
            scroll_lines.as_deref(),
            scroll_speed.as_deref(),
            invert_scroll.as_deref(),
        );
        let scroll_config = ScrollNormalizerConfig::for_terminal(
            terminal_session.context.brand,
            terminal_session.context.multiplexer,
        )
        .with_overrides(scroll_overrides);
        let mut scroll_normalizer = ScrollNormalizer::new(scroll_config);
        let mut presenter = Presenter::new();
        let mut pending_terminal = None;
        let mut arbiter = RuntimeArbiter::default();
        let mut input_budget = None;
        if let Some(session) = presentation_session.as_mut() {
            session.record_visible_cause(
                PresentationCauseKind::Startup,
                RenderReason::Startup,
                None,
            );
        }

        loop {
            let motion_plan = refresh_motion_plan(&mut app);
            let frame_ready = frame_output.is_ready_for_frame();
            if let Some(failure) = frame_output.take_fatal_failure() {
                return Err(failure.into());
            }
            if let Some(session) = presentation_session.as_mut() {
                session.record_acknowledgements(frame_output.take_acknowledgements());
            }
            if pending_terminal.is_none() {
                pending_terminal = terminal_ingress.queue.try_recv().ok();
            }
            if let Some(signal) = scheduling_readiness.as_mut() {
                let stream_active = app.active_turn_in_progress();
                let live = live_updates.as_ref().map_or(
                    SchedulingLiveReadiness {
                        stream_active,
                        ..SchedulingLiveReadiness::default()
                    },
                    |receiver| receiver.scheduling_readiness(stream_active),
                );
                signal
                    .publish_if_changed(live)
                    .context("failed to publish local scheduling readiness")?;
            }
            let now = Instant::now();
            if input_budget
                .as_ref()
                .is_some_and(|budget: &BatchBudget| budget.exhausted(now))
            {
                arbiter.input_quantum_exhausted();
            }
            let pacing_due = pacer.needs_poll(runtime_frame_now(pacing_epoch, now), motion_plan);
            let decision = select_runtime_decision(
                &arbiter,
                RuntimeReady {
                    quit: app.should_quit,
                    terminal_input: pending_terminal.is_some(),
                    pacer_deadline: pacing_due,
                    live_update: live_updates
                        .as_ref()
                        .is_some_and(|receiver| !receiver.is_empty()),
                    ..RuntimeReady::default()
                },
            );
            let mut input_priority = matches!(decision, RuntimeDecision::TerminalInput);
            if should_apply_live_update(decision, &presenter, frame_ready) {
                if let Some(update_rx) = live_updates.as_ref() {
                    let drain_state =
                        apply_live_update_quantum(&mut app, update_rx, &mut experience);
                    if drain_state.changed {
                        if let Some(session) = presentation_session.as_mut() {
                            session.record_visible_cause(
                                PresentationCauseKind::LiveUpdate,
                                RenderReason::LiveUpdate,
                                None,
                            );
                        }
                        presenter.request_redraw(Instant::now());
                        pacer.request_flush();
                    }
                    if drain_state.disconnected {
                        live_updates = None;
                    }
                    arbiter.live_applied();
                    input_budget = None;
                }
            }
            if app.clear_expired_quit_confirmation() {
                if let Some(session) = presentation_session.as_mut() {
                    session.record_visible_cause(
                        PresentationCauseKind::Expiry,
                        RenderReason::Expiry,
                        None,
                    );
                }
                pacer.request_flush();
            }

            let pacing_action = if matches!(
                decision,
                RuntimeDecision::PacerDeadline | RuntimeDecision::AnimationDeadline
            ) {
                let action =
                    pacer.poll(runtime_frame_now(pacing_epoch, Instant::now()), motion_plan);
                arbiter.deadline_served();
                action
            } else {
                RuntimePacerAction::default()
            };
            if pacing_action.advance_animation {
                app.sample_motion_clock();
                if let Some(session) = presentation_session.as_mut() {
                    session.record_visible_cause(
                        PresentationCauseKind::AnimationTimer,
                        RenderReason::Animation,
                        None,
                    );
                }
            }
            let wheel_changed = if let Some(batch) = pacing_action.wheel_batch {
                let size = terminal.size()?;
                let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                app.set_frame_area(frame_area);
                dispatch_wheel_batch(&mut app, frame_area, batch)
            } else {
                false
            };

            let paint_requested = pacing_action.should_paint(wheel_changed);
            if paint_requested {
                let demand = presentation_session
                    .as_mut()
                    .and_then(PresentationTelemetrySession::take_render_demand);
                match demand {
                    Some(demand) => presenter.request_redraw_for(demand, Instant::now()),
                    None if presentation_session.is_none() => {
                        presenter.request_redraw(Instant::now());
                    }
                    None => {}
                }
            }
            prioritize_terminal_before_present(
                &mut terminal_ingress.queue,
                &mut pending_terminal,
                &mut input_priority,
            );
            if !input_priority && presenter.should_present(frame_ready) {
                let demand = presenter.take_render_demand().or_else(|| {
                    presentation_session
                        .as_mut()
                        .and_then(PresentationTelemetrySession::take_render_demand)
                });
                if !has_canonical_render_demand(presentation_session.is_some(), demand.as_ref()) {
                    let submission = FrameSubmission::Unchanged;
                    pacer.record_submission(submission, motion_plan);
                    presenter.record_submission(submission, Instant::now());
                    continue;
                }
                let size = terminal.size()?;
                let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                app.set_frame_area(frame_area);
                experience.tick(&app);
                let submission = render_terminal_frame(
                    &mut terminal,
                    &mut frame_output,
                    demand.clone(),
                    |terminal| {
                        terminal.draw(|frame| ui::render_app(frame, &app))?;
                        experience.post_flush(terminal.backend_mut());
                        Ok(())
                    },
                )?;
                if matches!(submission, FrameSubmission::ResyncRequired) {
                    pacer.request_flush();
                }
                pacer.record_submission(submission, motion_plan);
                if let (Some(session), Some(demand)) =
                    (presentation_session.as_mut(), demand.as_ref())
                {
                    match submission {
                        FrameSubmission::Accepted(_) => {}
                        FrameSubmission::Unchanged => session
                            .record_no_visible_change(demand)
                            .context("failed to record unchanged presentation")?,
                        FrameSubmission::ResyncRequired => session
                            .record_resync(demand)
                            .context("failed to record presentation resync")?,
                    }
                }
                presenter.record_submission(submission, Instant::now());
            } else if presenter.scheduled_at().is_some() {
                pacer.request_flush();
            }

            if matches!(decision, RuntimeDecision::Quit) || app.should_quit {
                break;
            }

            let event = if input_priority {
                let envelope = pending_terminal.take();
                let budget = input_budget.get_or_insert_with(|| BatchBudget::input(Instant::now()));
                budget.consume();
                envelope.map(|envelope| envelope.event)
            } else if matches!(decision, RuntimeDecision::Park) {
                let now = Instant::now();
                let deadline = pacer
                    .next_wait_ms(runtime_frame_now(pacing_epoch, now))
                    .map(|millis| now + Duration::from_millis(millis));
                let wait_set = RuntimeWaitSet {
                    frame: frame_output.acknowledgement_receiver(),
                    reader: &terminal_ingress.status,
                    terminal: terminal_ingress.queue.receiver(),
                    live: live_updates.as_ref().map(LiveUpdateReceiver::receiver),
                };
                match wait_set.wait(deadline) {
                    RuntimeWake::Terminal(envelope) => {
                        pending_terminal = Some(envelope);
                        None
                    }
                    RuntimeWake::Live(update) => {
                        if let Some(update_rx) = live_updates.as_ref() {
                            update_rx.defer_selected(update);
                        }
                        None
                    }
                    RuntimeWake::Frame(FrameRuntimeEvent::Acknowledged(ack)) => {
                        frame_output.accept_acknowledgement(ack);
                        None
                    }
                    RuntimeWake::Frame(FrameRuntimeEvent::Failed { ack, stage }) => {
                        frame_output.accept_acknowledgement(ack);
                        return Err(crate::terminal::FrameOutputFailure::Write(stage).into());
                    }
                    RuntimeWake::Frame(FrameRuntimeEvent::Disconnected) => {
                        return Err(crate::terminal::FrameOutputFailure::Disconnected.into());
                    }
                    RuntimeWake::Reader(TerminalReaderStatus::Failed(error)) => {
                        return Err(error.into());
                    }
                    RuntimeWake::LiveDisconnected => {
                        live_updates = None;
                        None
                    }
                    RuntimeWake::Reader(TerminalReaderStatus::Stopped)
                    | RuntimeWake::ReaderDisconnected
                    | RuntimeWake::TerminalDisconnected => {
                        return Err(anyhow::anyhow!("terminal ingress reader disconnected"));
                    }
                    RuntimeWake::Deadline => None,
                }
            } else {
                None
            };

            if let Some(event) = event {
                let input_presentation = InputPresentation::for_event(&event);
                let event_class = match &event {
                    event::TuiEvent::Key(_) => InteractionEventClass::Key,
                    event::TuiEvent::Paste(_) => InteractionEventClass::Paste,
                    event::TuiEvent::Mouse(_) => InteractionEventClass::Mouse,
                    event::TuiEvent::Resize(_, _) => InteractionEventClass::Resize,
                    event::TuiEvent::FocusGained | event::TuiEvent::FocusLost => {
                        InteractionEventClass::Focus
                    }
                };
                let interaction_id = match presentation_session.as_mut() {
                    Some(session) => session
                        .take_interaction_id(event_class)
                        .context("failed to read runner interaction identity")?,
                    None => None,
                };
                let stream_active = app.active_turn_in_progress();
                let live_readiness = live_updates.as_ref().map_or(
                    SchedulingLiveReadiness {
                        stream_active,
                        ..SchedulingLiveReadiness::default()
                    },
                    |receiver| receiver.scheduling_readiness(stream_active),
                );
                let fairness_yield =
                    matches!(arbiter.fairness(), FairnessTurn::OneLiveAfterInputQuantum);
                let (cause_kind, render_reason) = match &event {
                    event::TuiEvent::Resize(_, _) => {
                        (PresentationCauseKind::Resize, RenderReason::Resize)
                    }
                    event::TuiEvent::FocusGained | event::TuiEvent::FocusLost => {
                        (PresentationCauseKind::Focus, RenderReason::Focus)
                    }
                    event::TuiEvent::Mouse(_) => {
                        (PresentationCauseKind::Wheel, RenderReason::Wheel)
                    }
                    event::TuiEvent::Key(_) | event::TuiEvent::Paste(_) => (
                        PresentationCauseKind::TerminalInput,
                        RenderReason::TerminalInput,
                    ),
                };
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
                            let cause_id = match presentation_session.as_mut() {
                                Some(session) => Some(
                                    session
                                        .record_no_visible_cause(cause_kind, interaction_id.clone())
                                        .context("failed to record ignored terminal input")?,
                                ),
                                None => None,
                            };
                            record_scheduling_decision(
                                scheduling_session.as_mut(),
                                cause_id.as_ref().and(interaction_id.as_ref()),
                                cause_id.as_ref(),
                                live_readiness,
                                fairness_yield,
                            );
                            continue;
                        }

                        let scroll_direction = match mouse.kind {
                            MouseEventKind::ScrollUp => Some(ScrollSampleDirection::Up),
                            MouseEventKind::ScrollDown => Some(ScrollSampleDirection::Down),
                            _ => None,
                        };
                        if let Some(direction) = scroll_direction {
                            let size = terminal.size()?;
                            let normalized = scroll_normalizer.push(
                                Instant::now().saturating_duration_since(pacing_epoch),
                                direction,
                                mouse.column,
                                mouse.row,
                                size.height,
                            );
                            if normalized.lines != 0 {
                                let direction = if normalized.lines.is_negative() {
                                    WheelDirection::Up
                                } else {
                                    WheelDirection::Down
                                };
                                let steps = u8::try_from(normalized.lines.unsigned_abs())
                                    .unwrap_or(u8::MAX);
                                pacer.queue_wheel(WheelSample::logical(
                                    direction,
                                    steps,
                                    normalized.column,
                                    normalized.row,
                                ));
                            }
                            let cause_id = if let Some(session) = presentation_session.as_mut() {
                                if normalized.lines == 0 {
                                    Some(
                                        session
                                            .record_no_visible_cause(
                                                cause_kind,
                                                interaction_id.clone(),
                                            )
                                            .context("failed to record unchanged wheel input")?,
                                    )
                                } else {
                                    Some(session.record_visible_cause(
                                        cause_kind,
                                        render_reason,
                                        interaction_id.clone(),
                                    ))
                                }
                            } else {
                                None
                            };
                            record_scheduling_decision(
                                scheduling_session.as_mut(),
                                cause_id.as_ref().and(interaction_id.as_ref()),
                                cause_id.as_ref(),
                                live_readiness,
                                fairness_yield,
                            );
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
                    event::TuiEvent::FocusGained => {
                        terminal_session.set_focus(true);
                        terminal_session.restore();
                        experience.set_focus(true, terminal.backend_mut());
                        true
                    }
                    event::TuiEvent::FocusLost => {
                        terminal_session.set_focus(false);
                        terminal_session.suspend();
                        experience.set_focus(false, terminal.backend_mut());
                        true
                    }
                };
                let input_presentation =
                    input_presentation.for_turn_start(stream_active, app.active_turn_in_progress());
                let cause_id = if event_changed {
                    input_presentation.request(true, &mut presenter, &mut pacer, Instant::now());
                    presentation_session.as_mut().map(|session| {
                        session.record_visible_cause(
                            cause_kind,
                            render_reason,
                            interaction_id.clone(),
                        )
                    })
                } else if let Some(session) = presentation_session.as_mut() {
                    Some(
                        session
                            .record_no_visible_cause(cause_kind, interaction_id.clone())
                            .context("failed to record unchanged terminal input")?,
                    )
                } else {
                    None
                };
                record_scheduling_decision(
                    scheduling_session.as_mut(),
                    interaction_id.as_ref(),
                    cause_id.as_ref(),
                    live_readiness,
                    fairness_yield,
                );
            }
        }
        Ok(())
    })();

    if let Some(session) = presentation_session.as_mut() {
        if let Some(demand) = session.take_render_demand() {
            session
                .record_no_visible_change(&demand)
                .context("failed to close unpresented shutdown demand")?;
        }
    }
    if terminal_reader.stop_and_join().is_err() && run_result.is_ok() {
        run_result = Err(anyhow::anyhow!("terminal ingress reader panicked"));
    }
    terminal.backend_mut().prepare_for_terminal_drop();
    drop(terminal);
    while frame_output.has_in_flight_frame() {
        match frame_output.acknowledgement_receiver().recv() {
            Ok(ack) => frame_output.accept_acknowledgement(ack),
            Err(_) => {
                if run_result.is_ok() {
                    run_result = Err(anyhow::anyhow!(
                        "terminal frame writer acknowledgement disconnected"
                    ));
                }
                break;
            }
        }
    }
    if let Some(session) = presentation_session.as_mut() {
        session.record_acknowledgements(frame_output.take_acknowledgements());
    }
    drop(frame_output);
    let writer_result = writer_worker.join();
    if let Some(session) = presentation_session.take() {
        session
            .finish()
            .context("failed to persist local presentation telemetry")?;
    }
    if let Some(session) = scheduling_session.take() {
        session
            .finish()
            .context("failed to persist local scheduling telemetry")?;
    }
    let mut stdout = writer_result.context("terminal frame writer failed")?;
    crossterm::execute!(stdout, Show).context("failed to restore terminal cursor after TUI")?;
    experience.cleanup(&mut stdout);

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
    teardown_terminal_session(&mut stdout, capabilities)?;
    terminal_session.finish();
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
    let (_tx, rx) = live_update_channel();
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
        skip_alternate_screen: false,
    })
}

fn runtime_frame_now(epoch: Instant, now: Instant) -> FrameNow {
    let elapsed_ms =
        u64::try_from(now.saturating_duration_since(epoch).as_millis()).unwrap_or(u64::MAX);
    FrameNow {
        animation_ms: elapsed_ms,
        flush_ms: elapsed_ms,
    }
}

#[cfg(test)]
fn poll_timeout(pacer: &RuntimePacer, now: FrameNow) -> Option<Duration> {
    pacer.next_wait_ms(now).map(Duration::from_millis)
}

fn dispatch_wheel_batch(
    app: &mut AppState,
    frame_area: ratatui::layout::Rect,
    batch: WheelBatch,
) -> bool {
    let kind = match batch.direction() {
        WheelDirection::Up => MouseEventKind::ScrollUp,
        WheelDirection::Down => MouseEventKind::ScrollDown,
    };
    let hovered_wheel_target =
        ui::hovered_wheel_target(app, frame_area, batch.column(), batch.row());
    let mut changed = false;
    for _ in 0..batch.steps() {
        changed |= app.handle_mouse(
            MouseEvent {
                kind,
                column: batch.column(),
                row: batch.row(),
                modifiers: KeyModifiers::NONE,
            },
            frame_area,
            hovered_wheel_target,
            None,
            None,
        );
    }
    changed
}

fn mouse_event_requires_handling(_kind: MouseEventKind, _slash_visible: bool) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, ToastVariant};
    use crate::scheduling::INPUT_BATCH_LIMIT;
    use crate::UnwrapOrAbort;
    use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

    #[test]
    fn production_selector_observes_arbiter_fairness_mutation() {
        let ready = RuntimeReady {
            terminal_input: true,
            live_update: true,
            ..RuntimeReady::default()
        };
        let mut arbiter = RuntimeArbiter::default();

        assert_eq!(
            select_runtime_decision(&arbiter, ready),
            RuntimeDecision::TerminalInput
        );
        arbiter.input_quantum_exhausted();
        assert_eq!(
            select_runtime_decision(&arbiter, ready),
            RuntimeDecision::LiveUpdate
        );
        arbiter.live_applied();
        assert_eq!(
            select_runtime_decision(&arbiter, ready),
            RuntimeDecision::TerminalInput
        );
    }

    #[test]
    fn active_stream_does_not_synthesize_live_readiness() {
        let active_without_work = SchedulingLiveReadiness {
            stream_active: true,
            ..SchedulingLiveReadiness::default()
        };
        assert_eq!(active_without_work.ready_depth(), 0);
        assert_eq!(
            SchedulingLiveReadiness {
                queued_depth: 7,
                deferred_ready: true,
                stream_active: true,
            }
            .ready_depth(),
            8
        );
    }

    #[test]
    fn native_telemetry_never_synthesizes_an_unrecorded_frame_cause() {
        assert!(!has_canonical_render_demand(true, None));
        assert!(has_canonical_render_demand(false, None));
    }

    #[test]
    fn production_selector_preempts_sustained_live_backlog_with_bounded_fairness() {
        let ready = RuntimeReady {
            terminal_input: true,
            live_update: true,
            ..RuntimeReady::default()
        };
        let mut arbiter = RuntimeArbiter::default();
        let now = Instant::now();
        let mut budget = BatchBudget::input(now);
        let mut terminal_decisions = 0_usize;
        let mut live_decisions = 0_usize;

        for _ in 0..(INPUT_BATCH_LIMIT * 4 + 4) {
            if budget.exhausted(now) {
                arbiter.input_quantum_exhausted();
            }
            match select_runtime_decision(&arbiter, ready) {
                RuntimeDecision::TerminalInput => {
                    terminal_decisions = terminal_decisions.saturating_add(1);
                    budget.consume();
                }
                RuntimeDecision::LiveUpdate => {
                    live_decisions = live_decisions.saturating_add(1);
                    arbiter.live_applied();
                    budget = BatchBudget::input(now);
                }
                decision => panic!("unexpected scheduling decision: {decision:?}"),
            }
        }

        assert_eq!(terminal_decisions, INPUT_BATCH_LIMIT * 4);
        assert_eq!(live_decisions, 4);
    }

    #[test]
    fn due_pacer_rotates_to_a_bounded_live_quantum() {
        // Given: motion is continuously due and more live work is queued than one quantum.
        let (sender, receiver) = live_update_channel();
        for index in 0..(LIVE_UPDATE_DRAIN_MAX_PER_FRAME + 3) {
            sender
                .send(LiveUpdate::Status(format!("status {index}")))
                .expect("live receiver remains connected");
        }
        let ready = RuntimeReady {
            pacer_deadline: true,
            live_update: true,
            ..RuntimeReady::default()
        };
        let mut arbiter = RuntimeArbiter::default();

        // When: one natural deadline is served and production applies the live turn.
        assert_eq!(
            select_runtime_decision(&arbiter, ready),
            RuntimeDecision::PacerDeadline
        );
        arbiter.deadline_served();
        assert_eq!(
            select_runtime_decision(&arbiter, ready),
            RuntimeDecision::LiveUpdate
        );
        let mut app = AppState::default();
        let mut experience = RuntimeExperience::new();
        let drained = apply_live_update_quantum(&mut app, &receiver, &mut experience);

        // Then: the bounded quantum progresses sixteen items, rather than one item per frame.
        assert!(drained.changed);
        assert!(drained.budget_exhausted);
        assert_eq!(receiver.ready_depth(), 3);
    }

    #[test]
    fn terminal_arrival_after_arbitration_preempts_dirty_frame_build() {
        // Given: lower-priority work won arbitration just before a click reaches ingress.
        let (sender, receiver) = crossbeam_channel::bounded(2);
        let mut queue = crate::input::TerminalQueue::new(receiver);
        let mut pending = None;
        let mut input_priority = false;
        sender
            .send(crate::input::TerminalEnvelope::new(
                crate::input::TerminalSequence::new(1),
                Instant::now(),
                event::TuiEvent::Mouse(crossterm::event::MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 6,
                    row: 8,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                }),
            ))
            .expect("terminal queue remains connected");

        // When: production reaches the last boundary before an expensive frame build.
        prioritize_terminal_before_present(&mut queue, &mut pending, &mut input_priority);

        // Then: the click is dispatched before lower-priority rendering starts.
        assert!(input_priority);
        assert!(matches!(
            pending.map(|envelope| envelope.event),
            Some(event::TuiEvent::Mouse(_))
        ));
    }

    #[test]
    fn poll_timeout_parks_when_runtime_pacer_is_idle() {
        // Given: an idle runtime pacer.
        let pacer = RuntimePacer::new();

        // When: the terminal asks how long it may park.
        let timeout = poll_timeout(&pacer, FrameNow::default());

        // Then: no paint deadline shortens the idle interval.
        assert_eq!(timeout, None);
    }

    #[test]
    fn poll_timeout_tracks_runtime_pacer_animation_deadline() {
        // Given: an active runtime pacer armed at zero.
        let mut pacer = RuntimePacer::new();
        pacer.poll(FrameNow::default(), true);

        // When: the terminal checks before and at the animation deadline.
        let pending = poll_timeout(&pacer, FrameNow::default());
        let due = poll_timeout(
            &pacer,
            FrameNow {
                animation_ms: crate::scheduling::ANIMATION_PERIOD_MS,
                flush_ms: 0,
            },
        );

        // Then: the scheduler's 30 Hz deadline is the poll authority.
        assert_eq!(
            (pending, due),
            (
                Some(Duration::from_millis(
                    crate::scheduling::ANIMATION_PERIOD_MS
                )),
                Some(Duration::ZERO),
            )
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
        let (tx, rx) = live_update_channel();
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
    fn overlay_pause_is_applied_before_runtime_arms_toast_deadline() {
        // Given: an active toast becomes occluded before the next runtime loop.
        let mut app = AppState::default();
        app.set_toast_for_test("Copied", ToastVariant::Info);
        assert!(app.motion_plan().until().is_some());
        app.palette_visible = true;

        // When: the production runtime refresh-and-plan boundary is evaluated.
        let plan = refresh_motion_plan(&mut app);

        // Then: the paused toast contributes no stale wake deadline.
        assert!(plan.is_none());
        let pacer = RuntimePacer::new();
        assert!(!pacer.needs_poll(FrameNow::default(), plan));
    }

    #[test]
    fn drain_live_updates_routes_operator_notice_to_toast() {
        // arrange
        // act
        // assert
        let (tx, rx) = live_update_channel();
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
        let (tx, rx) = live_update_channel();
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
        let (tx, rx) = live_update_channel();
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
    fn run_started_clears_the_new_session_bootstrap_status() {
        // Given: new-live bootstrap posted a transient status before runtime events arrived.
        let (tx, rx) = live_update_channel();
        tx.send(LiveUpdate::Status("starting new session".to_string()))
            .unwrap_or_abort();
        tx.send(LiveUpdate::Event(Box::new(EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: "evt-bootstrap-started".to_string(),
            seq: 1,
            run_id: "run-bootstrap-started".into(),
            mono_ms: 1,
            ts: None,
            actor: harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                Some("coordinator".to_string()),
            ),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run-bootstrap-started".to_string()),
            payload: harness_core::event::EventV1::RunStarted(
                harness_core::event::RunStartedEvent {
                    run_name: "bootstrap started".into(),
                    workspace_root: "/workspace".to_string(),
                },
            ),
        })))
        .unwrap_or_abort();
        let mut app = AppState::new_live(None, false, None);

        // When: the live-update quantum applies the bootstrap status and first runtime event.
        drain_live_updates(&mut app, &rx);

        // Then: the transient bootstrap banner no longer overrides live turn state.
        assert_eq!(app.status_banner, None);
    }

    #[test]
    fn drain_live_updates_applies_auth_backend_result_to_status_banner() {
        // arrange
        // act
        // assert
        let (tx, rx) = live_update_channel();
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_status_banner(Some("No provider connected. Use /connect.".to_string()));
        tx.send(LiveUpdate::AuthBackendResult {
            success: true,
            message: "auth backend completed".to_string(),
        })
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
    fn drain_live_updates_preserves_streamed_auth_failure_detail() {
        // arrange
        let (tx, rx) = live_update_channel();
        let mut app = AppState::new_startup(Vec::new(), None);
        tx.send(LiveUpdate::OperatorNotice {
            message:
                "auth backend error: auth login failed: could not bind Codex loopback callback"
                    .to_string(),
            level: OperatorNoticeLevel::Error,
        })
        .unwrap_or_abort();
        tx.send(LiveUpdate::OperatorNotice {
            message: "auth backend failed (exit 1): harness auth login openai".to_string(),
            level: OperatorNoticeLevel::Error,
        })
        .unwrap_or_abort();
        tx.send(LiveUpdate::AuthBackendResult {
            success: false,
            message: "auth backend failed (exit 1): harness auth login openai".to_string(),
        })
        .unwrap_or_abort();

        // act
        drain_live_updates(&mut app, &rx);

        // assert
        assert_eq!(
            app.status_banner.as_deref(),
            Some("auth backend error: auth login failed: could not bind Codex loopback callback")
        );
    }

    #[test]
    fn drain_live_updates_yields_after_frame_budget() {
        // arrange
        // act
        // assert
        let (tx, rx) = live_update_channel();
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
    fn reduced_motion_override_accepts_only_explicit_enabled_values() {
        assert!(reduced_motion_from_env(Some("1")));
        assert!(reduced_motion_from_env(Some("true")));
        assert!(reduced_motion_from_env(Some("YES")));
        assert!(reduced_motion_from_env(Some("on")));
        assert!(!reduced_motion_from_env(Some("0")));
        assert!(!reduced_motion_from_env(Some("false")));
        assert!(!reduced_motion_from_env(None));
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
