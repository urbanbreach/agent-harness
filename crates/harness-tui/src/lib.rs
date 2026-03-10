pub mod app;
pub mod event;
pub mod keybindings;
pub mod layout;
pub mod overlay;
pub mod theme;
pub mod ui;
mod view_model;

pub use keybindings::{Action, KeyMap};

pub use app::{surface_registry, SurfaceDescriptor, SurfaceRole};
pub use layout::FrameLayoutPlan;
pub use theme::{LiveShellLayout, LiveShellTokens, ShellGeometry, ShellGeometryTarget, Theme};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use harness_core::event::EventEnvelopeV1;
use ratatui::{backend::CrosstermBackend, Terminal};

pub use app::UiIntent;
use app::{AppState, SessionHistoryEntry};
use event::poll;

pub enum LiveUpdate {
    Event(Box<EventEnvelopeV1>),
    Status(String),
}

#[cfg(test)]
#[test]
fn replay_mode_snapshot_renders_two_pane_layout() {
    tests::module_replay_mode_snapshot_renders_two_pane_layout();
}

#[cfg(test)]
#[test]
fn diff_tab_snapshot_renders_artifact_contents() {
    tests::module_diff_tab_snapshot_renders_artifact_contents();
}

#[cfg(test)]
#[test]
fn replay_mode_never_reports_lifecycle_shell_actions() {
    let replay = app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            Some("req_replay_terminal"),
            harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(
        replay.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(!replay.lifecycle_shell_actions_visible());
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

const INTERNAL_THEME_OVERRIDE_KEY: &str = "__harness_tui_theme_override";

static NEXT_THEME_OVERRIDE_ID: AtomicU64 = AtomicU64::new(1);
static TUI_THEME_OVERRIDES: OnceLock<Mutex<std::collections::BTreeMap<String, Theme>>> =
    OnceLock::new();

fn tui_theme_overrides() -> &'static Mutex<std::collections::BTreeMap<String, Theme>> {
    TUI_THEME_OVERRIDES.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

fn store_tui_theme_override(theme: Theme) -> String {
    let id = format!(
        "theme-{}",
        NEXT_THEME_OVERRIDE_ID.fetch_add(1, Ordering::Relaxed)
    );
    tui_theme_overrides()
        .lock()
        .expect("tui theme overrides lock poisoned")
        .insert(id.clone(), theme);
    id
}

fn load_tui_theme_override(id: &str) -> Option<Theme> {
    tui_theme_overrides()
        .lock()
        .expect("tui theme overrides lock poisoned")
        .get(id)
        .copied()
}

fn clear_tui_theme_override(id: &str) {
    tui_theme_overrides()
        .lock()
        .expect("tui theme overrides lock poisoned")
        .remove(id);
}

impl TuiOptions {
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.set_theme(theme);
        self
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if let Some(previous_id) = self
            .keybindings
            .as_mut()
            .and_then(|bindings| bindings.remove(INTERNAL_THEME_OVERRIDE_KEY))
        {
            clear_tui_theme_override(&previous_id);
        }

        self.keybindings
            .get_or_insert_with(std::collections::BTreeMap::new)
            .insert(
                INTERNAL_THEME_OVERRIDE_KEY.to_string(),
                store_tui_theme_override(theme),
            );
    }

    pub fn theme(&self) -> Theme {
        self.keybindings
            .as_ref()
            .and_then(|bindings| bindings.get(INTERNAL_THEME_OVERRIDE_KEY))
            .and_then(|id| load_tui_theme_override(id))
            .unwrap_or_default()
    }

    fn take_external_keybindings(&mut self) -> Option<std::collections::BTreeMap<String, String>> {
        let mut keybindings = self.keybindings.take()?;
        if let Some(theme_override_id) = keybindings.remove(INTERNAL_THEME_OVERRIDE_KEY) {
            clear_tui_theme_override(&theme_override_id);
        }

        (!keybindings.is_empty()).then_some(keybindings)
    }
}

pub fn run_tui_with_options(mut options: TuiOptions) -> Result<()> {
    let theme = options.theme();
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
            app.set_theme(theme);
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            (app, None)
        }
        TuiMode::Replay { run_dir, events } => {
            let mut app = AppState::new_replay(run_dir, events);
            app.set_theme(theme);
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
            app.set_theme(theme);
            if let Some(bindings) = keybindings.as_ref() {
                app.apply_keybindings(bindings.clone());
            }
            for event in historical_events {
                app.ingest_historical_event(event);
            }
            (app, Some(update_rx))
        }
    };

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        EnableMouseCapture
    )?;
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
    crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )
    .context("failed to leave alternate screen after TUI")?;

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use harness_core::event::{
        ActorKind, EditAppliedEvent, EventActor, EventEnvelopeV1, EventV1,
        PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestFinishedEvent,
        ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, RunFinishedEvent,
        RunStartedEvent, TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
        SCHEMA_VERSION,
    };
    use harness_core::perm::PermissionDecision;
    use ratatui::{backend::TestBackend, Terminal};
    use tempfile::TempDir;

    #[test]
    pub(super) fn module_replay_mode_snapshot_renders_two_pane_layout() {
        let run_dir = write_replay_fixture(sample_replay_events());
        let events = load_events_from_run_dir(run_dir.path()).expect("load replay events");

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");

        let app = AppState::new_replay(run_dir.path().to_path_buf(), events);
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw replay frame");

        assert_buffer_snapshot(
            "replay_mode_snapshot_renders_two_pane_layout",
            terminal.backend().buffer(),
        );
    }

    #[test]
    fn replay_mode_r_key_marks_reload_requested() {
        let run_dir = write_replay_fixture(sample_replay_events());
        let events = load_events_from_run_dir(run_dir.path()).expect("load replay events");

        let mut app = AppState::new_replay(run_dir.path().to_path_buf(), events);
        app.handle_key(key(KeyCode::Char('r')));

        assert!(app.take_reload_requested());
    }

    #[test]
    fn live_mode_snapshot_renders_grouped_streams() {
        let mut app = AppState::new_live(None, false, None);
        for event in sample_live_events() {
            app.ingest_event(event);
        }
        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw live frame");

        assert_buffer_snapshot(
            "live_mode_snapshot_renders_grouped_streams",
            terminal.backend().buffer(),
        );
    }

    #[test]
    fn live_mode_renders_activity_and_transcript() {
        let mut app = AppState::new_live(None, false, None);
        for event in sample_live_events() {
            app.ingest_event(event);
        }
        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw live frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(
            debug.contains("hello world"),
            "live mode must center the conversation surface"
        );
        assert!(
            !debug.contains("Activity ("),
            "live mode should not render the old activity cockpit by default"
        );
        assert!(
            debug.contains("hello world"),
            "transcript must show streaming content"
        );
    }

    #[test]
    fn permission_modal_snapshot_renders_request() {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw modal frame");

        assert_buffer_snapshot(
            "permission_modal_snapshot_renders_request",
            terminal.backend().buffer(),
        );
    }

    #[test]
    fn permission_modal_a_emits_resolve_intent_and_closes_on_resolved() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let intent_sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_live(None, false, Some(intent_sink));
        app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

        app.handle_key(key(KeyCode::Char('a')));

        let intents = intents.lock().expect("lock intents");
        assert_eq!(intents.len(), 1);
        assert_eq!(
            intents[0],
            UiIntent::ResolvePermission {
                permission_id: "perm_1".to_string(),
                decision: PermissionDecision::Allow,
            }
        );
        drop(intents);

        assert!(app.active_permission().is_some());

        app.ingest_event(permission_resolved_event(
            2,
            "perm_1",
            PermissionDecision::Allow,
        ));
        assert!(app.active_permission().is_none());
    }

    #[test]
    pub(super) fn module_diff_tab_snapshot_renders_artifact_contents() {
        let run_dir = write_diff_fixture(true);
        let events = load_events_from_run_dir(run_dir.path()).expect("load diff fixture");

        let mut app = AppState::new_replay(run_dir.path().to_path_buf(), events);
        app.active_tab = app::Tab::Diff;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw diff frame");

        assert_buffer_snapshot(
            "diff_tab_snapshot_renders_artifact_contents",
            terminal.backend().buffer(),
        );
    }

    #[test]
    fn diff_tab_snapshot_handles_missing_artifact() {
        let run_dir = write_diff_fixture(false);
        let events = load_events_from_run_dir(run_dir.path()).expect("load diff fixture");

        let mut app = AppState::new_replay(run_dir.path().to_path_buf(), events);
        app.active_tab = app::Tab::Diff;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw missing diff frame");

        assert_buffer_snapshot(
            "diff_tab_snapshot_handles_missing_artifact",
            terminal.backend().buffer(),
        );
    }

    #[test]
    fn prompt_focus_enter_emits_submit_intent() {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let intent_sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().expect("lock intents").push(intent);
            })
        };

        let mut app = AppState::new_live(None, false, Some(intent_sink));
        app.focus = app::Focus::Prompt;

        for c in "hello".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }

        app.handle_key(key(KeyCode::Enter));

        let intents = intents.lock().expect("lock intents");
        assert_eq!(intents.len(), 1);
        assert_eq!(
            intents[0],
            UiIntent::SubmitPrompt {
                text: "hello".to_string()
            }
        );
        drop(intents);

        assert_eq!(app.prompt_buffer, "");
        assert_eq!(app.prompt_history.len(), 1);
        assert_eq!(app.prompt_history[0], "hello");
    }

    #[test]
    fn activity_groups_by_request_id() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_001".to_string(),
                text: "Hello AI".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello AI".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        assert_eq!(app.activities.len(), 1);
        let activity = app.activities.front().unwrap();
        assert_eq!(activity.request_id, "req_001");
        assert_eq!(activity.provider_id, "openai");
        assert_eq!(activity.model_id, "gpt-5-codex");
        assert!(activity.user_message.is_some());
        assert_eq!(activity.user_message.as_ref().unwrap().text, "Hello AI");
    }

    #[test]
    fn transcript_accumulates_stream_deltas() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "test".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "Hello ".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            3,
            Some("req_001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "world!".to_string(),
            }),
        ));

        let activity = app.activities.front().unwrap();
        assert_eq!(activity.transcript_text, "Hello world!");
    }

    #[test]
    fn activity_status_done_on_request_finished() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "test".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        assert_eq!(
            app.activities.front().unwrap().status,
            crate::app::ActivityStatus::Streaming
        );

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_001".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
            }),
        ));

        assert_eq!(
            app.activities.front().unwrap().status,
            crate::app::ActivityStatus::Done
        );
    }

    #[test]
    fn activity_status_error_on_run_failed() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "test".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            None,
            EventV1::RunFailed(RunFailedEvent {
                error: "API rate limit exceeded".to_string(),
            }),
        ));

        let activity = app.activities.front().unwrap();
        assert_eq!(activity.status, crate::app::ActivityStatus::Error);
        assert_eq!(
            activity.error_message.as_ref().unwrap(),
            "API rate limit exceeded"
        );
    }

    #[test]
    fn memory_cap_enforces_max_events() {
        let mut app = AppState::new_live(None, false, None);
        app.memory_caps.max_events = 5;

        for i in 1..=10 {
            app.ingest_event(envelope(
                i,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: format!("run-{}", i),
                    workspace_root: "/tmp".to_string(),
                }),
            ));
        }

        assert_eq!(app.events.len(), 5);
        assert_eq!(app.events_trimmed_count, 5);
    }

    #[test]
    fn memory_cap_enforces_max_transcript_chars() {
        let mut app = AppState::new_live(None, false, None);
        app.memory_caps.max_transcript_chars = 20;

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "test".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        // Add 30 characters in deltas
        for i in 0..3 {
            app.ingest_event(envelope(
                2 + i,
                Some("req_001"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_001".to_string(),
                    delta: "0123456789".to_string(),
                }),
            ));
        }

        assert!(app.transcript_trimmed_count > 0);
    }

    #[test]
    fn run_workspace_renders_activity_with_compact_format() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_000123"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000123".to_string(),
                text: "Hello".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_000123"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000123".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.handle_key(key(crossterm::event::KeyCode::Tab));
        app.handle_key(key(crossterm::event::KeyCode::Char('i')));

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw run workspace frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(
            debug.contains("req_000123"),
            "details drawer must keep request_id reachable"
        );
        assert!(
            debug.contains("gpt-5-codex"),
            "details drawer must show model_id"
        );
    }

    #[test]
    fn tool_call_requested_renders_pending_status() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_001".to_string(),
                text: "Hello".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            3,
            Some("req_001"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_001".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"test.txt"}"#.to_string(),
                args_digest: "digest-args".to_string(),
            }),
        ));

        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw tool call frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(debug.contains("fs.read"), "transcript must show tool_id");
        assert!(
            debug.contains("pending permission"),
            "transcript must show pending permission status"
        );
    }

    #[test]
    fn tool_call_started_renders_running_status() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_001".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"test.txt"}"#.to_string(),
                args_digest: "digest-args".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            3,
            Some("req_001"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_001".to_string(),
            }),
        ));

        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw tool call frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(debug.contains("fs.read"), "transcript must show tool_id");
        assert!(
            debug.contains("running"),
            "transcript must show running status"
        );
    }

    #[test]
    fn tool_call_finished_renders_truncated_output() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_001".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"test.txt"}"#.to_string(),
                args_digest: "digest-args".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            3,
            Some("req_001"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_001".to_string(),
            }),
        ));

        let long_output = "x".repeat(150);
        app.ingest_event(envelope(
            4,
            Some("req_001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_001".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(long_output.clone()),
                output_digest: Some("digest-output".to_string()),
            }),
        ));

        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw tool call frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(debug.contains("fs.read"), "transcript must show tool_id");
        assert!(
            debug.contains("succeeded"),
            "transcript must show succeeded status"
        );
        assert!(debug.contains("└"), "transcript must show output indicator");
    }

    #[test]
    fn tool_call_failed_renders_error() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_001".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-args".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            3,
            Some("req_001"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_001".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            4,
            Some("req_001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_001".to_string(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 1".to_string()),
                output_digest: None,
            }),
        ));

        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw tool call frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(debug.contains("shell.run"), "transcript must show tool_id");
        assert!(
            debug.contains("failed"),
            "transcript must show failed status"
        );
        assert!(
            debug.contains("exit code: 1"),
            "transcript must show error message"
        );
    }

    #[test]
    fn task_scheduled_queued_does_not_reuse_tool_call_id_as_task_id() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            Some("req_001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Hello".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            2,
            Some("req_001"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_001".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"test.txt"}"#.to_string(),
                args_digest: "digest-args".to_string(),
            }),
        ));

        app.ingest_event(envelope(
            3,
            Some("req_001"),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "tc_001".to_string(),
                state: TaskScheduleState::Queued,
                queue_key: Some("tool:fs.read".to_string()),
            }),
        ));

        let activity = app.activities.front().unwrap();
        let tool_call = activity.tool_calls.first().unwrap();
        assert_eq!(
            tool_call.status,
            crate::app::ToolCallDisplayStatus::PendingPermission,
            "TaskScheduled must not treat task_id as a tool_call_id"
        );

        let rows = app.orchestration_visible_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_id, "tc_001");
        assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::Queued);
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sample_replay_events() -> Vec<EventEnvelopeV1> {
        vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "replay-run".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ]
    }

    fn sample_live_events() -> Vec<EventEnvelopeV1> {
        vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "live-run".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                Some("req_1"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "summarized prompt".to_string(),
                    request_digest: "digest-req-1".to_string(),
                }),
            ),
            envelope(
                3,
                Some("req_1"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_1".to_string(),
                    delta: "hello ".to_string(),
                }),
            ),
            envelope(
                4,
                Some("req_1"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_1".to_string(),
                    delta: "world".to_string(),
                }),
            ),
            envelope(
                5,
                Some("req_1"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_1".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-output".to_string()),
                }),
            ),
            permission_requested_event(6, "perm_1", "tool_call_1"),
            permission_resolved_event(7, "perm_1", PermissionDecision::Allow),
            envelope(
                8,
                Some("tool_call_1"),
                EventV1::EditApplied(EditAppliedEvent {
                    edit_id: "edit_1".to_string(),
                    path: "demo.txt".to_string(),
                    new_file_digest: "digest-new-file".to_string(),
                    diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
                    diff_digest: Some("diff-digest".to_string()),
                }),
            ),
            envelope(
                9,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ]
    }

    fn permission_requested_event(
        seq: u64,
        permission_id: &str,
        tool_call_id: &str,
    ) -> EventEnvelopeV1 {
        envelope(
            seq,
            Some(tool_call_id),
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: permission_id.to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some(tool_call_id.to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        )
    }

    fn permission_resolved_event(
        seq: u64,
        permission_id: &str,
        decision: PermissionDecision,
    ) -> EventEnvelopeV1 {
        envelope(
            seq,
            Some("tool_call_1"),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: permission_id.to_string(),
                decision: match decision {
                    PermissionDecision::Allow => harness_core::event::PermissionDecision::Allow,
                    PermissionDecision::Deny => harness_core::event::PermissionDecision::Deny,
                },
                reason: Some("resolved in test".to_string()),
            }),
        )
    }

    fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
        envelope_with_actor(
            seq,
            correlation_id,
            EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            payload,
        )
    }

    fn envelope_with_actor(
        seq: u64,
        correlation_id: Option<&str>,
        actor: EventActor,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_fixture".to_string(),
            mono_ms: seq,
            ts: None,
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload,
        }
    }

    fn write_replay_fixture(events: Vec<EventEnvelopeV1>) -> TempDir {
        let run_dir = tempfile::tempdir().expect("create temp run dir");
        write_events_jsonl(run_dir.path(), &events);
        run_dir
    }

    fn write_diff_fixture(with_diff_file: bool) -> TempDir {
        let run_dir = tempfile::tempdir().expect("create temp run dir");

        if with_diff_file {
            let artifacts_dir = run_dir.path().join("artifacts");
            fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
            fs::write(
                artifacts_dir.join("edit-edit-golden-path.diff"),
                "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n",
            )
            .expect("write diff fixture");
        }

        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "diff-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                Some("tool_call_1"),
                EventV1::EditApplied(EditAppliedEvent {
                    edit_id: "edit-golden-path".to_string(),
                    path: "demo.txt".to_string(),
                    new_file_digest: "digest".to_string(),
                    diff_rel_path: Some("artifacts/edit-edit-golden-path.diff".to_string()),
                    diff_digest: Some("digest-diff".to_string()),
                }),
            ),
            envelope(
                3,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ];

        write_events_jsonl(run_dir.path(), &events);
        run_dir
    }

    fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).expect("serialize event"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events jsonl");
    }

    fn assert_buffer_snapshot(name: &str, buffer: &ratatui::buffer::Buffer) {
        let normalized = normalize_temp_paths(&format!("{buffer:#?}"));
        insta::assert_snapshot!(name, normalized);
    }

    fn normalize_temp_paths(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut output = String::with_capacity(input.len());
        let mut index = 0;

        while index < bytes.len() {
            if bytes[index..].starts_with(b"/tmp/.tmp") {
                output.push_str("/tmp/TMPDIR");
                index += b"/tmp/.tmp".len();
                while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
                    index += 1;
                }
            } else {
                output.push(bytes[index] as char);
                index += 1;
            }
        }

        output
    }
}

#[cfg(test)]
#[test]
fn default_theme_is_harness_app_dark() {
    let default = Theme::default();
    let harness = Theme::harness_app_dark();

    assert_eq!(default.surface, harness.surface);
    assert_eq!(default.border, harness.border);
    assert_eq!(default.text, harness.text);
    assert_eq!(default.status, harness.status);
}

#[cfg(test)]
#[test]
fn theme_tokens_cover_live_shell_states() {
    let default = Theme::default();
    let mono = Theme::mono();
    let tokens = default.token_families();

    assert_eq!(default.live_shell.glyphs.streaming, "◐");
    assert_eq!(default.live_shell.glyphs.done, "●");
    assert_eq!(default.live_shell.glyphs.error, "✗");
    assert_eq!(default.live_shell.glyphs.pending_permission, "◷");
    assert_eq!(default.live_shell.glyphs.queued, "◴");
    assert_eq!(default.live_shell.glyphs.running, "◐");
    assert_eq!(default.live_shell.glyphs.succeeded, "●");
    assert_eq!(default.live_shell.glyphs.failed, "✗");
    assert_eq!(tokens.live_shell.glyphs.ascii.status.streaming, "o");
    assert_eq!(
        tokens.live_shell.glyphs.ascii.status.pending_permission,
        "?"
    );
    assert_eq!(tokens.live_shell.glyphs.ascii.status.failed, "x");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.user_marker, ">");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.card_top, "+-");

    assert_eq!(default.live_shell.heights.header, 1);
    assert_eq!(default.live_shell.heights.tabs, 3);
    assert_eq!(default.live_shell.heights.status, 1);
    assert_eq!(default.live_shell.heights.footer, 1);
    assert_eq!(default.live_shell.heights.prompt_block(), 5);
    assert_eq!(default.live_shell.rhythm.transcript_gutter_x, 1);
    assert_eq!(default.live_shell.rhythm.status_separator, 2);
    assert_eq!(default.live_shell.minimum.centered_content_width, 78);
    assert_eq!(default.live_shell.minimum.content_margin_x, 1);
    assert_eq!(default.live_shell.primary.centered_content_width, 92);
    assert_eq!(default.live_shell.primary.content_margin_x, 2);
    assert_eq!(tokens.palette.surfaces, default.surface);
    assert_eq!(tokens.palette.borders, default.border);
    assert_eq!(
        tokens.live_shell.geometry.breakpoints,
        crate::theme::ShellBreakpoints::DEFAULT
    );
    assert_eq!(
        tokens.live_shell.geometry.minimum,
        default.live_shell.minimum
    );
    assert_eq!(
        tokens.live_shell.spacing.heights,
        default.live_shell.heights
    );
    assert_eq!(tokens.live_shell.spacing.rhythm, default.live_shell.rhythm);
    assert_eq!(tokens.live_shell.copy.startup, default.live_shell.startup);
    assert_eq!(
        tokens.live_shell.copy.empty_state,
        default.live_shell.empty_state
    );
    assert_eq!(
        mono.live_shell.glyphs.failed,
        default.live_shell.glyphs.failed
    );
    assert_eq!(
        default.live_shell.primary.target,
        ShellGeometryTarget::Primary
    );
    assert_eq!(
        default.live_shell.minimum.target,
        ShellGeometryTarget::Minimum
    );
}

#[cfg(test)]
#[test]
fn theme_roundtrips_through_tui_options() {
    let (_tx, rx) = mpsc::channel();
    let theme = Theme::mono();
    let options = TuiOptions {
        mode: TuiMode::Live {
            run_dir: PathBuf::from("."),
            historical_events: Vec::new(),
            update_rx: rx,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
    }
    .with_theme(theme);

    assert_eq!(options.theme(), theme);
}

#[cfg(test)]
#[test]
fn command_palette_state_filters_existing_commands() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "d");
    assert_eq!(app.palette_cursor, 1);
    assert_eq!(
        app.palette_filtered,
        vec!["details".to_string(), "diff".to_string()]
    );
    assert!(app.palette_filtered.iter().all(|command| {
        Action::palette_commands()
            .iter()
            .any(|(existing, _)| existing == command)
    }));
}

#[cfg(test)]
#[test]
fn hovered_wheel_target_uses_layout_plan() {
    let area = ratatui::layout::Rect::new(0, 0, 140, 40);

    let mut default_app = app::AppState::new_live(None, false, None);
    default_app.active_tab = app::Tab::Details;

    let mut themed_app = app::AppState::new_live(None, false, None);
    themed_app.active_tab = app::Tab::Details;
    let mut custom_theme = Theme::default();
    custom_theme.live_shell.primary.centered_content_width = 72;
    custom_theme.live_shell.primary.content_margin_x = 10;
    custom_theme.live_shell.primary.activity_drawer_width = 18;
    custom_theme.live_shell.primary.details_sidebar_width = 36;
    themed_app.set_theme(custom_theme);

    let themed_plan = layout::FrameLayoutPlan::for_app(&themed_app, area);
    let themed_inspector =
        layout::details_drawer_areas(themed_plan.details_overlay.expect("themed details area"))[1];
    let probe_column = themed_inspector.x.saturating_add(2);
    let probe_row = themed_inspector.y.saturating_add(1);

    let default_target = ui::hovered_wheel_target(&default_app, area, probe_column, probe_row);
    let themed_target = ui::hovered_wheel_target(&themed_app, area, probe_column, probe_row);

    assert_ne!(default_target, themed_target);
    assert_eq!(themed_target, Some(ui::WheelTarget::Inspector));
}

#[cfg(test)]
#[test]
fn layout_plan_minimum_geometry_matches_shell_contract() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 80, 24));

    assert_eq!(plan.root, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(plan.header, ratatui::layout::Rect::new(0, 0, 80, 1));
    assert_eq!(plan.content, ratatui::layout::Rect::new(0, 1, 80, 22));
    assert_eq!(plan.shell, ratatui::layout::Rect::new(1, 1, 78, 22));
    assert_eq!(plan.footer, ratatui::layout::Rect::new(0, 23, 80, 1));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(1, 1, 78, 18))
    );
    assert_eq!(plan.status, Some(ratatui::layout::Rect::new(1, 19, 78, 1)));
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(1, 20, 78, 3))
    );
}

#[cfg(test)]
#[test]
fn layout_plan_primary_geometry_matches_shell_contract() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));

    assert_eq!(plan.root, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.header, ratatui::layout::Rect::new(0, 0, 100, 1));
    assert_eq!(plan.content, ratatui::layout::Rect::new(0, 1, 100, 28));
    assert_eq!(plan.shell, ratatui::layout::Rect::new(2, 1, 96, 28));
    assert_eq!(plan.footer, ratatui::layout::Rect::new(0, 29, 100, 1));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(2, 1, 96, 24))
    );
    assert_eq!(plan.status, Some(ratatui::layout::Rect::new(2, 25, 96, 1)));
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 26, 96, 3))
    );
}

#[cfg(test)]
#[test]
fn wide_primary_live_layout_uses_available_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;

    let area = ratatui::layout::Rect::new(0, 0, 160, 40);
    let theme = app.theme();
    let shell_layout = theme.live_shell_layout(area.width, area.height);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);

    assert_eq!(shell_layout.target, ShellGeometryTarget::Primary);
    assert_eq!(plan.shell.x, shell_layout.content_margin_x);
    assert_eq!(
        plan.shell.width,
        area.width
            .saturating_sub(shell_layout.content_margin_x.saturating_mul(2))
    );
    assert!(plan.shell.width > shell_layout.centered_content_width);
}

#[cfg(test)]
#[test]
fn split_window_live_layout_uses_available_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;

    let area = ratatui::layout::Rect::new(0, 0, 96, 40);
    let theme = app.theme();
    let shell_layout = theme.live_shell_layout(area.width, area.height);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);

    assert_eq!(shell_layout.target, ShellGeometryTarget::Split);
    assert_eq!(plan.shell.x, shell_layout.content_margin_x);
    assert_eq!(
        plan.shell.width,
        area.width
            .saturating_sub(shell_layout.content_margin_x.saturating_mul(2))
    );
    assert!(plan.shell.width > shell_layout.centered_content_width);
}

#[cfg(test)]
#[test]
fn live_layout_breakpoints_choose_shell_variant() {
    let theme = Theme::default();

    let minimum = theme.live_shell_layout(80, 24);
    assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
    assert_eq!(minimum.activity_drawer_width, 20);
    assert_eq!(minimum.inspector_drawer_width, 20);
    assert_eq!(minimum.details_sidebar_width, 34);
    assert_eq!(minimum.transcript_min_width, 28);
    assert_eq!(minimum.centered_content_width, 78);

    let split = theme.live_shell_layout(96, 40);
    assert_eq!(split.target, ShellGeometryTarget::Split);
    assert_eq!(split.activity_drawer_width, 18);
    assert_eq!(split.inspector_drawer_width, 24);
    assert_eq!(split.details_sidebar_width, 32);
    assert_eq!(split.transcript_min_width, 32);
    assert_eq!(split.centered_content_width, 88);

    let primary = theme.live_shell_layout(100, 30);
    assert_eq!(primary.target, ShellGeometryTarget::Primary);
    assert_eq!(primary.activity_drawer_width, 24);
    assert_eq!(primary.inspector_drawer_width, 28);
    assert_eq!(primary.details_sidebar_width, 40);
    assert_eq!(primary.transcript_min_width, 40);
    assert_eq!(primary.centered_content_width, 92);

    assert_eq!(
        theme.live_shell.target(89, 40),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(
        theme.live_shell.target(90, 35),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(theme.live_shell.target(90, 36), ShellGeometryTarget::Split);
    assert_eq!(
        theme.live_shell.target(99, 30),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(theme.live_shell.target(99, 40), ShellGeometryTarget::Split);
    assert_eq!(
        theme.live_shell.target(100, 29),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(
        theme.live_shell.target(100, 30),
        ShellGeometryTarget::Primary
    );
}

#[cfg(test)]
#[test]
fn session_view_tracks_request_turn_and_tool_state() {
    let events = session_view_events();

    let mut live = app::AppState::new_live(None, false, None);
    for event in events.clone() {
        live.ingest_event(event);
    }
    assert_session_view_state(&live);

    let replay = app::AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), events);
    assert_session_view_state(&replay);
}

#[cfg(test)]
#[test]
fn session_view_ignores_duplicate_seq_without_losing_ui_state() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));
    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(app.active_permission().is_none());

    app.focus = app::Focus::Prompt;
    for c in "draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(envelope(
        1,
        Some("req_duplicate"),
        harness_core::event::EventV1::RunStarted(harness_core::event::RunStartedEvent {
            run_name: "duplicate-seq".to_string(),
            workspace_root: "/tmp".to_string(),
        }),
    ));

    assert_eq!(app.events.len(), 1);
    assert!(app.active_permission().is_none());
    assert_eq!(app.prompt_buffer, "draft");
    assert_eq!(app.prompt_cursor, "draft".chars().count());
}

#[cfg(test)]
#[test]
fn orchestration_projection_resolves_owner_labels() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));

    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        None,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_supervisor".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:supervisor".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        None,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_system".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("tool:shell.run".to_string()),
        }),
    ));

    let summary = app.orchestration_summary();
    assert_eq!(
        summary,
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 2,
            running: 1,
            stale: 0,
        }
    );

    let rows = app.orchestration_visible_rows();
    assert_eq!(
        rows.iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_supervisor", "task_system", "task_worker"]
    );

    let worker = rows
        .iter()
        .find(|row| row.task_id == "task_worker")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(worker),
        crate::app::OrchestrationOwnerLabels {
            label: "agent_worker".to_string(),
            profile: "researcher".to_string(),
        }
    );

    let supervisor = rows
        .iter()
        .find(|row| row.task_id == "task_supervisor")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(supervisor),
        crate::app::OrchestrationOwnerLabels {
            label: "supervisor".to_string(),
            profile: "n/a".to_string(),
        }
    );

    let system = rows
        .iter()
        .find(|row| row.task_id == "task_system")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(system),
        crate::app::OrchestrationOwnerLabels {
            label: "system".to_string(),
            profile: "n/a".to_string(),
        }
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_ignores_duplicate_seq_events() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_dup".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_dup".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    assert_eq!(app.orchestration_visible_rows().len(), 1);
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("stale for 3001 ms")
    );

    app.ingest_event(envelope_with_actor(
        1,
        None,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "rewritten".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_dup".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_dup".to_string(),
            stale_for_ms: 9999,
        }),
    ));

    assert_eq!(app.events.len(), 3);
    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::Stale);
    assert_eq!(rows[0].queue_key.as_deref(), Some("agent:running"));
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("stale for 3001 ms")
    );
    assert_eq!(
        app.orchestration_owner_labels(&rows[0]),
        crate::app::OrchestrationOwnerLabels {
            label: "agent_worker".to_string(),
            profile: "researcher".to_string(),
        }
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_tracks_queued_started_completed_counts() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));

    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_primary".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:primary".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 0,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_worker_primary",
            Some("agent:queued:primary"),
            None,
            crate::app::OrchestrationTaskState::Queued,
        )
    );

    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_primary".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:primary".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 1,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_worker_primary",
            Some("agent:running:primary"),
            None,
            crate::app::OrchestrationTaskState::Running,
        )
    );

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_worker_secondary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_secondary".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:secondary".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 1,
            stale: 0,
        },
        "active_agents must count unique worker owners only"
    );
    assert_eq!(
        app.orchestration_visible_rows()
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_worker_primary", "task_worker_secondary"]
    );

    app.ingest_event(envelope_with_actor(
        5,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_worker_primary".to_string(),
            result_summary: "primary completed".to_string(),
            result_digest: "digest-primary".to_string(),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 0,
            stale: 0,
        }
    );

    app.ingest_event(envelope_with_actor(
        6,
        Some("req_worker_secondary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_worker_secondary".to_string(),
            result_summary: "secondary completed".to_string(),
            result_digest: "digest-secondary".to_string(),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 0,
            stale: 0,
        }
    );
    assert_eq!(
        app.orchestration_visible_rows()
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_worker_secondary", "task_worker_primary"]
    );

    app.ingest_event(envelope_with_actor(
        7,
        None,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_supervisor_only".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:supervisor".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 1,
            stale: 0,
        },
        "non-worker rows must not contribute to active_agents"
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_tracks_stale_then_late_result() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:stale".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 1,
            stale: 0,
        }
    );

    app.ingest_event(envelope_with_actor(
        3,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_stale".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_stale",
            Some("agent:running:stale"),
            Some("stale for 3001 ms"),
            crate::app::OrchestrationTaskState::Stale,
        )
    );

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskResultLate(harness_core::event::TaskResultLateEvent {
            task_id: "task_stale".to_string(),
            result_digest: "digest-late".to_string(),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 0,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(
        rows.len(),
        1,
        "late result must update the stale row in place"
    );
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_stale",
            Some("agent:running:stale"),
            Some("late result after stale cancellation"),
            crate::app::OrchestrationTaskState::LateResult,
        )
    );
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("late result after stale cancellation")
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_retains_only_recent_terminal_rows() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_live_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:live".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_live_stale".to_string(),
            stale_for_ms: 4242,
        }),
    ));
    app.ingest_event(envelope(
        3,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_live_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:live".to_string()),
        }),
    ));

    app.ingest_event(envelope(
        4,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_1".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q1".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        5,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_1".to_string(),
            result_summary: "terminal 1 completed".to_string(),
            result_digest: "digest-terminal-1".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        6,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_2".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q2".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        7,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_terminal_2".to_string(),
            reason: "cancelled 2".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        8,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_3".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q3".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        9,
        None,
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_terminal_3".to_string(),
            stale_for_ms: 9003,
        }),
    ));
    app.ingest_event(envelope(
        10,
        None,
        harness_core::event::EventV1::TaskResultLate(harness_core::event::TaskResultLateEvent {
            task_id: "task_terminal_3".to_string(),
            result_digest: "digest-terminal-3".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        11,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_4".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q4".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        12,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_4".to_string(),
            result_summary: "terminal 4 completed".to_string(),
            result_digest: "digest-terminal-4".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        13,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_5".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q5".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        14,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_terminal_5".to_string(),
            reason: "cancelled 5".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        15,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_6".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q6".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        16,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_6".to_string(),
            result_summary: "terminal 6 completed".to_string(),
            result_digest: "digest-terminal-6".to_string(),
        }),
    ));

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 7);
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.task_id.as_str(),
                row.queue_key.as_deref(),
                row.warning.as_deref(),
                row.state,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "task_live_stale",
                Some("agent:running:live"),
                Some("stale for 4242 ms"),
                crate::app::OrchestrationTaskState::Stale,
            ),
            (
                "task_live_queued",
                Some("agent:queued:live"),
                None,
                crate::app::OrchestrationTaskState::Queued,
            ),
            (
                "task_terminal_6",
                Some("terminal:q6"),
                None,
                crate::app::OrchestrationTaskState::Completed,
            ),
            (
                "task_terminal_5",
                Some("terminal:q5"),
                Some("cancelled 5"),
                crate::app::OrchestrationTaskState::Cancelled,
            ),
            (
                "task_terminal_4",
                Some("terminal:q4"),
                None,
                crate::app::OrchestrationTaskState::Completed,
            ),
            (
                "task_terminal_3",
                Some("terminal:q3"),
                Some("late result after stale cancellation"),
                crate::app::OrchestrationTaskState::LateResult,
            ),
            (
                "task_terminal_2",
                Some("terminal:q2"),
                Some("cancelled 2"),
                crate::app::OrchestrationTaskState::Cancelled,
            ),
        ]
    );
    assert!(
        !rows.iter().any(|row| row.task_id == "task_terminal_1"),
        "terminal retention must drop the oldest terminal row once six exist"
    );
}

#[cfg(test)]
fn session_view_events() -> Vec<harness_core::event::EventEnvelopeV1> {
    vec![
        envelope(
            1,
            Some("req_001"),
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_001".to_string(),
                    text: "Explain the refactor".to_string(),
                },
            ),
        ),
        envelope(
            2,
            Some("req_001"),
            harness_core::event::EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_001".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5-codex".to_string(),
                    prompt_summary: "Explain the refactor".to_string(),
                    request_digest: "digest-req-001".to_string(),
                },
            ),
        ),
        envelope(
            3,
            Some("req_001"),
            harness_core::event::EventV1::ProviderStreamDelta(
                harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_001".to_string(),
                    delta: "Working through the steps.".to_string(),
                },
            ),
        ),
        envelope(
            4,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallRequested(
                harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                    tool_id: "fs.read".to_string(),
                    args_summary: r#"{"path":"src/app.rs"}"#.to_string(),
                    args_digest: "digest-tool-args".to_string(),
                },
            ),
        ),
        permission_requested_event(5, "perm_1", "tool_call_1"),
        permission_resolved_event(6, "perm_1", harness_core::perm::PermissionDecision::Allow),
        envelope(
            7,
            Some("req_001"),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "tool_call_1".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("tool:fs.read".to_string()),
            }),
        ),
        envelope(
            8,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallStarted(
                harness_core::event::ToolCallStartedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                },
            ),
        ),
        envelope(
            9,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallFinished(
                harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("tool output".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                },
            ),
        ),
        envelope(
            10,
            Some("req_001"),
            harness_core::event::EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: "req_001".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-final".to_string()),
                },
            ),
        ),
    ]
}

#[cfg(test)]
fn orchestration_details_drawer_events(extra_terminal_rows: usize) -> Vec<EventEnvelopeV1> {
    let mut events = session_view_events();
    events.extend([
        envelope(
            11,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "w1".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            12,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "w2".to_string(),
                profile: "scout".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            13,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w1".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_stale".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some("scan".to_string()),
            }),
        ),
        envelope_with_actor(
            14,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w1".to_string()),
            ),
            harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
                task_id: "task_stale".to_string(),
                stale_for_ms: 3001,
            }),
        ),
        envelope_with_actor(
            15,
            Some("req_001"),
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_run".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: None,
            }),
        ),
        envelope_with_actor(
            16,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                Some("coordinator".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_queue".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("tool:read".to_string()),
            }),
        ),
        envelope_with_actor(
            17,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_done".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some("tool:done".to_string()),
            }),
        ),
        envelope_with_actor(
            18,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id: "task_done".to_string(),
                result_summary: "done".to_string(),
                result_digest: "digest-task-done".to_string(),
            }),
        ),
    ]);

    let mut seq = 19;
    for index in 0..extra_terminal_rows {
        let task_id = format!("task_tail_{index}");
        events.push(envelope_with_actor(
            seq,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: task_id.clone(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some(format!("tail:{index}")),
            }),
        ));
        seq += 1;
        events.push(envelope_with_actor(
            seq,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id,
                result_summary: format!("tail {index} done"),
                result_digest: format!("digest-tail-{index}"),
            }),
        ));
        seq += 1;
    }

    events
}

#[cfg(test)]
fn orchestration_details_drawer_app(extra_terminal_rows: usize) -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    for event in orchestration_details_drawer_events(extra_terminal_rows) {
        app.ingest_event(event);
    }
    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));
    app
}

#[cfg(test)]
fn assert_session_view_state(app: &app::AppState) {
    assert_eq!(app.activities.len(), 1);

    let activity = app.activities.front().expect("activity exists");
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.provider_id, "openai");
    assert_eq!(activity.model_id, "gpt-5-codex");
    assert_eq!(activity.status, app::ActivityStatus::Done);
    assert_eq!(activity.transcript_text, "Working through the steps.");
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("Explain the refactor")
    );

    assert_eq!(activity.tool_calls.len(), 1);
    let tool_call = activity.tool_calls.first().expect("tool call exists");
    assert_eq!(tool_call.tool_call_id, "tool_call_1");
    assert_eq!(tool_call.tool_id, "fs.read");
    assert_eq!(tool_call.status, app::ToolCallDisplayStatus::Succeeded);
    assert_eq!(tool_call.output_summary.as_deref(), Some("tool output"));
    assert_eq!(tool_call.truncated_output.as_deref(), Some("tool output"));

    assert!(app.active_permission().is_none());
}

#[cfg(test)]
fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[cfg(test)]
fn key_with_modifiers(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[cfg(test)]
fn render_live_buffer(app: &app::AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("draw frame");
    format!("{:?}", terminal.backend().buffer())
}

#[cfg(test)]
fn render_live_screen(app: &app::AppState, width: u16, height: u16) -> String {
    let debug = render_live_buffer(app, width, height);
    let mut in_content = false;
    let mut rows = Vec::new();

    for line in debug.lines() {
        if line.trim() == "content: [" {
            in_content = true;
            continue;
        }
        if in_content && line.trim() == "]," {
            break;
        }
        if !in_content {
            continue;
        }

        let trimmed = line.trim();
        if let Some(content) = trimmed
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix("\","))
        {
            rows.push(content.to_string());
        }
    }

    rows.join("\n")
}

#[cfg(test)]
#[test]
fn permission_modal_snapshot_renders_request() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission Requested",
            "FAIL CLOSED",
            "default deny",
            "[d] deny",
        ],
    );
}

#[cfg(test)]
#[test]
fn overlay_stack_orders_details_palette_permission() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Details;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[
            overlay::OverlayKind::DetailsDrawer,
            overlay::OverlayKind::CommandPalette,
        ]
    );

    app.ingest_event(permission_requested_event(
        1,
        "perm_stack_order",
        "tool_call_stack_order",
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[
            overlay::OverlayKind::DetailsDrawer,
            overlay::OverlayKind::PermissionModal,
        ]
    );
}

#[cfg(test)]
#[test]
fn permission_modal_preempts_palette() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette",
        "tool_call_preempt_palette",
    ));

    app.handle_key(key(crossterm::event::KeyCode::Char('a')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "d");
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::PermissionModal)
    );

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_preempt_palette".to_string(),
            decision: harness_core::perm::PermissionDecision::Allow,
        }]
    );
}

#[cfg(test)]
#[test]
fn focus_returns_after_palette_close() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.prompt_buffer = "keep prompt draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    assert!(app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");
    let open_debug = render_live_screen(&app, 100, 24);
    println!("PALETTE_OPEN\n{open_debug}");

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");
    assert_eq!(app.prompt_cursor, "keep prompt draft".chars().count());
    let closed_debug = render_live_screen(&app, 100, 24);
    println!("PALETTE_CLOSED\n{closed_debug}");
}

#[cfg(test)]
#[test]
fn live_status_strip_distinguishes_terminal_states() {
    let ready = app::AppState::new_live(None, false, None);
    let ready_debug = render_live_buffer(&ready, 80, 24);
    assert!(ready_debug.contains("Ready"));
    assert!(ready_debug.contains("ready for first turn"));

    let mut sending = app::AppState::new_live(None, false, None);
    for c in "hello".chars() {
        sending.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    sending.handle_key(key(crossterm::event::KeyCode::Enter));

    let sending_debug = render_live_buffer(&sending, 80, 24);
    assert!(sending_debug.contains("Sending"));
    assert!(sending_debug.contains("waiting for first tokens"));

    sending.ingest_event(envelope(
        1,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_phase".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "hello".to_string(),
                request_digest: "digest-phase".to_string(),
            },
        ),
    ));
    sending.ingest_event(envelope(
        2,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_phase".to_string(),
                delta: "streaming text".to_string(),
            },
        ),
    ));

    let streaming_debug = render_live_buffer(&sending, 80, 24);
    assert!(streaming_debug.contains("Streaming"));
    assert!(streaming_debug.contains("receiving output"));

    sending.ingest_event(envelope(
        3,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_phase".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
            },
        ),
    ));

    let ready_after_turn_debug = render_live_buffer(&sending, 80, 24);
    assert!(ready_after_turn_debug.contains("Success"));
    assert!(ready_after_turn_debug.contains("ready for next turn"));

    let mut cancelled = app::AppState::new_live(None, false, None);
    cancelled.ingest_event(envelope(
        1,
        Some("req_cancel"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_cancel".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "cancel".to_string(),
                request_digest: "digest-cancel".to_string(),
            },
        ),
    ));
    cancelled.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "req_cancel".to_string(),
            reason: "operator cancelled".to_string(),
        }),
    ));
    let cancelled_debug = render_live_buffer(&cancelled, 80, 24);
    assert!(cancelled_debug.contains("Cancelled"));
    assert!(cancelled_debug.contains("operator cancelled"));

    let mut errored = app::AppState::new_live(None, false, None);
    errored.ingest_event(envelope(
        1,
        Some("req_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "fail".to_string(),
                request_digest: "digest-error".to_string(),
            },
        ),
    ));
    errored.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "API rate limit exceeded".to_string(),
        }),
    ));
    let error_debug = render_live_buffer(&errored, 80, 24);
    assert!(error_debug.contains("Failure"));
    assert!(error_debug.contains("inspect transcript"));
    assert!(error_debug.contains("API rate limit exceeded"));
    assert!(error_debug.contains("adjust the draft"));

    let mut permission_blocked = app::AppState::new_live(None, false, None);
    permission_blocked.ingest_event(permission_requested_event(1, "perm_blocked", "tool_call_1"));
    let permission_blocked_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_blocked_debug.contains("Permission blocked"));
    assert!(permission_blocked_debug.contains("Draft preserved under the permission checkpoint"));
    assert!(permission_blocked_debug.contains("FAIL CLOSED"));

    permission_blocked.handle_key(key(crossterm::event::KeyCode::Char('a')));
    let permission_pending_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_pending_debug.contains("Permission pending"));
    assert!(permission_pending_debug.contains("awaiting confirmation"));

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    let degraded_debug = render_live_buffer(&degraded, 80, 24);
    assert!(degraded_debug.contains("Degraded"));
    assert!(degraded_debug.contains("replaying from seq 1"));
    assert!(degraded_debug.contains("Composer · disabled · Degraded"));
    assert!(degraded_debug.contains("Draft preserved locally"));
    assert!(degraded_debug.contains("Sending paused"));

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    let disconnected_debug = render_live_buffer(&disconnected, 80, 24);
    assert!(disconnected_debug.contains("Disconnected"));
    assert!(disconnected_debug.contains("Composer · disabled · Disconnected"));
}

#[cfg(test)]
#[test]
fn run_finished_keeps_transcript_and_ready_composer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_done".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "finished".to_string(),
                request_digest: "digest-done".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_done"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_done".to_string(),
                delta: "transcript remains visible".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_done".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-done-out".to_string()),
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    insta::assert_snapshot!("live_shell_finished_state", render_live_lines(&app, 80, 24));
}

#[cfg(test)]
#[test]
fn streaming_transcript_auto_scrolls_to_latest_wrapped_content() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_scroll"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_scroll".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "scroll test".to_string(),
                request_digest: "digest-scroll".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_scroll"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_scroll".to_string(),
                delta: [
                    "HEADTOKEN",
                    "alpha",
                    "beta",
                    "gamma",
                    "delta",
                    "epsilon",
                    "zeta",
                    "eta",
                    "theta",
                    "iota",
                    "kappa",
                    "lambda",
                    "mu",
                    "nu",
                    "xi",
                    "omicron",
                    "pi",
                    "rho",
                    "sigma",
                    "tau",
                    "upsilon",
                    "phi",
                    "chi",
                    "psi",
                    "TAILTOKEN",
                ]
                .join(" "),
            },
        ),
    ));

    let debug = render_live_buffer(&app, 38, 10);
    assert!(
        debug.contains("TAILTOKEN"),
        "auto-follow should keep the latest wrapped transcript content visible: {debug}"
    );
}

#[cfg(test)]
#[test]
fn disconnected_stream_disables_composer_with_reopen_guidance() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_disconnect"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_disconnect".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "disconnect".to_string(),
                request_digest: "digest-disconnect".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_disconnect"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_disconnect".to_string(),
                delta: "transcript stays visible".to_string(),
            },
        ),
    ));
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(app.prompt_buffer.is_empty());
    assert!(debug.contains("transcript stays visible"));
    assert!(debug.contains("Disconnected"));
    assert!(debug.contains("Composer · disabled · Disconnected"));
    assert!(debug.contains("reopen the TUI to reconnect"));
    assert!(debug.contains("Draft preserved locally"));
}

#[cfg(test)]
#[test]
fn transcript_renders_inline_tool_states_and_prompt_echo() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "Inspect src/ui.rs".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    app.ingest_event(envelope(
        1,
        Some("req_inline"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_inline".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Inspect src/ui.rs".to_string(),
                request_digest: "digest-inline".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_inline"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_inline".to_string(),
                delta: "Drafting a plan".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-inline-args".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_inline".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_inline".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("exit code: 1".to_string()),
                output_digest: None,
            },
        ),
    ));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Inspect src/ui.rs"));
    assert!(debug.contains("Drafting a plan"));
    assert!(debug.contains("shell.run"));
    assert!(debug.contains("failed"));
    assert!(debug.contains("exit code: 1"));
    assert!(!debug.contains("args {"));
    assert!(!debug.contains(r#"{"cmd":"false"}"#));
}

#[cfg(test)]
#[test]
fn transcript_tool_rows_keep_status_but_not_raw_json_dump() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_compact"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_tool_compact".to_string(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_compact".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-tool-compact".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_compact".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#.to_string(),
                args_digest: "digest-tool-compact-args".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_compact".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_compact".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("12 lines read".to_string()),
                output_digest: Some("digest-tool-compact-output".to_string()),
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("tool fs.read"));
    assert!(transcript.contains("12 lines read"));
    assert!(transcript.contains("succeeded"));
    assert!(!transcript.contains(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#));
    assert!(!transcript.contains("args {"));
}

#[cfg(test)]
#[test]
fn failed_tool_rows_still_surface_error_summary() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Run the command".to_string(),
                request_digest: "digest-tool-error".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_error".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false","cwd":"/tmp/demo"}"#.to_string(),
                args_digest: "digest-tool-error-args".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_error".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_error".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: permission denied".to_string()),
                output_digest: None,
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("tool shell.run"));
    assert!(transcript.contains("exit code: 1 stderr: permission denied"));
    assert!(transcript.contains("failed"));
    assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
    assert!(!transcript.contains("args {"));
}

#[cfg(test)]
#[test]
fn permission_overlay_preserves_draft_and_transcript_context() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay",
        "tool_call_overlay",
    ));

    let debug = render_live_buffer(&app, 80, 24);
    // Composer is disabled when permission is pending, showing hint instead of draft
    assert!(debug.contains("Composer · disabled · Permission blocked"));
    assert!(debug.contains("Permission Requested"));
    assert!(!debug.contains("Select an activity to view transcript"));
    assert!(
        debug.matches("Apply hashline edit to demo.txt").count() >= 2,
        "permission summary should remain visible in both transcript and modal"
    );
}

#[cfg(test)]
fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: permission_id.to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some(tool_call_id.to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    )
}

#[cfg(test)]
fn permission_resolved_event(
    seq: u64,
    permission_id: &str,
    decision: harness_core::perm::PermissionDecision,
) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        Some("tool_call_1"),
        harness_core::event::EventV1::PermissionResolved(
            harness_core::event::PermissionResolvedEvent {
                permission_id: permission_id.to_string(),
                decision: match decision {
                    harness_core::perm::PermissionDecision::Allow => {
                        harness_core::event::PermissionDecision::Allow
                    }
                    harness_core::perm::PermissionDecision::Deny => {
                        harness_core::event::PermissionDecision::Deny
                    }
                },
                reason: Some("resolved in test".to_string()),
            },
        ),
    )
}

#[cfg(test)]
fn startup_session_entry(
    run_id: &str,
    run_dir: &str,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    startup_session_entry_with_details(
        run_id,
        run_dir,
        &format!("run-{run_id}"),
        None,
        None,
        "default",
        "openai/gpt-5.3-codex",
        is_resumable,
        resume_disabled_reason,
    )
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps session-history fixture fields explicit at call sites"
)]
fn startup_session_entry_with_details(
    run_id: &str,
    run_dir: &str,
    run_name: &str,
    status: Option<harness_core::proj::RunStatus>,
    last_updated_at: Option<&str>,
    profile_preset: &str,
    provider_model: &str,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    startup_session_entry_with_mode_and_details(
        run_id,
        run_dir,
        run_name,
        status,
        last_updated_at,
        profile_preset,
        provider_model,
        harness_core::proj::SessionModeSource::InteractiveLive,
        is_resumable,
        resume_disabled_reason,
    )
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps session-history fixture fields explicit at call sites"
)]
fn startup_session_entry_with_mode_and_details(
    run_id: &str,
    run_dir: &str,
    run_name: &str,
    status: Option<harness_core::proj::RunStatus>,
    last_updated_at: Option<&str>,
    profile_preset: &str,
    provider_model: &str,
    mode_source: harness_core::proj::SessionModeSource,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    app::SessionHistoryEntry {
        run_dir: PathBuf::from(run_dir),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some(run_name.to_string()),
            status,
            last_updated_at: last_updated_at.map(str::to_string),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some(profile_preset.to_string()),
            provider_model: Some(provider_model.to_string()),
            mode_source,
            is_resumable,
            resume_disabled_reason: resume_disabled_reason.map(str::to_string),
        },
    }
}

#[cfg(test)]
fn envelope(
    seq: u64,
    correlation_id: Option<&str>,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        correlation_id,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        payload,
    )
}

#[cfg(test)]
fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: harness_core::event::EventActor,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

#[cfg(test)]
fn orchestration_status_strip_fixture() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_alpha".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_beta".to_string(),
            profile: "reviewer".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_orch_queued"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:alpha".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        Some("req_orch_running"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_beta".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_running".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:beta".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        5,
        Some("req_orch_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:alpha".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        6,
        Some("req_orch_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_stale".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    app
}

#[cfg(test)]
#[test]
fn live_mode_hides_primary_tabs_but_replay_preserves_secondary_access() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }

    let live_backend = TestBackend::new(80, 24);
    let mut live_terminal = Terminal::new(live_backend).expect("create live terminal");
    live_terminal
        .draw(|frame| ui::render_app(frame, &live))
        .expect("draw live frame");

    let live_debug = format!("{:?}", live_terminal.backend().buffer());
    assert!(live_debug.contains("Composer ·"));
    assert!(!live_debug.contains("Tabs"));
    assert!(!live_debug.contains("Activity ("));
    assert!(!live_debug.contains("Inspector"));

    live.handle_key(key(crossterm::event::KeyCode::Tab));
    live.handle_key(key(crossterm::event::KeyCode::Char('i')));
    assert_eq!(live.active_tab, app::Tab::Run);
    assert!(live.details_drawer_open());
    live.handle_key(key(crossterm::event::KeyCode::Char('2')));
    assert_eq!(live.active_tab, app::Tab::Events);
    assert!(!live.details_drawer_open());
    live.handle_key(key(crossterm::event::KeyCode::Char('1')));
    assert_eq!(live.active_tab, app::Tab::Run);
    assert!(!live.details_drawer_open());

    let replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    let replay_backend = TestBackend::new(80, 24);
    let mut replay_terminal = Terminal::new(replay_backend).expect("create replay terminal");
    replay_terminal
        .draw(|frame| ui::render_app(frame, &replay))
        .expect("draw replay frame");

    let replay_debug = format!("{:?}", replay_terminal.backend().buffer());
    assert!(replay_debug.contains("Tabs"));
    assert!(replay_debug.contains("Events"));
}

#[cfg(test)]
#[test]
fn live_mode_accepts_input_without_focus_switch() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "hello");
    assert_eq!(app.prompt_cursor, 5);
}

#[cfg(test)]
#[test]
fn command_palette_renders_and_filters() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert_eq!(
        app.palette_filtered,
        Action::palette_commands()
            .iter()
            .map(|(command, _)| (*command).to_string())
            .collect::<Vec<_>>()
    );

    let open_debug = render_live_screen(&app, 100, 24);
    println!("OPEN\n{open_debug}");
    assert!(open_debug.contains("Command palette"));
    assert!(open_debug.contains("New session"));
    assert!(open_debug.contains("Open Help surface"));
    assert!(open_debug.contains("Return to conversation surface"));

    app.handle_key(key(crossterm::event::KeyCode::Char('d')));

    assert_eq!(app.palette_input, "d");
    assert_eq!(app.palette_cursor, 1);
    assert_eq!(
        app.palette_filtered,
        vec!["details".to_string(), "diff".to_string()]
    );

    let filtered_debug = render_live_screen(&app, 100, 24);
    println!("FILTERED\n{filtered_debug}");
    assert!(filtered_debug.contains("Command palette"));
    assert!(filtered_debug.contains("Toggle live details drawer"));
    assert!(filtered_debug.contains("Open Diff surface"));
    assert!(!filtered_debug.contains("Open Help surface"));
}

#[cfg(test)]
#[test]
fn command_palette_empty_state_renders() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('z')));

    assert!(app.palette_visible);
    assert!(app.palette_filtered.is_empty());

    let debug = render_live_screen(&app, 100, 24);
    println!("EMPTY\n{debug}");
    assert!(debug.contains("Command palette"));
    assert!(debug.contains("No commands"));
}

#[cfg(test)]
#[test]
fn command_palette_includes_session_history_entry() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert!(app.palette_filtered.starts_with(&[
        "new_session".to_string(),
        "resume_session".to_string(),
        "replay_session".to_string(),
    ]));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("New session"));
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("Replay session"));
}

#[cfg(test)]
#[test]
fn session_history_picker_renders_resumable_and_replay_rows() {
    let normalize_snapshot = |render: String| {
        render
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    };

    let entries = vec![
        startup_session_entry_with_details(
            "run_resume",
            "/tmp/sessions/run_resume",
            "alpha-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4",
            true,
            None,
        ),
        startup_session_entry_with_mode_and_details(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            "beta-prompt",
            Some(harness_core::proj::RunStatus::Failed),
            Some("2026-03-07T03:21:00Z"),
            "ops",
            "anthropic/claude-3.7",
            harness_core::proj::SessionModeSource::Prompt,
            false,
            Some("prompt runs are not resumable"),
        ),
    ];
    let mut app = app::AppState::new_startup(entries, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    let resume_render = render_live_lines(&app, 120, 30);

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    let replay_render = render_live_lines(&app, 120, 30);

    insta::assert_snapshot!(
        format!(
            "RESUME\n{}\n\nREPLAY\n{}",
            normalize_snapshot(resume_render),
            normalize_snapshot(replay_render)
        ),
        @r###"
RESUME








                         ┌Harness─────────────────────────────────────────────────────────────┐
              ┌Continue session · 1 match────────────────────────────────────────────────────────────────┐
              │> █                                                                                       │
              │Interactive histories · 1 ready · filter by run/profile/model                             │
              │▶ continue alpha-run · continue ready · finished · deep/openai/gpt-5.4                    │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              └──────────────────────────────────────────────────────────────────────────────────────────┘





   Ready   startup launcher ready · choose New/Continue/Replay or type to quick-start  ·  agents 0 · queued 0
  ┌Composer · 1 line · 0 chars───────────────────────────────────────────────────────────────────────────────────────┐
  │Type to quick-start a new session while the lifecycle actions stay available.                                     │
  └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
  ↑ prev  ↓ next  Enter select  Tab composer  Ctrl+p palette  q quit

REPLAY








                         ┌Harness─────────────────────────────────────────────────────────────┐
              ┌Replay session · 2 matches────────────────────────────────────────────────────────────────┐
              │> █                                                                                       │
              │Read-only replays · 2 matching · 1 prompt-only still visible                              │
              │↺ replay alpha-run · replay ready · continue ready · finished · deep/openai/gpt-5.4       │
              │↺ replay beta-prompt · prompt-only replay ready · failed · ops/anthropic/claude-3.7       │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              │                                                                                          │
              └──────────────────────────────────────────────────────────────────────────────────────────┘





   Ready   startup launcher ready · choose New/Continue/Replay or type to quick-start  ·  agents 0 · queued 0
  ┌Composer · 1 line · 0 chars───────────────────────────────────────────────────────────────────────────────────────┐
  │Type to quick-start a new session while the lifecycle actions stay available.                                     │
  └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
  ↑ prev  ↓ next  Enter select  Tab composer  Ctrl+p palette  q quit
"###);
}

#[cfg(test)]
#[test]
fn session_history_filter_uses_case_insensitive_substrings() {
    fn open_continue_picker() -> app::AppState {
        let mut app = app::AppState::new_startup(
            vec![
                startup_session_entry_with_mode_and_details(
                    "RUN-ABC123",
                    "/tmp/sessions/RUN-ABC123",
                    "Alpha Runner",
                    Some(harness_core::proj::RunStatus::Finished),
                    Some("2026-03-08T12:34:56Z"),
                    "DeepOps",
                    "OpenAI/GPT-5.4",
                    harness_core::proj::SessionModeSource::InteractiveLive,
                    false,
                    Some("run is still active"),
                ),
                startup_session_entry_with_details(
                    "run_other",
                    "/tmp/sessions/run_other",
                    "beta-run",
                    Some(harness_core::proj::RunStatus::Running),
                    Some("2026-03-08T08:00:00Z"),
                    "ops",
                    "anthropic/claude-3.7",
                    true,
                    None,
                ),
            ],
            None,
        );

        app.handle_key(key_with_modifiers(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in "resume".chars() {
            app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
        }
        app.handle_key(key(crossterm::event::KeyCode::Enter));
        app
    }

    let mut by_run_id = open_continue_picker();
    for ch in "bc12".chars() {
        by_run_id.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_run_id.palette_input, "bc12");
    assert_eq!(by_run_id.session_history_filtered, vec![0]);

    let mut by_run_name = open_continue_picker();
    for ch in "runner".chars() {
        by_run_name.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_run_name.session_history_filtered, vec![0]);

    let mut by_status = open_continue_picker();
    for ch in "finish".chars() {
        by_status.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_status.session_history_filtered, vec![0]);

    let mut by_timestamp = open_continue_picker();
    for ch in "12:34".chars() {
        by_timestamp.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_timestamp.session_history_filtered, vec![0]);

    let mut by_profile = open_continue_picker();
    for ch in "ops".chars() {
        by_profile.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_profile.session_history_filtered, vec![1, 0]);

    let mut by_provider = open_continue_picker();
    for ch in "gpt-5".chars() {
        by_provider.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_provider.session_history_filtered, vec![0]);

    let mut by_resumability = open_continue_picker();
    for ch in "still active".chars() {
        by_resumability.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_resumability.session_history_filtered, vec![0]);

    let mut no_match = open_continue_picker();
    for ch in "missing".chars() {
        no_match.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    no_match.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(no_match.session_history_filtered.is_empty());
    assert_eq!(
        no_match.continue_disabled_banner.as_deref(),
        Some("no sessions match the current filter")
    );
    let rendered = render_live_lines(&no_match, 120, 30);
    assert!(rendered.contains("no sessions match the current filter"));
    assert!(rendered.contains("No saved runs match this filter."));
}

#[cfg(test)]
#[test]
fn continue_picker_filters_to_interactive_sessions() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry_with_mode_and_details(
                "run_blocked",
                "/tmp/sessions/run_blocked",
                "blocked-interactive",
                Some(harness_core::proj::RunStatus::Running),
                Some("2026-03-08T09:00:00Z"),
                "ops",
                "openai/gpt-5.4",
                harness_core::proj::SessionModeSource::InteractiveLive,
                false,
                Some("run is still active"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_prompt",
                "/tmp/sessions/run_prompt",
                "prompt-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T08:00:00Z"),
                "ops",
                "openai/gpt-5.3-codex",
                harness_core::proj::SessionModeSource::Prompt,
                false,
                Some("prompt runs are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_ready_live",
                "/tmp/sessions/run_ready_live",
                "ready-live",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T07:00:00Z"),
                "deep",
                "openai/gpt-5.4",
                harness_core::proj::SessionModeSource::InteractiveLive,
                true,
                None,
            ),
            startup_session_entry_with_mode_and_details(
                "run_scenario",
                "/tmp/sessions/run_scenario",
                "scenario-fixture",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T06:00:00Z"),
                "default",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::ScenarioFixture,
                false,
                Some("scenario fixture runs are excluded from resume"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_replay_only",
                "/tmp/sessions/run_replay_only",
                "replay-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T05:00:00Z"),
                "default",
                "openai/gpt-5.4",
                harness_core::proj::SessionModeSource::ReplayOnly,
                false,
                Some("replay-only launches are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_ready_mock",
                "/tmp/sessions/run_ready_mock",
                "ready-mock",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T04:00:00Z"),
                "mock",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::InteractiveMock,
                true,
                None,
            ),
        ],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_ready_live", "run_ready_mock", "run_blocked"]
    );
    assert_eq!(
        app.session_history_entries[*app
            .session_history_filtered
            .last()
            .expect("blocked interactive entry present")]
        .catalog
        .resume_disabled_reason
        .as_deref(),
        Some("run is still active")
    );
    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("run is still active"));
    assert!(!rendered.contains("prompt-only"));
    assert!(!rendered.contains("scenario-fixture"));
    assert!(!rendered.contains("replay-only"));
}

#[cfg(test)]
#[test]
fn replay_picker_keeps_prompt_runs_visible() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry_with_mode_and_details(
                "run_ready_live",
                "/tmp/sessions/run_ready_live",
                "ready-live",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T07:00:00Z"),
                "deep",
                "openai/gpt-5.4",
                harness_core::proj::SessionModeSource::InteractiveLive,
                true,
                None,
            ),
            startup_session_entry_with_mode_and_details(
                "run_prompt",
                "/tmp/sessions/run_prompt",
                "prompt-only",
                Some(harness_core::proj::RunStatus::Failed),
                Some("2026-03-08T06:00:00Z"),
                "ops",
                "openai/gpt-5.3-codex",
                harness_core::proj::SessionModeSource::Prompt,
                false,
                Some("prompt runs are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_scenario",
                "/tmp/sessions/run_scenario",
                "scenario-fixture",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T05:00:00Z"),
                "default",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::ScenarioFixture,
                false,
                Some("scenario fixture runs are excluded from resume"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_replay_only",
                "/tmp/sessions/run_replay_only",
                "replay-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T04:00:00Z"),
                "default",
                "openai/gpt-5.4",
                harness_core::proj::SessionModeSource::ReplayOnly,
                false,
                Some("replay-only launches are not resumable"),
            ),
        ],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_ready_live", "run_prompt"]
    );
    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Replay session"));
    assert!(rendered.contains("prompt-only"));
    assert!(rendered.contains("prompt-only replay ready"));
    assert!(!rendered.contains("scenario-fixture"));
    assert!(!rendered.contains("replay-only"));
}

#[cfg(test)]
#[test]
fn focus_returns_after_session_history_close() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.prompt_buffer = "keep prompt draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.set_session_history_entries(vec![startup_session_entry_with_details(
        "run_replay",
        "/tmp/sessions/run_replay",
        "replayable-run",
        Some(harness_core::proj::RunStatus::Finished),
        Some("2026-03-08T12:34:56Z"),
        "deep",
        "openai/gpt-5.4",
        true,
        None,
    )]);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.session_history_visible);
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");
    assert_eq!(app.prompt_cursor, "keep prompt draft".chars().count());
}

#[cfg(test)]
#[test]
fn command_palette_enter_executes_selected_command() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Help;
    app.focus = app::Focus::Details;
    app.prompt_buffer = "preserve me".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('e')));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(app.active_tab, app::Tab::Events);
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "preserve me");
    assert_eq!(app.prompt_cursor, "preserve me".chars().count());
}

#[cfg(test)]
#[test]
fn palette_escape_preserves_prompt_draft() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "keep this prompt".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let prompt_before = app.prompt_buffer.clone();
    let cursor_before = app.prompt_cursor;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "d");

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(app.palette_cursor, 0);
    assert!(app.palette_filtered.is_empty());
    assert_eq!(app.palette_selected, 0);
    assert_eq!(app.prompt_buffer, prompt_before);
    assert_eq!(app.prompt_cursor, cursor_before);
    assert!(app.prompt_history.is_empty());
    assert_eq!(app.prompt_history_index, None);
}

#[cfg(test)]
#[test]
fn permission_modal_preempts_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    for c in "blocked by permission".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_block_submit",
        "tool_call_block_submit",
    ));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert!(intents.is_empty());
    drop(intents);

    assert_eq!(app.prompt_buffer, "blocked by permission");
    assert_eq!(app.prompt_cursor, "blocked by permission".chars().count());
    assert!(app.prompt_history.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.active_permission().is_some());
}

#[cfg(test)]
#[test]
fn startup_surface_renders_primary_actions() {
    let normalize_snapshot = |render: String| {
        render
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert_eq!(app.focus, app::Focus::List);

    let new_idx = rendered.find("New session").expect("new session label");
    let continue_idx = rendered
        .find("Continue session")
        .expect("continue session label");
    let replay_idx = rendered
        .find("Replay session")
        .expect("replay session label");

    assert!(new_idx < continue_idx);
    assert!(continue_idx < replay_idx);
    insta::assert_snapshot!(normalize_snapshot(rendered), @r###"





    ┌Harness─────────────────────────────────────────────────────────────┐
    │                 Preset worker · mock/model-1 · Demo                │
    │   Dispatch a new run, reopen live work, or inspect saved history.  │
    │ + New session · dispatch a fresh run from the draft below          │
    │ ▶ Continue session · reopen interactive work · 1 ready             │
    │ ↺ Replay session · inspect saved runs read-only · 1 available      │
    │            1 saved run ready across Continue and Replay            │
    │    Type to quick-start a fresh run · Ctrl+P opens session tools    │
    └────────────────────────────────────────────────────────────────────┘





 Ready   startup launcher ready · choose New/Continue/Replay or type to quick-
┌Composer · 1 line · 0 chars─────────────────────────────────────────────────┐
│Type to quick-start a new session while the lifecycle actions stay          │
└────────────────────────────────────────────────────────────────────────────┘
↑ prev  ↓ next  Enter select  Tab composer  Ctrl+p palette  q quit
"###);
}

#[cfg(test)]
#[test]
fn startup_typing_moves_to_quick_start_prompt() {
    let mut app = app::AppState::new_startup(Vec::new(), None);

    assert_eq!(app.focus, app::Focus::List);
    assert!(app.prompt_buffer.is_empty());

    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    assert_eq!(app.focus, app::Focus::Prompt);
    assert_eq!(app.prompt_buffer, "x");
    assert_eq!(app.prompt_cursor, 1);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("New session"));
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("Replay session"));
    assert!(rendered.contains("x█"));
}

#[cfg(test)]
#[test]
fn startup_palette_remains_secondary_and_draft_safe() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );

    for ch in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("New session"));
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("Replay session"));
    assert_eq!(app.prompt_buffer, "keep this draft");
    assert_eq!(app.focus, app::Focus::Prompt);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    insta::assert_snapshot!(render_live_lines(&app, 100, 24), @r###"
                                                                                         
                                                                                         
                                                                                         
                                                                                         
                                                                                         
    ┌Harness─────────────────────────────────────────────────────────────┐               
┌Command palette─────────────────────────────────────────────────────────────┐           
│> █                                                                         │           
│New session  Start a fresh live session                                     │           
│Continue session  Continue a prior session when resumable                   │           
│Replay session  Replay a previous session as read-only                      │           
│Help  Open Help surface                                                     │           
│Run  Return to conversation surface                                         │           
│Details  Toggle live details drawer                                         │           
│Events  Open Events surface                                                 │           
│Diff  Open Diff surface                                                     │           
└────────────────────────────────────────────────────────────────────────────┘           
                                                                                         
                                                                                         
 Ready   startup launcher ready · choose New/Continue/Replay or type to quick-           
┌Composer · 1 line · 15 chars────────────────────────────────────────────────┐           
│keep this draft█                                                            │           
└────────────────────────────────────────────────────────────────────────────┘           
Enter send  Shift+Enter nl  i details  2 events  3 diff  4 help  q quit
"###);

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.palette_visible);
    assert_eq!(app.prompt_buffer, "keep this draft");
    assert_eq!(app.prompt_cursor, "keep this draft".chars().count());
    assert_eq!(app.focus, app::Focus::Prompt);
}

#[cfg(test)]
#[test]
fn post_run_handoff_renders_next_actions() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.active_tab = app::Tab::Events;
    app.focus = app::Focus::Prompt;
    app.prompt_buffer = "keep this draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert_eq!(app.focus, app::Focus::List);
    assert!(app.post_run_handoff_visible());
    assert!(app.runtime_state().composer_disabled);

    let rendered = render_live_lines(&app, 100, 24);
    let continue_idx = rendered
        .find("Continue this session")
        .expect("continue action");
    let replay_idx = rendered.find("Replay this run").expect("replay action");
    let new_idx = rendered
        .find("Start another session")
        .expect("new session action");
    let quit_idx = rendered.find("Quit").expect("quit action");

    assert!(continue_idx < replay_idx);
    assert!(replay_idx < new_idx);
    assert!(new_idx < quit_idx);
    assert!(rendered.contains("Next action"));
    assert!(rendered.contains("Continue available"));
    assert!(rendered.contains("› ▶ Continue this session"));
    assert!(rendered.contains("resume this run live from the composer"));
    assert!(!rendered.contains("Composer"));
    assert!(rendered.contains("↑ prev"));
    assert!(rendered.contains("↓ next"));
    assert!(rendered.contains("Enter select"));
    assert!(rendered.contains("2 events"));
    assert!(rendered.contains("3 diff"));
    assert!(rendered.contains("4 help"));
    assert!(!rendered.contains(" send"));
    assert!(!rendered.contains(" nl"));
}

#[cfg(test)]
#[test]
fn post_run_failure_handoff_renders_recovery_actions() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "tool execution failed".to_string(),
        }),
    ));

    assert!(matches!(
        app.runtime_state().kind,
        app::RuntimeStateKind::Failure
    ));
    assert_eq!(app.focus, app::Focus::List);

    let rendered = render_live_lines(&app, 100, 24);
    let continue_idx = rendered
        .find("Continue this session")
        .expect("continue action");
    let replay_idx = rendered.find("Replay this run").expect("replay action");
    let new_idx = rendered
        .find("Start another session")
        .expect("new session action");
    let quit_idx = rendered.find("Quit").expect("quit action");

    assert!(continue_idx < replay_idx);
    assert!(replay_idx < new_idx);
    assert!(new_idx < quit_idx);
    assert!(rendered.contains("Next action"));
    assert!(rendered.contains("Recovery available"));
    assert!(rendered.contains("› ▶ Continue this session"));
    assert!(rendered.contains("run failed · choose what to do next"));
    assert!(!rendered.contains("current run cannot be reopened"));
    assert!(!rendered.contains("Composer"));
}

#[cfg(test)]
#[test]
fn post_run_handoff_disables_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.prompt_buffer = "blocked prompt".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("Next action"));
    assert!(rendered.contains("current run cannot be reopened"));
    assert!(rendered.contains("Recovery only"));
    assert!(rendered.contains("› + Start another session"));
    assert!(rendered.contains("  × Quit"));
    assert!(!rendered.contains("Continue this session"));
    assert!(!rendered.contains("Replay this run"));
    assert!(!rendered.contains("Composer"));

    app.focus = app::Focus::Prompt;
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert_eq!(app.prompt_buffer, "blocked prompt");
    assert!(intents.lock().expect("lock intents").is_empty());

    app.focus = app::Focus::List;
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(&*intents, &[UiIntent::NewSession]);
    assert!(app.should_quit);
}

#[cfg(test)]
#[test]
fn continued_quiescent_bootstrap_shows_handoff_before_reopening_live_conversation() {
    app::set_pending_live_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "default:model-1")
            .with_mode_label("Continued"),
    );
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
        false,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        Some("req_resume_terminal"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let handoff_render = render_live_lines(&app, 100, 24);
    assert!(handoff_render.contains("Next action"));
    assert!(handoff_render.contains("› ▶ Continue this session"));
    assert!(!handoff_render.contains("Continued · run run_resume_quiescent"));
    assert!(!handoff_render.contains("Composer"));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let resumed_render = render_live_lines(&app, 100, 24);
    assert!(!resumed_render.contains("Next action"));
    assert!(resumed_render.contains("Continued live run"));
    assert!(resumed_render.contains("Same run reopened live"));
    assert!(resumed_render.contains("Composer"));
}

#[cfg(test)]
#[test]
fn lifecycle_shell_state_transitions() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.prompt_buffer = "draft prompt".to_string();
    startup.prompt_cursor = startup.prompt_buffer.chars().count();

    assert_eq!(
        startup.lifecycle_shell_state(),
        app::LifecycleShellState::Startup
    );
    assert!(startup.startup_shell_visible());
    assert!(!startup.post_run_handoff_visible());
    assert!(!startup.composer_disabled());

    let mut post_run = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    post_run.ingest_event(envelope(
        1,
        Some("req_state_transition"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(
        post_run.lifecycle_shell_state(),
        app::LifecycleShellState::PostRun
    );
    assert!(!post_run.startup_shell_visible());
    assert!(post_run.post_run_handoff_visible());
    assert!(post_run.composer_disabled());
    assert_eq!(
        post_run.selected_post_run_handoff_action(),
        app::PostRunHandoffAction::ContinueSession
    );

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut missing_session_path = app::AppState::new_live(None, false, Some(fallback_sink));
    missing_session_path.ingest_event(envelope(
        1,
        Some("req_state_transition_missing_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(
        missing_session_path.lifecycle_shell_state(),
        app::LifecycleShellState::PostRun
    );
    assert_eq!(
        missing_session_path.post_run_handoff_notice(),
        Some("current run cannot be reopened")
    );
    assert_eq!(
        missing_session_path.selected_post_run_handoff_action(),
        app::PostRunHandoffAction::StartAnotherSession
    );

    let replay = app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            Some("req_replay_state_transition"),
            harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(
        replay.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(replay.composer_disabled());
}

#[cfg(test)]
#[test]
fn lifecycle_shell_snapshots() {
    let normalize_snapshot = |render: String| {
        render
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut startup = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    startup.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let startup_render = render_live_lines(&startup, 100, 24);
    let new_idx = startup_render
        .find("New session")
        .expect("new session label");
    let continue_idx = startup_render
        .find("Continue session")
        .expect("continue session label");
    let replay_idx = startup_render
        .find("Replay session")
        .expect("replay session label");
    assert!(new_idx < continue_idx);
    assert!(continue_idx < replay_idx);
    assert!(startup_render.contains("Type to quick-start"));
    insta::assert_snapshot!(
        "lifecycle_shell_startup_surface",
        normalize_snapshot(startup_render)
    );

    let entries = vec![
        startup_session_entry_with_details(
            "run_resume",
            "/tmp/sessions/run_resume",
            "alpha-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4",
            true,
            None,
        ),
        startup_session_entry_with_mode_and_details(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            "beta-prompt",
            Some(harness_core::proj::RunStatus::Failed),
            Some("2026-03-07T03:21:00Z"),
            "ops",
            "anthropic/claude-3.7",
            harness_core::proj::SessionModeSource::Prompt,
            false,
            Some("prompt runs are not resumable"),
        ),
        startup_session_entry_with_mode_and_details(
            "run_blocked",
            "/tmp/sessions/run_blocked",
            "blocked-interactive",
            Some(harness_core::proj::RunStatus::Running),
            Some("2026-03-06T09:15:00Z"),
            "ops",
            "openai/gpt-5.4",
            harness_core::proj::SessionModeSource::InteractiveLive,
            false,
            Some("run is still active"),
        ),
    ];
    let mut picker = app::AppState::new_startup(entries, None);
    for ch in "keep this draft".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(picker.prompt_buffer, "keep this draft");
    assert_eq!(picker.focus, app::Focus::Prompt);

    picker.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    picker.handle_key(key(crossterm::event::KeyCode::Enter));

    let continue_render = render_live_lines(&picker, 120, 30);
    assert!(picker.session_history_visible);
    assert_eq!(picker.prompt_buffer, "keep this draft");
    assert!(continue_render.contains("Continue session"));
    assert!(continue_render.contains("continue ready"));
    assert!(continue_render.contains("run is still active"));
    assert!(!continue_render.contains("beta-prompt"));

    picker.handle_key(key(crossterm::event::KeyCode::Esc));
    picker.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    picker.handle_key(key(crossterm::event::KeyCode::Enter));

    let replay_render = render_live_lines(&picker, 120, 30);
    assert!(picker.session_history_visible);
    assert_eq!(picker.prompt_buffer, "keep this draft");
    assert!(replay_render.contains("Replay session"));
    assert!(replay_render.contains("beta-prompt"));
    assert!(replay_render.contains("prompt-only replay ready"));
    insta::assert_snapshot!(
        "lifecycle_shell_session_picker",
        format!(
            "CONTINUE\n{}\n\nREPLAY\n{}",
            normalize_snapshot(continue_render),
            normalize_snapshot(replay_render)
        )
    );

    let mut post_run = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    post_run.active_tab = app::Tab::Events;
    post_run.focus = app::Focus::Prompt;
    post_run.prompt_buffer = "keep this draft".to_string();
    post_run.prompt_cursor = post_run.prompt_buffer.chars().count();
    post_run.ingest_event(envelope(
        1,
        Some("req_post_run"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let post_run_render = render_live_lines(&post_run, 100, 24);
    let continue_idx = post_run_render
        .find("Continue this session")
        .expect("continue action");
    let replay_idx = post_run_render
        .find("Replay this run")
        .expect("replay action");
    let new_idx = post_run_render
        .find("Start another session")
        .expect("new session action");
    let quit_idx = post_run_render.find("Quit").expect("quit action");
    assert!(continue_idx < replay_idx);
    assert!(replay_idx < new_idx);
    assert!(new_idx < quit_idx);
    assert!(post_run_render.contains("Next action"));
    assert!(post_run_render.contains("Continue available"));
    assert!(post_run_render.contains("› ▶ Continue this session"));
    assert!(post_run_render.contains("run finished · choose what to do next"));
    assert!(!post_run_render.contains("Composer"));
    insta::assert_snapshot!(
        "lifecycle_shell_post_run_surface",
        normalize_snapshot(post_run_render)
    );

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut fallback = app::AppState::new_live(None, false, Some(fallback_sink));
    fallback.ingest_event(envelope(
        1,
        Some("req_post_run_missing_session_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let fallback_render = render_live_lines(&fallback, 100, 24);
    assert!(fallback_render.contains("Next action"));
    assert!(fallback_render.contains("current run cannot be reopened"));
    assert!(fallback_render.contains("Recovery only"));
    assert!(fallback_render.contains("› + Start another session"));
    assert!(fallback_render.contains("  × Quit"));
    assert!(!fallback_render.contains("Continue this session"));
    assert!(!fallback_render.contains("Replay this run"));
    assert!(!fallback_render.contains("Composer"));
    insta::assert_snapshot!(
        "lifecycle_shell_post_run_fallback_surface",
        normalize_snapshot(fallback_render)
    );
}

#[cfg(test)]
#[test]
fn session_history_browse_preserves_draft() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry("run_a", "/tmp/sessions/run_a", true, None),
            startup_session_entry("run_b", "/tmp/sessions/run_b", true, None),
        ],
        None,
    );
    for c in "startup draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    let before = app.prompt_buffer.clone();
    let cursor_before = app.prompt_cursor;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(app.prompt_buffer, before);
    assert_eq!(app.prompt_cursor, cursor_before);

    app.handle_key(key(crossterm::event::KeyCode::Down));
    assert_eq!(app.session_history_selected, 1);

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.session_history_visible);
    assert_eq!(app.prompt_buffer, before);
    assert_eq!(app.prompt_cursor, cursor_before);
}

#[cfg(test)]
#[test]
fn new_session_resets_transcript_but_keeps_unsent_draft() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_a",
            "/tmp/sessions/run_a",
            true,
            None,
        )],
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_before_reset"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_before_reset".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.3-codex".to_string(),
                prompt_summary: "before reset".to_string(),
                request_digest: "digest-before-reset".to_string(),
            },
        ),
    ));
    app.prompt_history.push("older sent prompt".to_string());
    app.prompt_buffer = "unsent startup draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.events.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.prompt_history.is_empty());
    assert_eq!(app.prompt_buffer, "unsent startup draft");
    assert_eq!(app.prompt_cursor, "unsent startup draft".chars().count());
}

#[cfg(test)]
#[test]
fn continue_disabled_session_shows_reason_banner() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            false,
            Some("prompt runs are not resumable"),
        )],
        Some(intent_sink),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert!(intents.is_empty());
    drop(intents);
    assert!(app.session_history_visible);
    assert_eq!(
        app.continue_disabled_banner.as_deref(),
        Some("continue unavailable: prompt runs are not resumable")
    );
    assert!(app
        .runtime_state()
        .summary
        .contains("continue unavailable: prompt runs are not resumable"));
}

#[cfg(test)]
#[test]
fn replay_session_intent_never_enables_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_replay",
            "/tmp/sessions/run_replay",
            true,
            None,
        )],
        Some(intent_sink),
    );
    app.prompt_buffer = "do not submit".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ReplaySession {
            run_id: "run_replay".to_string(),
            run_dir: PathBuf::from("/tmp/sessions/run_replay"),
        }]
    );
    drop(intents);
    assert_eq!(app.prompt_buffer, "do not submit");
    assert!(app.prompt_history.is_empty());
}

#[cfg(test)]
#[test]
fn overlay_wheel_routing_preserved() {
    let mut palette_overlay = app::AppState::new_live(None, false, None);
    palette_overlay.details_scroll = 6;
    palette_overlay.transcript_scroll = 4;
    palette_overlay.follow_mode = false;
    palette_overlay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    palette_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Some(crate::ui::WheelTarget::Transcript),
    );
    palette_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 70,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Some(crate::ui::WheelTarget::Inspector),
    );

    assert!(palette_overlay.palette_visible);
    assert_eq!(palette_overlay.details_scroll, 6);
    assert_eq!(palette_overlay.transcript_scroll, 4);
    assert!(!palette_overlay.follow_mode);

    let mut permission_overlay = app::AppState::new_live(None, false, None);
    permission_overlay.details_scroll = 8;
    permission_overlay.transcript_scroll = 3;
    permission_overlay.follow_mode = false;
    permission_overlay.ingest_event(permission_requested_event(
        1,
        "perm_overlay_wheel",
        "tool_call_overlay_wheel",
    ));

    permission_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Some(crate::ui::WheelTarget::Transcript),
    );
    permission_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 70,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Some(crate::ui::WheelTarget::Inspector),
    );

    assert!(permission_overlay.active_permission().is_some());
    assert_eq!(permission_overlay.details_scroll, 8);
    assert_eq!(permission_overlay.transcript_scroll, 3);
    assert!(!permission_overlay.follow_mode);
}

#[cfg(test)]
#[test]
fn replay_secondary_surfaces_remain_reachable_after_live_shell_refactor() {
    let replay_registry = surface_registry(true);
    assert!(replay_registry
        .iter()
        .all(|surface| surface.tab != app::Tab::Details));

    let mut replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );

    replay.handle_key(key(crossterm::event::KeyCode::Char('2')));
    assert_eq!(replay.active_tab, app::Tab::Events);

    replay.handle_key(key(crossterm::event::KeyCode::Char('3')));
    assert_eq!(replay.active_tab, app::Tab::Diff);
    let replay_diff_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_diff_debug.contains("Tabs"));
    assert!(replay_diff_debug.contains("Diff"));

    replay.handle_key(key(crossterm::event::KeyCode::Char('4')));
    assert_eq!(replay.active_tab, app::Tab::Help);
    let replay_help_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_help_debug.contains("Tabs"));
    assert!(replay_help_debug.contains("Help"));

    replay.handle_key(key(crossterm::event::KeyCode::Char('1')));
    assert_eq!(replay.active_tab, app::Tab::Run);
}

#[cfg(test)]
#[test]
fn composer_enter_submits_and_shift_enter_inserts_newline() {
    use std::sync::Mutex;

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for c in "world".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "hello\nworld".to_string(),
        }]
    );
    drop(intents);

    assert!(app.prompt_buffer.is_empty());
    assert_eq!(
        app.prompt_history.last().map(String::as_str),
        Some("hello\nworld")
    );

    let activity = app.activities.back().expect("submitted activity");
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("hello\nworld")
    );
    assert_eq!(activity.status, app::ActivityStatus::Streaming);
}

#[cfg(test)]
#[test]
fn composer_preserves_draft_while_streaming() {
    use std::sync::Mutex;

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));

    for c in "first".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    for c in "next".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "first".to_string(),
                request_digest: "digest-1".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_001"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "streaming".to_string(),
            },
        ),
    ));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "first".to_string(),
        }]
    );
    drop(intents);

    assert_eq!(app.prompt_buffer, "next");
    assert_eq!(app.prompt_cursor, 4);
    assert_eq!(app.prompt_history.last().map(String::as_str), Some("first"));
    let activity = app.activities.back().expect("streaming activity");
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.transcript_text, "streaming");
    assert_eq!(activity.status, app::ActivityStatus::Streaming);
}

#[cfg(test)]
#[test]
fn surface_registry_exposes_details_drawer_and_secondary_views() {
    let live_registry = surface_registry(false);
    assert!(live_registry
        .iter()
        .any(|surface| { surface.tab == app::Tab::Run && surface.role == SurfaceRole::Primary }));
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Details && surface.role == SurfaceRole::Drawer
    }));
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Events && surface.role == SurfaceRole::Secondary
    }));
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Diff && surface.role == SurfaceRole::Secondary
    }));
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Help && surface.role == SurfaceRole::Secondary
    }));

    let replay_registry = surface_registry(true);
    assert!(replay_registry
        .iter()
        .all(|surface| surface.tab != app::Tab::Details));

    let replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    assert!(!replay.details_drawer_open());
}

#[cfg(test)]
#[test]
fn replay_mode_does_not_render_orchestration_summary() {
    let mut events = session_view_events();
    events.extend([
        envelope(
            100,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_replay".to_string(),
                profile: "researcher".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            101,
            Some("req_replay_orch"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_replay".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_replay_orch".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("agent:queued:replay".to_string()),
            }),
        ),
        envelope_with_actor(
            102,
            Some("req_replay_orch"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_replay".to_string()),
            ),
            harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
                task_id: "task_replay_orch".to_string(),
                stale_for_ms: 3001,
            }),
        ),
    ]);

    let mut replay =
        app::AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), events);

    let replay_run = render_live_lines(&replay, 120, 30);
    assert!(!replay_run.contains("Orchestration"));
    assert!(!replay_run.contains("agents "));
    assert!(!replay.details_drawer_open());

    replay.handle_key(key(crossterm::event::KeyCode::Char('2')));
    let replay_events = render_live_lines(&replay, 120, 30);
    assert!(!replay_events.contains("Orchestration"));
    assert!(!replay_events.contains("agents "));

    replay.handle_key(key(crossterm::event::KeyCode::Char('3')));
    let replay_diff = render_live_lines(&replay, 120, 30);
    assert!(!replay_diff.contains("Orchestration"));
    assert!(!replay_diff.contains("agents "));

    replay.handle_key(key(crossterm::event::KeyCode::Char('4')));
    let replay_help = render_live_lines(&replay, 120, 30);
    assert!(!replay_help.contains("Orchestration"));
    assert!(!replay_help.contains("agents "));
}

#[cfg(test)]
#[test]
fn startup_shell_shows_profile_provider_and_model_chrome() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("Harness"));
    assert!(rendered.contains("Preset deep · proxy/gpt-5.4 · Demo"));
    assert!(rendered.contains("Dispatch a new run"));
    assert!(rendered.contains("New session"));
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("Replay session"));
}

#[cfg(test)]
#[test]
fn lifecycle_shell_narrow_layout_renders_primary_cta() {
    let app = app::AppState::new_startup(Vec::new(), None);

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let new_session_row = find_line_containing(&lines, "New session").expect("primary CTA row");
    let continue_row = find_line_containing(&lines, "Continue session").expect("secondary CTA row");
    let replay_row = find_line_containing(&lines, "Replay session").expect("replay CTA row");
    let hint_row =
        find_line_containing(&lines, "Type to quick-start").expect("quick-start hint row");
    let footer_row = find_line_containing(&lines, "q quit").expect("footer row");

    assert!(new_session_row < continue_row);
    assert!(continue_row < replay_row);
    assert!(replay_row < hint_row);
    assert!(hint_row < footer_row);
}

#[cfg(test)]
#[test]
fn startup_card_uses_lifecycle_geometry_contract() {
    let theme = Theme::default();
    let minimum_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let primary_area = ratatui::layout::Rect::new(0, 0, 100, 30);

    let minimum_layout = theme.lifecycle_surface_layout(minimum_area.width, minimum_area.height);
    let primary_layout = theme.lifecycle_surface_layout(primary_area.width, primary_area.height);

    let minimum_startup = layout::startup_shell_area(minimum_area, &theme);
    let primary_startup = layout::startup_shell_area(primary_area, &theme);

    assert_eq!(
        minimum_startup,
        layout::lifecycle_card_area(minimum_area, &theme, minimum_layout.startup_card)
    );
    assert_eq!(
        primary_startup,
        layout::lifecycle_card_area(primary_area, &theme, primary_layout.startup_card)
    );
    assert_eq!(minimum_startup, ratatui::layout::Rect::new(5, 7, 70, 9));
    assert_eq!(primary_startup, ratatui::layout::Rect::new(13, 10, 74, 9));
    assert_ne!(
        minimum_startup,
        layout::live_empty_state_area(minimum_area, &theme)
    );
    assert_ne!(
        primary_startup,
        layout::live_empty_state_area(primary_area, &theme)
    );
}

#[cfg(test)]
#[test]
fn post_run_card_uses_lifecycle_geometry_contract() {
    let theme = Theme::default();
    let minimum_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let primary_area = ratatui::layout::Rect::new(0, 0, 100, 30);

    let minimum_layout = theme.lifecycle_surface_layout(minimum_area.width, minimum_area.height);
    let primary_layout = theme.lifecycle_surface_layout(primary_area.width, primary_area.height);

    let minimum_post_run =
        layout::lifecycle_card_area(minimum_area, &theme, minimum_layout.post_run_card);
    let primary_post_run =
        layout::lifecycle_card_area(primary_area, &theme, primary_layout.post_run_card);

    assert_eq!(minimum_post_run, ratatui::layout::Rect::new(4, 7, 72, 10));
    assert_eq!(primary_post_run, ratatui::layout::Rect::new(12, 10, 76, 10));
    assert!(minimum_post_run.height > minimum_layout.startup_card.height);
    assert!(primary_post_run.width > primary_layout.startup_card.width);
    assert_eq!(
        minimum_post_run
            .x
            .saturating_add(minimum_post_run.width / 2),
        minimum_area.width / 2
    );
    assert_eq!(
        primary_post_run
            .x
            .saturating_add(primary_post_run.width / 2),
        primary_area.width / 2
    );
}

#[cfg(test)]
#[test]
fn live_empty_state_uses_shared_startup_copy_without_mode_badges() {
    let mut demo = app::AppState::new_live(None, false, None);
    demo.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let demo_rendered = render_live_lines(&demo, 100, 24);
    assert!(demo_rendered.contains("Harness"));
    assert!(demo_rendered.contains("Start a conversation to begin"));
    assert!(!demo_rendered.contains("Demo mode · mock provider"));
    assert!(!demo_rendered.contains("Preset worker · mock/model-1 · Demo"));

    let mut mock = app::AppState::new_live(None, false, None);
    mock.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
    );

    let mock_rendered = render_live_lines(&mock, 100, 24);
    assert!(mock_rendered.contains("Harness"));
    assert!(mock_rendered.contains("Start a conversation to begin"));
    assert!(!mock_rendered.contains("Mock mode · mock provider"));
    assert!(!mock_rendered.contains("Preset worker · mock/model-1 · Mock"));
}

#[cfg(test)]
#[test]
fn live_shell_minimum_geometry_snapshot_renders_without_overlap() {
    assert_live_shell_geometry(80, 24);
}

#[cfg(test)]
#[test]
fn live_shell_primary_geometry_snapshot_renders_without_overlap() {
    assert_live_shell_geometry(100, 30);
}

#[cfg(test)]
#[test]
fn live_empty_state_snapshot_renders_input_first_shell() {
    let app = app::AppState::new_live(None, false, None);
    insta::assert_snapshot!(
        "live_empty_state_snapshot_renders_input_first_shell",
        render_live_lines(&app, 80, 24)
    );
}

#[cfg(test)]
#[test]
fn live_empty_state_disappears_after_first_activity() {
    let theme = Theme::default();
    let mut app = app::AppState::new_live(None, false, None);

    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains(theme.live_shell.empty_state.value_prop));
    assert!(!rendered.contains(theme.live_shell.empty_state.example_prompts[0].prompt));
    assert!(rendered.contains("ship it"));
    assert!(rendered.contains("pending turn"));
}

#[cfg(test)]
#[test]
fn live_shell_orchestration_status_strip_snapshot() {
    let app = orchestration_status_strip_fixture();
    let status_row = live_status_strip_row(&app, 160, 30, "ready for first turn");

    insta::assert_snapshot!(status_row);
}

#[cfg(test)]
#[test]
fn live_status_strip_orchestration_summary_truncates_warning_last() {
    let app = orchestration_status_strip_fixture();

    let wide = live_status_strip_row(&app, 160, 30, "ready for first turn");
    assert!(wide.contains("orch 2a 1q 1r 1s"));
    assert!(wide.contains("· warn stale for 3001 ms"));

    let counts_only = live_status_strip_row(&app, 77, 24, "ready for first turn");
    assert!(counts_only.contains("orch 2a 1q 1r 1s"));
    assert!(!counts_only.contains("warn"));
}

#[cfg(test)]
#[test]
fn live_status_strip_renders_zero_state_orchestration_counts() {
    let app = app::AppState::new_live(None, false, None);

    let status_row = live_status_strip_row(&app, 80, 24, "ready for first turn");
    assert!(status_row.contains("orch 0a 0q 0r 0s"));
    assert!(!status_row.contains("warn"));
}

#[cfg(test)]
#[test]
fn live_empty_state_respects_compact_geometry() {
    let theme = Theme::default();
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let title_row =
        find_line_containing(&lines, theme.live_shell.empty_state.title).expect("title row");
    let value_prop_row = find_line_containing(&lines, theme.live_shell.empty_state.value_prop)
        .expect("value prop row");
    let help_row = find_line_containing(&lines, "Enter send").expect("in-panel help row");
    let status_row =
        find_line_containing(&lines, "ready for first turn").expect("status strip row");

    assert!(
        title_row > 0,
        "empty state title should not render flush against the header"
    );
    assert!(title_row < value_prop_row);
    assert!(value_prop_row < help_row);
    assert!(help_row < status_row);
}

#[cfg(test)]
#[test]
fn live_shell_type_first_input_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "draft prompt".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    insta::assert_snapshot!(
        "live_shell_type_first_input",
        render_live_lines(&app, 80, 24)
    );
}

#[cfg(test)]
#[test]
fn live_shell_shift_enter_keeps_draft_multiline() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "first line".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for c in "second line".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_history.len(), 0);
    assert_eq!(app.prompt_buffer, "first line\nsecond line");
    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "first line",
            "second line",
            "Composer · draft · 2 lines · 22 chars",
        ],
    );
}

#[cfg(test)]
#[test]
fn live_shell_enter_submits_and_echoes_prompt_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(app.prompt_buffer, "");
    assert_eq!(
        app.prompt_history.last().map(String::as_str),
        Some("ship it")
    );
    insta::assert_snapshot!("live_shell_prompt_echo", render_live_lines(&app, 80, 24));
}

#[cfg(test)]
#[test]
fn live_shell_inline_tool_state_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_inline_tool"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_inline_tool".to_string(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_inline_tool"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_inline_tool".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-inline-tool".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline_tool"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline_tool".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs"}"#.to_string(),
                args_digest: "digest-inline-tool-args".to_string(),
            },
        ),
    ));
    app.ingest_event(permission_requested_event(
        4,
        "perm_inline_tool",
        "tc_inline_tool",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    println!("{rendered}");

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission Requested",
            "Read the file",
            "demo.txt",
            "[d] deny",
        ],
    );
}

#[cfg(test)]
#[test]
fn live_shell_permission_preserves_draft_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_snapshot",
        "tool_call_snapshot",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    println!("{rendered}");

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission blocked",
            "Composer · disabled · Permission blocked",
            "FAIL CLOSED",
            "[d] deny",
        ],
    );
}

#[cfg(test)]
#[test]
fn live_shell_degraded_bootstrap_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    println!("{}", render_live_lines(&app, 80, 24));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Degraded",
            "Composer · disabled · Degraded",
            "replaying from seq 1",
            "Sending paused",
        ],
    );
}

#[cfg(test)]
#[test]
fn live_shell_disconnected_stream_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    println!("{}", render_live_lines(&app, 80, 24));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Disconnected",
            "reopen the TUI",
            "Composer · disabled · Disconnected",
            "Draft preserved locally",
        ],
    );
}

#[cfg(test)]
fn orchestration_details_drawer_card_body(app: &app::AppState, height: u16, width: u16) -> String {
    ui::orchestration_card_text_for_test(app, height, width).join("\n")
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_orchestration_snapshot() {
    let app = orchestration_details_drawer_app(0);
    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);

    println!("{card_body}");
    insta::assert_snapshot!(card_body, @r###"
overview · 1 active agents · 2 queued · 1 running · 1 stale
watch · stale for 3001 ms
 stale  task_stale · w1/deep · scan
 running  task_run · supervisor/n/a · queue:none
 queued  task_queue · system/n/a · tool:read
 queued  tool_call_1 · system/n/a · tool:fs.read
 completed  task_done · w2/scout · tool:done
"###);
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_orchestration_primary_snapshot() {
    let app = orchestration_details_drawer_app(0);

    let rendered = render_live_lines(&app, 100, 30);
    println!("{rendered}");
    assert!(rendered.contains("○ Orchestration"));
    assert!(rendered.contains("○ Orchestration · 5 tracked · 1 active"));
    assert!(rendered.contains("watch · stale for 3001 ms"));
    assert!(rendered.contains("completed  task_done · w2/scout"));
    assert!(rendered.contains("● Details"));
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_orchestration_overflow_snapshot() {
    let app = orchestration_details_drawer_app(4);
    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);

    println!("{card_body}");
    insta::assert_snapshot!(card_body, @r###"
overview · 1 active agents · 2 queued · 1 running · 1 stale
watch · stale for 3001 ms
 stale  task_stale · w1/deep · scan
 running  task_run · supervisor/n/a · queue:none
 queued  task_queue · system/n/a · tool:read
 queued  tool_call_1 · system/n/a · tool:fs.read
+5 more
"###);
}

#[cfg(test)]
#[test]
fn live_details_drawer_orchestration_warning_fallback() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);
    assert!(card_body.contains("watch · none"));
    assert!(card_body.contains("overview · 0 active agents · 1 queued · 0 running · 0 stale"));
}

#[cfg(test)]
#[test]
fn layout_plan_primary_geometry_docks_live_details_sidebar() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));

    assert_eq!(plan.shell, ratatui::layout::Rect::new(2, 1, 96, 28));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(2, 1, 55, 24))
    );
    assert_eq!(
        plan.details_overlay,
        Some(ratatui::layout::Rect::new(58, 1, 40, 24))
    );
    assert_eq!(plan.status, Some(ratatui::layout::Rect::new(2, 25, 96, 1)));
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 26, 96, 3))
    );
}

#[cfg(test)]
#[test]
fn layout_plan_minimum_geometry_stacks_live_details_drawer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 80, 24));

    assert_eq!(plan.shell, ratatui::layout::Rect::new(1, 1, 78, 22));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(1, 1, 78, 9))
    );
    assert_eq!(
        plan.details_overlay,
        Some(ratatui::layout::Rect::new(1, 11, 78, 8))
    );
    assert_eq!(plan.status, Some(ratatui::layout::Rect::new(1, 19, 78, 1)));
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(1, 20, 78, 3))
    );
}

#[cfg(test)]
fn assert_live_shell_geometry(width: u16, height: u16) {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let rendered = render_live_lines(&app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let lines = rendered.lines().collect::<Vec<_>>();
    let status_row = find_line_containing(&lines, "Success").expect("status strip");
    let composer_top = find_line_containing(&lines, "Composer").expect("composer frame title");
    let composer_bottom = find_line_containing_from(&lines, composer_top + 1, "─")
        .expect("composer frame bottom border");
    let footer_row = find_line_containing(&lines, "Enter send").expect("footer legend");

    assert!(lines[..status_row]
        .iter()
        .any(|line| line.contains("assistant")));
    assert_eq!(status_row + 1, composer_top);
    assert!(composer_top < composer_bottom);
    assert_eq!(composer_bottom + 1, footer_row);
}

#[cfg(test)]
fn assert_live_shell_contains(app: &app::AppState, width: u16, height: u16, markers: &[&str]) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    for marker in markers {
        assert!(
            rendered.contains(marker),
            "expected live shell to contain {marker:?}\n{rendered}"
        );
    }
}

#[cfg(test)]
fn render_live_lines(app: &app::AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create live shell terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("draw live shell frame");

    terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
fn live_status_strip_row(app: &app::AppState, width: u16, height: u16, marker: &str) -> String {
    let rendered = render_live_lines(app, width, height);
    let lines = rendered.lines().collect::<Vec<_>>();
    let row = find_line_containing(&lines, marker).expect("status strip row");
    lines[row].trim_end().to_string()
}

#[cfg(test)]
fn assert_live_shell_frame_invariants(rendered: &str, width: u16, height: u16) {
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        height as usize,
        "row count must match geometry"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.chars().count() == width as usize),
        "every row must preserve the requested width"
    );
}

#[cfg(test)]
fn find_line_containing(lines: &[&str], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

#[cfg(test)]
fn find_line_containing_from(lines: &[&str], start: usize, needle: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}

#[cfg(test)]
#[test]
fn details_drawer_toggles_without_leaving_live_surface() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.details_drawer_open());

    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(app.details_drawer_open());
    let open_debug = render_live_buffer(&app, 80, 24);
    assert!(open_debug.contains("Request ID:"));
    assert!(open_debug.contains("gpt-5-codex"));

    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.details_drawer_open());
    let closed_debug = render_live_buffer(&app, 80, 24);
    assert!(!closed_debug.contains("Request ID:"));
}

#[cfg(test)]
#[test]
fn replay_and_diff_surfaces_remain_secondary_but_reachable() {
    let live_registry = surface_registry(false);
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Events && surface.role == SurfaceRole::Secondary
    }));
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Diff && surface.role == SurfaceRole::Secondary
    }));
    assert!(live_registry.iter().any(|surface| {
        surface.tab == app::Tab::Help && surface.role == SurfaceRole::Secondary
    }));

    let replay_registry = surface_registry(true);
    assert!(replay_registry
        .iter()
        .all(|surface| surface.tab != app::Tab::Details));

    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }

    live.handle_key(key(crossterm::event::KeyCode::Tab));
    live.handle_key(key(crossterm::event::KeyCode::Char('2')));
    assert_eq!(live.active_tab, app::Tab::Events);
    assert!(!live.details_drawer_open());
    let live_events_debug = render_live_buffer(&live, 80, 24);
    assert!(live_events_debug.contains("Event log"));
    assert!(live_events_debug.contains("Event details"));

    live.handle_key(key(crossterm::event::KeyCode::Char('3')));
    assert_eq!(live.active_tab, app::Tab::Diff);
    assert!(!live.details_drawer_open());
    let live_diff_debug = render_live_buffer(&live, 80, 24);
    assert!(live_diff_debug.contains("Diff"));

    live.handle_key(key(crossterm::event::KeyCode::Char('4')));
    assert_eq!(live.active_tab, app::Tab::Help);
    let live_help_debug = render_live_buffer(&live, 80, 24);
    assert!(live_help_debug.contains("Live surfaces:"));

    live.handle_key(key(crossterm::event::KeyCode::Char('1')));
    assert_eq!(live.active_tab, app::Tab::Run);
    assert!(!live.details_drawer_open());

    let mut replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    replay.handle_key(key(crossterm::event::KeyCode::Char('2')));
    assert_eq!(replay.active_tab, app::Tab::Events);
    let replay_events_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_events_debug.contains("Tabs"));
    assert!(replay_events_debug.contains("Selected event"));

    replay.handle_key(key(crossterm::event::KeyCode::Char('3')));
    assert_eq!(replay.active_tab, app::Tab::Diff);
    let replay_diff_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_diff_debug.contains("Tabs"));
    assert!(replay_diff_debug.contains("Diff"));
    assert!(!replay_diff_debug.contains("Commands"));
    assert!(!replay_diff_debug.contains("Permission Requested"));

    replay.handle_key(key(crossterm::event::KeyCode::Char('4')));
    assert_eq!(replay.active_tab, app::Tab::Help);
    let replay_help_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_help_debug.contains("Replay surfaces:"));
    assert!(!replay_help_debug.contains("Commands"));
    assert!(!replay_help_debug.contains("Permission Requested"));

    replay.handle_key(key(crossterm::event::KeyCode::Char('1')));
    assert_eq!(replay.active_tab, app::Tab::Run);
}
