use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn slash_exit_from_inline_subagent_restores_parent_before_quit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).unwrap_or_abort();

    let mut app = AppState::new_live(Some(parent_path), false, Some(intent_sink));
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_exit",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "inspect child".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));

    app.navigate_to_child_session_id("agent_child".to_string());
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode);

    app.execute_slash_command("exit", None);

    assert!(app.should_quit);
    assert_eq!(app.current_session_id(), Some("parent_run"));
    assert!(!app.replay_mode);
    assert!(!app.current_subagent_session_present());
    let intents = intents.lock().unwrap_or_abort();
    assert!(intents
        .iter()
        .any(|intent| matches!(intent, UiIntent::QuitRequested)));
}
