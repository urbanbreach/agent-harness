use super::*;

mod events {
    include!("opencode_subagent_parity_events.rs");
}

pub(super) fn no_child_app() -> AppState {
    let mut app = AppState::new_live(Some(inline_parent_path()), false, None);
    app.apply_keybindings(default_navigation_keybindings());
    app.ingest_event(events::run_started(1));
    app.ingest_event(events::agent_spawned_with_parent(
        2, "parent", "build", None,
    ));
    app.ingest_event(envelope(
        3,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(4, "req_parent", "default", "model-parent"));
    app.focus = Focus::Details;
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Up));
    app
}

pub(super) fn foreground_running_app() -> AppState {
    subagent_app(events::TaskFixtureState::Running)
}

pub(super) fn background_affordance_app() -> AppState {
    let mut running = foreground_running_app();
    running.focus = Focus::Details;
    running.handle_key(key_with_modifiers(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    ));
    running.ingest_event(events::detached_foreground_tool_call_event(11));
    running
}

pub(super) fn completed_app() -> AppState {
    subagent_app(events::TaskFixtureState::Completed)
}

pub(super) fn retry_app() -> AppState {
    subagent_app(events::TaskFixtureState::Retrying)
}

pub(super) fn background_completed_app() -> AppState {
    subagent_app(events::TaskFixtureState::BackgroundCompleted)
}

pub(super) fn child_footer_app() -> AppState {
    let mut app = AppState::new_replay(
        inline_parent_path(),
        events::subagent_events(events::TaskFixtureState::Completed),
    );
    app.apply_keybindings(default_navigation_keybindings());
    app.navigate_to_child_session_id("agent_worker".to_string());
    app
}

pub(super) fn sibling_after_navigation_app() -> AppState {
    let mut app = AppState::new_replay(inline_parent_path(), events::sibling_events());
    app.apply_keybindings(default_navigation_keybindings());
    app.focus = Focus::Details;
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Left));
    app
}

fn subagent_app(state: events::TaskFixtureState) -> AppState {
    let mut app = AppState::new_live(Some(inline_parent_path()), false, None);
    app.apply_keybindings(default_navigation_keybindings());
    for event in events::subagent_events(state) {
        app.ingest_event(event);
    }
    app
}

fn inline_parent_path() -> PathBuf {
    tempfile::Builder::new()
        .prefix("harness-parity-inline-")
        .tempdir()
        .expect("create inline parity tempdir")
        .path()
        .join("parent")
}
