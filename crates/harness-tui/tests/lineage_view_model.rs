use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus, SCHEMA_VERSION,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::app::{AppState, SessionHistoryEntry};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn catalog_entry(run_id: &str, parent: Option<&str>, updated: &str) -> SessionCatalogEntry {
    SessionCatalogEntry {
        run_id: run_id.to_string(),
        run_name: Some(run_id.replace('-', " ")),
        status: Some(RunStatus::Finished),
        last_updated_at: Some(updated.to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: Some("build".to_string()),
        provider_model: Some("default/gpt-5.5".to_string()),
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: parent.map(str::to_string),
    }
}

fn history_entry(run_id: &str, parent: Option<&str>, updated: &str) -> SessionHistoryEntry {
    SessionHistoryEntry {
        run_dir: PathBuf::from("/runs").join(run_id),
        catalog: catalog_entry(run_id, parent, updated),
    }
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_lineage_tui_{seq:04}"),
        seq,
        run_id: "run_fork_source".to_string(),
        mono_ms: seq,
        ts: Some(format!("2026-05-03T00:0{seq}:00Z")),
        actor: EventActor::new(
            ActorKind::System,
            Some("lineage-view-model-test".to_string()),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_fork_source".to_string()),
        payload,
    }
}

fn fork_source_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "fork source".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_1".to_string(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-tool-1".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished before tool result".to_string(),
            }),
        ),
        envelope(
            4,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_1".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("ok".to_string()),
                output_digest: Some("digest-output-1".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
    ]
}

#[test]
fn lineage_tree_navigation_filters_and_folds() {
    let mut app = AppState::new_live(Some(PathBuf::from("/runs/child-new")), false, None);
    app.set_session_history_entries(vec![
        history_entry("child-old", Some("root"), "2026-05-03T00:01:00Z"),
        history_entry("grandchild", Some("child-new"), "2026-05-03T00:03:00Z"),
        history_entry("root", None, "2026-05-03T00:00:00Z"),
        history_entry("child-new", Some("root"), "2026-05-03T00:02:00Z"),
    ]);

    app.open_lineage_browser();
    let rows = app.lineage_browser_view_model().rows;
    assert_eq!(
        rows.iter()
            .map(|row| (row.depth, row.run_id.as_str(), row.current))
            .collect::<Vec<_>>(),
        vec![
            (0, "root", false),
            (1, "child-new", true),
            (2, "grandchild", false),
            (1, "child-old", false),
        ]
    );

    app.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(
        app.lineage_browser_view_model()
            .rows
            .iter()
            .map(|row| row.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["root"]
    );

    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(ctrl('n'));
    assert_eq!(
        app.lineage_browser_view_model().selected_run_id.as_deref(),
        Some("grandchild")
    );
    app.handle_key(ctrl('p'));
    assert_eq!(
        app.lineage_browser_view_model().selected_run_id.as_deref(),
        Some("child-new")
    );

    for ch in "grand".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let filtered = app.lineage_browser_view_model();
    assert_eq!(filtered.filter_input, "grand");
    assert_eq!(
        filtered
            .rows
            .iter()
            .map(|row| row.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "child-new", "grandchild"]
    );

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.lineage_browser_visible);
}

#[test]
fn fork_selector_excludes_unstable_cutoffs() {
    let mut app = AppState::new_live(Some(PathBuf::from("/runs/source")), false, None);
    for event in fork_source_events() {
        app.ingest_event(event);
    }

    app.open_fork_selector();
    let initial = app.fork_selector_view_model();
    assert_eq!(
        initial
            .rows
            .iter()
            .map(|row| row.cutoff_seq)
            .collect::<Vec<_>>(),
        vec![4]
    );
    assert_eq!(initial.selected_cutoff_seq, Some(4));

    for ch in "0003".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(app.fork_selector_view_model().rows.is_empty());
    app.handle_key(key(KeyCode::Enter));
    assert!(app.confirmed_fork_prefix().is_none());
    assert!(app.fork_selector_visible);

    for _ in 0..4 {
        app.handle_key(key(KeyCode::Backspace));
    }
    app.handle_key(key(KeyCode::Char('4')));
    app.handle_key(key(KeyCode::Enter));

    let confirmed = app
        .confirmed_fork_prefix()
        .expect("stable cutoff confirmed");
    assert_eq!(confirmed.cutoff_seq, 4);
    assert_eq!(confirmed.event_count, 4);
    assert!(!app.fork_selector_visible);
}

#[test]
fn lineage_view_model_empty_and_single_tree_states_are_deterministic() {
    let mut empty = AppState::new_live(None, false, None);
    empty.open_lineage_browser();
    let empty_vm = empty.lineage_browser_view_model();
    assert!(empty_vm.rows.is_empty());
    assert_eq!(empty_vm.empty_message.as_deref(), Some("No saved sessions"));

    let mut single = AppState::new_live(Some(PathBuf::from("/runs/solo")), false, None);
    single.set_session_history_entries(vec![history_entry("solo", None, "2026-05-03T00:00:00Z")]);
    single.open_lineage_browser();
    let single_vm = single.lineage_browser_view_model();
    assert_eq!(single_vm.empty_message, None);
    assert_eq!(single_vm.rows.len(), 1);
    assert_eq!(single_vm.rows[0].run_id, "solo");
    assert_eq!(single_vm.rows[0].depth, 0);
    assert!(single_vm.rows[0].current);
}

#[test]
fn resumed_live_lineage_browser_uses_preloaded_session_history() {
    let mut app = AppState::new_live_with_session_history(
        Some(PathBuf::from("/runs/child")),
        false,
        None,
        vec![
            history_entry("root", None, "2026-05-03T00:00:00Z"),
            history_entry("child", Some("root"), "2026-05-03T00:01:00Z"),
        ],
    );

    app.open_lineage_browser();
    let vm = app.lineage_browser_view_model();

    assert_eq!(vm.empty_message, None);
    assert_eq!(
        vm.rows
            .iter()
            .map(|row| (row.depth, row.run_id.as_str(), row.current))
            .collect::<Vec<_>>(),
        vec![(0, "root", false), (1, "child", true)]
    );
}
