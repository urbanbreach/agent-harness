pub mod app;
pub mod event;
pub mod keybindings;
pub mod theme;
pub mod ui;

pub use keybindings::{Action, KeyMap};

pub use app::{surface_registry, SurfaceDescriptor, SurfaceRole};
pub use theme::{LiveShellLayout, LiveShellTokens, ShellGeometry, ShellGeometryTarget, Theme};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use harness_core::event::EventEnvelopeV1;
use ratatui::{backend::CrosstermBackend, Terminal};

use app::AppState;
pub use app::UiIntent;
use event::poll;

pub enum LiveUpdate {
    Event(Box<EventEnvelopeV1>),
    Status(String),
}

pub enum TuiMode {
    Replay {
        run_dir: PathBuf,
        events: Vec<EventEnvelopeV1>,
    },
    Live {
        run_dir: PathBuf,
        update_rx: Receiver<LiveUpdate>,
    },
}

pub struct TuiOptions {
    pub mode: TuiMode,
    pub exit_on_finish: bool,
    pub on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    pub keybindings: Option<std::collections::BTreeMap<String, String>>,
}

pub fn run_tui_with_options(options: TuiOptions) -> Result<()> {
    let TuiOptions {
        mode,
        exit_on_finish,
        on_ui_intent,
        keybindings,
    } = options;

    let (mut app, live_updates) = match mode {
        TuiMode::Replay { run_dir, events } => {
            let mut app = AppState::new_replay(run_dir, events);
            if let Some(bindings) = keybindings {
                app.apply_keybindings(bindings);
            }
            (app, None)
        }
        TuiMode::Live { run_dir, update_rx } => {
            let mut app = AppState::new_live(Some(run_dir), exit_on_finish, on_ui_intent);
            if let Some(bindings) = keybindings {
                app.apply_keybindings(bindings);
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
    fn replay_mode_snapshot_renders_two_pane_layout() {
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
            debug.contains("Conversation"),
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
    fn diff_tab_snapshot_renders_artifact_contents() {
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
    fn task_scheduled_queued_updates_tool_status() {
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
            crate::app::ToolCallDisplayStatus::Queued,
            "tool call should be queued after TaskScheduled(Queued)"
        );
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
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_fixture".to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
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
fn theme_tokens_cover_live_shell_states() {
    let default = Theme::default();
    let mono = Theme::mono();

    assert_eq!(default.live_shell.glyphs.streaming, "◐");
    assert_eq!(default.live_shell.glyphs.done, "●");
    assert_eq!(default.live_shell.glyphs.error, "✗");
    assert_eq!(default.live_shell.glyphs.pending_permission, "◷");
    assert_eq!(default.live_shell.glyphs.queued, "◴");
    assert_eq!(default.live_shell.glyphs.running, "◐");
    assert_eq!(default.live_shell.glyphs.succeeded, "●");
    assert_eq!(default.live_shell.glyphs.failed, "✗");

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
fn live_layout_breakpoints_choose_shell_variant() {
    let theme = Theme::default();

    let minimum = theme.live_shell_layout(80, 24);
    assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
    assert_eq!(minimum.activity_drawer_width, 20);
    assert_eq!(minimum.inspector_drawer_width, 20);
    assert_eq!(minimum.transcript_min_width, 28);
    assert_eq!(minimum.centered_content_width, 78);

    let primary = theme.live_shell_layout(100, 30);
    assert_eq!(primary.target, ShellGeometryTarget::Primary);
    assert_eq!(primary.activity_drawer_width, 24);
    assert_eq!(primary.inspector_drawer_width, 28);
    assert_eq!(primary.transcript_min_width, 40);
    assert_eq!(primary.centered_content_width, 92);

    assert_eq!(
        theme.live_shell.target(99, 30),
        ShellGeometryTarget::Minimum
    );
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

    let mut permission_blocked = app::AppState::new_live(None, false, None);
    permission_blocked.ingest_event(permission_requested_event(1, "perm_blocked", "tool_call_1"));
    let permission_blocked_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_blocked_debug.contains("Permission blocked"));
    assert!(permission_blocked_debug.contains("approve or deny the pending permission request"));

    permission_blocked.handle_key(key(crossterm::event::KeyCode::Char('a')));
    let permission_pending_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_pending_debug.contains("Permission pending"));
    assert!(permission_pending_debug.contains("waiting for the permission decision to complete"));

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    let degraded_debug = render_live_buffer(&degraded, 80, 24);
    assert!(degraded_debug.contains("Degraded"));
    assert!(degraded_debug.contains("replaying from seq 1"));
    assert!(degraded_debug.contains("Composer (disabled · Degraded)"));
    assert!(degraded_debug.contains("waiting for live recovery"));

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    let disconnected_debug = render_live_buffer(&disconnected, 80, 24);
    assert!(disconnected_debug.contains("Disconnected"));
    assert!(disconnected_debug.contains("Composer (disabled · Disconnected)"));
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
    assert!(debug.contains("Composer (disabled · Disconnected)"));
    assert!(debug.contains("Reopen the TUI to reconnect"));
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
    assert!(transcript.contains("tool fs.read · succeeded · 12 lines read"));
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
    assert!(transcript.contains("tool shell.run · failed · exit code: 1 stderr: permission denied"));
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
    assert!(debug.contains("Composer (disabled · Permission blocked"));
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
fn envelope(
    seq: u64,
    correlation_id: Option<&str>,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
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
    assert!(live_debug.contains("Conversation"));
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
fn live_empty_state_respects_compact_geometry() {
    let theme = Theme::default();
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let value_prop_row = find_line_containing(&lines, theme.live_shell.empty_state.value_prop)
        .expect("value prop row");
    let first_example_row = find_line_containing(
        &lines,
        theme.live_shell.empty_state.example_prompts[0].prompt,
    )
    .expect("first example row");
    let help_row = find_line_containing(&lines, "Enter send").expect("in-panel help row");
    let status_row =
        find_line_containing(&lines, "Ready · ready for first turn").expect("status strip row");

    assert!(
        value_prop_row > lines.len() / 3,
        "empty state should sit near the composer"
    );
    assert!(value_prop_row < first_example_row);
    assert!(first_example_row < help_row);
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
        &["first line", "second line", "Composer (2 lines · 22 chars)"],
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

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission Requested",
            "Read the file",
            "demo.txt",
            "[a]llow  [d]eny  [esc]dismiss",
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

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission blocked",
            "Composer (disabled · Permission blocked)",
            "[a]llow  [d]eny  [esc]dismiss",
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

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Degraded",
            "Composer (disabled · Degraded)",
            "replaying from seq 1",
        ],
    );
}

#[cfg(test)]
#[test]
fn live_shell_disconnected_stream_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some("live event stream disconnected".to_string()));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &["Disconnected", "reopen the TUI", "Conversation"],
    );
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    insta::assert_snapshot!("live_shell_details_drawer", render_live_lines(&app, 80, 24));
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
    let conversation_top =
        find_line_containing(&lines, "Conversation").expect("conversation frame title");
    let conversation_bottom = find_line_containing_from(&lines, conversation_top + 1, "└")
        .expect("conversation frame bottom border");
    let status_row = find_line_containing(&lines, "Success").expect("status strip");
    let composer_top = find_line_containing(&lines, "Composer").expect("composer frame title");
    let composer_bottom = find_line_containing_from(&lines, composer_top + 1, "└")
        .expect("composer frame bottom border");
    let footer_row = find_line_containing(&lines, "Enter send").expect("footer legend");

    assert!(conversation_top < conversation_bottom);
    assert_eq!(conversation_bottom + 1, status_row);
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
    replay.handle_key(key(crossterm::event::KeyCode::Char('3')));
    assert_eq!(replay.active_tab, app::Tab::Diff);
    let replay_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_debug.contains("Tabs"));
    assert!(replay_debug.contains("Diff"));
}
