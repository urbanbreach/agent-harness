pub mod app;
pub mod event;
pub mod keybindings;
pub mod theme;
pub mod ui;

pub use keybindings::{Action, KeyMap};

pub use theme::Theme;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
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
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
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

            if let Some(event::TuiEvent::Key(key)) = poll(Duration::from_millis(100))? {
                app.handle_key(key);
            }
        }
        Ok(())
    })();

    crossterm::terminal::disable_raw_mode()
        .context("failed to disable terminal raw mode after TUI")?;
    crossterm::execute!(
        terminal.backend_mut(),
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
            Ok(LiveUpdate::Event(event)) => app.ingest_event(*event),
            Ok(LiveUpdate::Status(status)) => app.set_status_banner(Some(status)),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                app.set_status_banner(Some("live event stream disconnected".to_string()));
                break;
            }
        }
    }
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
        RunStartedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
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
            debug.contains("req_1"),
            "activity entry must show request_id"
        );
        assert!(debug.contains("model-1"), "activity entry must show model");
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

        app.active_tab = app::Tab::Run;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| ui::render_app(frame, &app))
            .expect("draw run workspace frame");

        let debug = format!("{:?}", terminal.backend().buffer());
        assert!(
            debug.contains("req_000123"),
            "activity must show request_id"
        );
        assert!(debug.contains("gpt-5-codex"), "activity must show model_id");
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
