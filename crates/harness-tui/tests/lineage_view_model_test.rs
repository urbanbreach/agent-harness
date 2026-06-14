use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EditAppliedEvent, EditProposedEvent, EventActor, EventEnvelopeV1,
    EventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunStartedEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
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
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_1".to_string(),
                parent_agent_id: None,
                profile: "build".to_string(),
            }),
        ),
        envelope(
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_first".to_string(),
                text: "First prompt".to_string(),
            }),
        ),
        envelope(
            4,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_first".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "First prompt".to_string(),
                request_digest: "digest-req-first".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_1".to_string(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-tool-1".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            6,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_unstable".to_string(),
                text: "Unstable prompt".to_string(),
            }),
        ),
        envelope(
            7,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_1".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("ok".to_string()),
                output_digest: Some("digest-output-1".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            8,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_first".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-first-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_unstable".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Unstable prompt".to_string(),
                request_digest: "digest-req-unstable".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            10,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_unstable".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-unstable-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            11,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_second".to_string(),
                text: "Second prompt".to_string(),
            }),
        ),
    ]
}

fn fork_source_events_with_completed_native_edit() -> Vec<EventEnvelopeV1> {
    let mut events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "fork source".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_1".to_string(),
                parent_agent_id: None,
                profile: "build".to_string(),
            }),
        ),
    ];

    push_completed_turn(&mut events, "req_first", "First prompt");
    let edit_insert_at = events.len() - 1;
    events.insert(
        edit_insert_at,
        envelope(
            5,
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit-tool_1".to_string(),
                path: "demo.txt".to_string(),
                summary: "rewrite file through native edit tool".to_string(),
                patch_digest: "digest-native-edit".to_string(),
            }),
        ),
    );
    events.insert(
        edit_insert_at + 1,
        envelope(
            6,
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "create-demo".to_string(),
                path: "demo.txt".to_string(),
                new_file_digest: "digest-demo".to_string(),
                diff_rel_path: Some("artifacts/toolcalls/edit-create-demo.diff".to_string()),
                diff_digest: Some("digest-demo-diff".to_string()),
            }),
        ),
    );
    resequence_events(&mut events);

    push_completed_turn(&mut events, "req_second", "Second prompt");
    push_completed_turn(&mut events, "req_third", "Third prompt");
    push_completed_turn(&mut events, "req_fourth", "Fourth prompt");
    push_completed_turn(&mut events, "req_fifth", "Fifth prompt");
    events
}

fn fork_source_events_with_lingering_native_edit_state() -> Vec<EventEnvelopeV1> {
    let mut events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "fork source".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_1".to_string(),
                parent_agent_id: None,
                profile: "build".to_string(),
            }),
        ),
    ];

    push_completed_turn(&mut events, "req_first", "First prompt");
    events.push(envelope(
        events.len() as u64 + 1,
        EventV1::EditProposed(EditProposedEvent {
            edit_id: "edit-tool_1".to_string(),
            path: "demo.txt".to_string(),
            summary: "rewrite file through native edit tool".to_string(),
            patch_digest: "digest-native-edit".to_string(),
        }),
    ));
    push_completed_turn(&mut events, "req_second", "Second prompt");
    push_completed_turn(&mut events, "req_third", "Third prompt");
    push_completed_turn(&mut events, "req_fourth", "Fourth prompt");
    push_completed_turn(&mut events, "req_fifth", "Fifth prompt");
    events
}

fn push_completed_turn(events: &mut Vec<EventEnvelopeV1>, request_id: &str, text: &str) {
    let next_seq = events.len() as u64 + 1;
    events.extend([
        envelope(
            next_seq,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: text.to_string(),
            }),
        ),
        envelope(
            next_seq + 1,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: text.to_string(),
                request_digest: format!("digest-{request_id}"),
                metadata: None,
            }),
        ),
        envelope(
            next_seq + 2,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("digest-{request_id}-output")),
                usage: None,
                metadata: None,
            }),
        ),
    ]);
}

fn resequence_events(events: &mut [EventEnvelopeV1]) {
    for (index, event) in events.iter_mut().enumerate() {
        let seq = index as u64 + 1;
        event.seq = seq;
        event.event_id = format!("evt_lineage_tui_{seq:04}");
        event.mono_ms = seq;
        event.ts = Some(format!("2026-05-03T00:{:02}:00Z", seq.min(59)));
    }
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
    assert!(!app.overlay_state.lineage_browser_visible);
}

#[test]
fn fork_selector_lists_user_messages_like_reference_selector() {
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
            .map(|row| (row.cutoff_seq, row.prompt_text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (11, "Full session"),
            (10, "Second prompt"),
            (5, "Unstable prompt"),
            (2, "First prompt"),
        ]
    );
    assert_eq!(initial.selected_cutoff_seq, Some(11));

    for ch in "unstable".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert_eq!(
        app.fork_selector_view_model()
            .rows
            .iter()
            .map(|row| (row.cutoff_seq, row.prompt_text.as_str()))
            .collect::<Vec<_>>(),
        vec![(5, "Unstable prompt")]
    );
    app.handle_key(key(KeyCode::Enter));

    let confirmed = app
        .confirmed_fork_prefix()
        .expect("reference-style selected user message confirmed");
    assert_eq!(confirmed.cutoff_seq, 5);
    assert_eq!(confirmed.event_count, 5);
    assert!(!app.overlay_state.fork_selector_visible);

    app.open_fork_selector();

    for _ in 0..8 {
        app.handle_key(key(KeyCode::Backspace));
    }
    for ch in "second".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let confirmed = app
        .confirmed_fork_prefix()
        .expect("stable cutoff confirmed");
    assert_eq!(confirmed.cutoff_seq, 10);
    assert_eq!(confirmed.event_count, 10);
    assert!(!app.overlay_state.fork_selector_visible);
}

#[test]
fn fork_selector_keeps_later_messages_after_completed_native_edit() {
    let mut app = AppState::new_live(Some(PathBuf::from("/runs/source")), false, None);
    for event in fork_source_events_with_completed_native_edit() {
        app.ingest_event(event);
    }

    app.open_fork_selector();
    let rows = app.fork_selector_view_model().rows;

    assert_eq!(
        rows.iter()
            .map(|row| row.prompt_text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Full session",
            "Fifth prompt",
            "Fourth prompt",
            "Third prompt",
            "Second prompt",
            "First prompt",
        ]
    );
    assert_eq!(rows.iter().filter(|row| row.event_id.is_some()).count(), 5);
}

#[test]
fn fork_selector_keeps_later_messages_after_lingering_native_edit_state() {
    let mut app = AppState::new_live(Some(PathBuf::from("/runs/source")), false, None);
    for event in fork_source_events_with_lingering_native_edit_state() {
        app.ingest_event(event);
    }

    app.open_fork_selector();
    let rows = app.fork_selector_view_model().rows;

    assert_eq!(
        rows.iter()
            .map(|row| row.prompt_text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Full session",
            "Fifth prompt",
            "Fourth prompt",
            "Third prompt",
            "Second prompt",
            "First prompt",
        ]
    );
    assert_eq!(rows.iter().filter(|row| row.event_id.is_some()).count(), 5);
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
