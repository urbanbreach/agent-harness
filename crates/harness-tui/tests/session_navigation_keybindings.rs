use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestStartedEvent, RunStartedEvent, TaskLineageMetadata, ToolCallMetadata,
    ToolCallRequestedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata, ModelOption, UiIntent};

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_session_nav_{seq:04}"),
        seq,
        run_id: "run_session_nav_tests".to_string(),
        mono_ms: seq,
        ts: Some("2026-02-03T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("session-nav-tests".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn default_navigation_keybindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("session_child_first".to_string(), "ctrl+]".to_string()),
        ("session_child_cycle".to_string(), "]".to_string()),
        ("session_child_cycle_reverse".to_string(), "[".to_string()),
        ("session_parent".to_string(), "ctrl+[".to_string()),
    ])
}

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir).expect("create run dir");
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
}

fn run_started(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_run_started",
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
        }),
    )
}

fn agent_spawned(seq: u64, agent_id: &str, profile: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )
}

fn provider_started(seq: u64, request_id: &str, provider: &str, model: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.to_string(),
            provider_id: provider.to_string(),
            model_id: model.to_string(),
            prompt_summary: "prompt summary".to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

fn child_link_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: Option<&str>,
    parent_session_id: Option<&str>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "agent.spawn".to_string(),
            args_summary: "{}".to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_session_id: parent_session_id.map(str::to_string),
                    child_session_id: child_session_id.map(str::to_string),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn session_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, Vec<EventEnvelopeV1>) {
    let parent_dir = root.join("parent");
    let child_a_dir = root.join("child_a");
    let child_b_dir = root.join("child_b");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_link_requested(4, "req_parent", "tc_child_a", Some("child_a"), None),
        child_link_requested(5, "req_parent", "tc_child_b", Some("child_b"), None),
    ];
    let child_a_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
        child_link_requested(4, "req_child_a", "tc_parent_a", None, Some("parent")),
    ];
    let child_b_events = vec![
        run_started(1),
        agent_spawned(2, "child_b", "worker-b"),
        provider_started(3, "req_child_b", "mock", "model-child-b"),
        child_link_requested(4, "req_child_b", "tc_parent_b", None, Some("parent")),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_a_dir, &child_a_events);
    write_events_jsonl(&child_b_dir, &child_b_events);

    (parent_dir, child_a_dir, child_b_dir, parent_events)
}

fn continued_runtime_model_options() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: Some("Deep work".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
        ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("creative".to_string()),
            variant_display_label: Some("Creative".to_string()),
            display_label: Some("GPT-5.4 Mini · Creative".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: Some("Deep work".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
    ]
}

#[test]
fn child_session_navigation_keybinds_follow_default_contract() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let (parent_dir, child_a_dir, child_b_dir, parent_events) = session_fixture(run_dir.path());

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut parent_app =
        AppState::new_live(Some(parent_dir.clone()), false, Some(Arc::clone(&sink)));
    parent_app.apply_keybindings(default_navigation_keybindings());
    for event in parent_events.clone() {
        parent_app.ingest_event(event);
    }
    parent_app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));

    let mut child_a_app =
        AppState::new_live(Some(child_a_dir.clone()), false, Some(Arc::clone(&sink)));
    child_a_app.apply_keybindings(default_navigation_keybindings());
    child_a_app.ingest_event(run_started(1));
    child_a_app.ingest_event(agent_spawned(2, "child_a", "worker-a"));
    child_a_app.ingest_event(provider_started(3, "req_child_a", "mock", "model-child-a"));
    child_a_app.ingest_event(child_link_requested(
        4,
        "req_child_a",
        "tc_parent_a",
        None,
        Some("parent"),
    ));
    child_a_app.handle_key(key(KeyCode::Char(']')));
    child_a_app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));

    let mut child_b_app = AppState::new_live(Some(child_b_dir.clone()), false, Some(sink));
    child_b_app.apply_keybindings(default_navigation_keybindings());
    child_b_app.ingest_event(run_started(1));
    child_b_app.ingest_event(agent_spawned(2, "child_b", "worker-b"));
    child_b_app.ingest_event(provider_started(3, "req_child_b", "mock", "model-child-b"));
    child_b_app.ingest_event(child_link_requested(
        4,
        "req_child_b",
        "tc_parent_b",
        None,
        Some("parent"),
    ));
    child_b_app.handle_key(key(KeyCode::Char('[')));

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::ReplaySession {
                run_id: "child_a".to_string(),
                run_dir: child_a_dir.clone(),
            },
            UiIntent::ReplaySession {
                run_id: "child_b".to_string(),
                run_dir: child_b_dir.clone(),
            },
            UiIntent::ReplaySession {
                run_id: "parent".to_string(),
                run_dir: parent_dir.clone(),
            },
            UiIntent::ReplaySession {
                run_id: "child_a".to_string(),
                run_dir: child_a_dir,
            },
        ]
    );
}

#[test]
fn replay_child_navigation_does_not_emit_live_intents() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let (parent_dir, _child_a_dir, _child_b_dir, parent_events) = session_fixture(run_dir.path());

    let mut app = AppState::new_replay(parent_dir, parent_events);
    app.apply_keybindings(default_navigation_keybindings());
    app.set_launch_metadata(LaunchMetadata::new(
        "planner",
        "mock",
        Some("model-parent".to_string()),
    ));

    assert_eq!(app.active_profile(), "planner");
    assert_eq!(app.current_model_label(), "model-parent");

    app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.active_profile(), "worker-a");
    assert_eq!(app.current_model_label(), "model-child-a");
    assert!(app.replay_mode);

    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(app.active_profile(), "worker-b");
    assert_eq!(app.current_model_label(), "model-child-b");

    app.handle_key(key(KeyCode::Char('[')));
    assert_eq!(app.active_profile(), "worker-a");
    assert_eq!(app.current_model_label(), "model-child-a");

    app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.active_profile(), "planner");
    assert_eq!(app.current_model_label(), "model-parent");
    assert!(app.replay_mode);
}

#[test]
fn continued_runtime_stays_primary_until_variant_cycle_sets_next_turns() {
    let variant_cycle_overrides =
        BTreeMap::from([("variant_cycle".to_string(), "tab".to_string())]);
    let options = continued_runtime_model_options();
    let primary = options[0].clone();

    let mut app = AppState::new_live(None, false, None);
    app.apply_keybindings(variant_cycle_overrides);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&primary)
            .with_available_models(options)
            .with_mode_label("Continued"),
    );

    assert_eq!(
        app.runtime_context_primary_summary(),
        "Continued runtime: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(app.runtime_context_summary_segment_text(), None);

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(
        app.runtime_context_primary_summary(),
        "Continued runtime: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(
        app.runtime_context_summary_segment_text(),
        Some("Next turns: deep · GPT-5.4 Mini".to_string())
    );
}
