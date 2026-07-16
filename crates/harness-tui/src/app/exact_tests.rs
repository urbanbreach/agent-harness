use super::*;
use crate::UnwrapOrAbort;
use std::sync::Mutex;

#[cfg(test)]
pub(crate) fn exact_test_startup_slash_commands_execute_without_menu() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(app.slash_overlay_should_render());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::SlashCommands));
    assert_eq!(
        app.slash_filtered,
        vec![
            "agents".to_string(),
            "auth".to_string(),
            "connect".to_string(),
            "exit".to_string(),
            "help".to_string(),
            "mcps".to_string(),
            "models".to_string(),
            "new".to_string(),
            "sessions".to_string(),
            "thinking".to_string(),
        ]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_slash_new_preserves_draft_and_returns_home() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/new".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("carry draft home".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.startup_shell_visible());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "carry draft home");
    assert_eq!(
        app.composer.prompt_cursor,
        "carry draft home".chars().count()
    );
    assert!(!app.should_quit);
    assert!(!app.replay_mode);
    assert!(app.session_path.is_none());
}

#[cfg(test)]
pub(crate) fn exact_test_replay_mode_disables_slash_workflow() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay"), Vec::new());
    app.focus = Focus::Prompt;
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!app.slash_visible);
    assert_eq!(app.overlay_stack().top(), None);
    assert!(app.composer.prompt_buffer.is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_resume_opens_history_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/resume".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("resume this draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.palette_visible);
    assert!(app.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ContinueSession
    );
    assert_eq!(app.composer.prompt_buffer, "resume this draft");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_events_is_removed() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/events".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("keep events draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.active_review_surface, None);
    assert_eq!(app.composer.prompt_buffer, "/events");
    assert!(app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_shell_closes_review_surface() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.open_review_surface(ReviewSurface::Help);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "/shell".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("back to shell".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.active_review_surface, None);
    assert_eq!(app.composer.prompt_buffer, "back to shell");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_follow_toggles_follow_mode() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = 12;
    app.composer.prompt_buffer = "/follow".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("follow draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.transcript_view.follow_mode);
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert_eq!(app.composer.prompt_buffer, "follow draft");
    assert!(!app.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_live_slash_compact_emits_ui_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.set_compact_session_supported(true);
    app.composer.prompt_buffer = "/compact".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("compact draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.composer.prompt_buffer, "compact draft");
    assert!(!app.slash_visible);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::CompactSession]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_auth_slash_and_palette_emit_ui_intent_mid_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut slash = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::clone(&sink)),
    );
    slash.composer.prompt_buffer = "/login codex --method device".to_string();
    slash.composer.prompt_cursor = slash.composer.prompt_buffer.chars().count();
    slash.slash_draft_snapshot = Some("draft after auth".to_string());
    slash.sync_slash_overlay();

    slash.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(slash.composer.prompt_buffer, "draft after auth");
    assert!(!slash.slash_visible);
    assert_eq!(
        slash.status_banner.as_deref(),
        Some("auth backend requested: harness auth login codex --method device")
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "device".to_string()
            ],
            stdin: None,
        }]
    );

    let mut palette = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    palette.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for ch in "connect".chars() {
        palette.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    palette.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(!palette.palette_visible);
    assert!(palette.connect_dialog.visible);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "device".to_string()
            ],
            stdin: None,
        }]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_connect_slash_command_emits_open_auth_manager() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::new(|_| {})),
    );
    app.composer.prompt_buffer = "/connect".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("draft after connect".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.composer.prompt_buffer, "draft after connect");
    assert!(!app.slash_visible);
    assert!(
        app.connect_dialog.visible,
        "/connect should open the auth dialog"
    );
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::SelectProvider,
        "dialog should start at provider selection"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_connect_slash_command_passes_provider_args() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;

        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let copilot = registry
            .get(&ProviderId::github_copilot())
            .unwrap_or_abort();
        let providers = vec![
            crate::app::ConnectProviderOption {
                id: ProviderId::codex(),
                label: codex.label().to_string(),
                description: codex.description().to_string(),
                methods: codex.auth_methods().to_vec(),
                models: vec!["gpt-5.5".to_string(), "gpt-5.4-mini".to_string()],
            },
            crate::app::ConnectProviderOption {
                id: ProviderId::github_copilot(),
                label: copilot.label().to_string(),
                description: copilot.description().to_string(),
                methods: copilot.auth_methods().to_vec(),
                models: vec!["gpt-4o".to_string()],
            },
        ];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    assert!(app.connect_dialog.visible);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        app.connect_dialog.selected_provider == Some(0),
        "first provider should be selected"
    );
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::SelectMethod,
        "should advance to method selection for OpenAI"
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::ApiKeyInput,
        "selecting API Key method should advance to key input"
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::Waiting,
        "submitting API key should enter waiting state"
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "api-key".to_string(),
                "--api-key-stdin".to_string(),
            ],
            stdin: Some("test".to_string()),
        }]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_connect_slash_command_available_in_session() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::new(|_| {})),
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(
        app.slash_filtered
            .iter()
            .any(|command| command == "connect"),
        "/connect should appear in slash command list during live session"
    );
    assert_eq!(
        crate::keybindings::slash_command_description("connect"),
        "Connect a provider"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_no_provider_banner_shown_when_disconnected() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.maybe_set_no_provider_banner();
    assert_eq!(
        app.status_banner.as_deref(),
        Some("No provider connected. Run `harness auth login` in a terminal or use /connect to set up a provider.")
    );
}

#[cfg(test)]
pub(crate) fn exact_test_apply_auth_backend_result_updates_banner() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.maybe_set_no_provider_banner();
    assert!(app.status_banner.is_some());

    app.apply_auth_backend_result(true);

    assert!(
        app.status_banner.is_none(),
        "successful auth backend result should clear the no-provider banner"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_apply_auth_backend_result_failure_shows_error() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.maybe_set_no_provider_banner();

    app.apply_auth_backend_result(false);

    assert!(
        app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains("auth backend failed"),
        "failed auth backend result should show an error banner, got: {:?}",
        app.status_banner
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_slash_compact_appears_when_supported() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::new(|_| {})),
    );
    app.set_compact_session_supported(true);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(app
        .slash_filtered
        .iter()
        .any(|command| command == "compact"));
}

#[cfg(test)]
pub(crate) fn exact_test_live_without_compact_support_hides_slash_compact() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(Arc::new(|_| {})),
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!app
        .slash_filtered
        .iter()
        .any(|command| command == "compact"));

    app.clear_prompt_input();
    for ch in "/compact".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert!(app.typed_slash_command().is_none());
    assert!(!app
        .slash_filtered
        .iter()
        .any(|command| command == "compact"));
}

#[cfg(test)]
pub(crate) fn exact_test_slash_menu_lists_lineage_commands() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    for query in ["fork", "tree", "clone"] {
        app.replace_prompt_input(format!("/{query}"));
        app.sync_slash_overlay();

        assert_eq!(app.slash_filtered, vec![query.to_string()]);
        assert_eq!(app.typed_slash_command(), Some(query));
    }

    assert_eq!(
        crate::keybindings::slash_command_description("fork"),
        "Fork session"
    );
    assert_eq!(
        crate::keybindings::slash_command_description("tree"),
        "View the Harness session tree"
    );
    assert_eq!(
        crate::keybindings::slash_command_description("clone"),
        "Prepare a Harness session clone"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_write_commands_blocked_in_replay() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay"), Vec::new());
    app.on_ui_intent = Some(sink);

    app.composer.prompt_buffer = "/fork".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    assert!(app.typed_slash_command().is_none());
    app.execute_slash_command("fork", Some("replay draft".to_string()));
    assert_eq!(app.composer.prompt_buffer, "replay draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session fork blocked: replay mode is read-only")
    );

    app.composer.prompt_buffer = "/clone".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    assert!(app.typed_slash_command().is_none());
    app.execute_slash_command("clone", Some("clone draft".to_string()));
    assert_eq!(app.composer.prompt_buffer, "clone draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session clone blocked: replay mode is read-only")
    );

    app.composer.prompt_buffer = "/tree".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    assert_eq!(app.typed_slash_command(), Some("tree"));
    app.execute_slash_command("tree", Some("tree draft".to_string()));
    assert_eq!(app.composer.prompt_buffer, "tree draft");
    assert!(app.lineage_browser_visible);

    assert!(intents.lock().unwrap_or_abort().is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_write_commands_blocked_when_live_unstable() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.ingest_event(EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_live_lineage_unstable".to_string(),
        seq: 1,
        run_id: "run_live_lineage_unstable".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_live_lineage_unstable".to_string()),
        causation_id: None,
        stream_key: Some("req_live_lineage_unstable".to_string()),
        payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_live_lineage_unstable".into(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Inspect active work".to_string(),
            request_digest: "digest-live-lineage-unstable".to_string(),
            metadata: None,
        }),
    });
    assert!(app.active_turn_in_progress());

    app.replace_prompt_input("/fork".to_string());
    app.sync_slash_overlay();
    assert_eq!(app.typed_slash_command(), Some("fork"));
    assert_eq!(app.slash_filtered, vec!["fork".to_string()]);
    app.execute_slash_command("fork", Some("fork draft".to_string()));
    assert_eq!(app.composer.prompt_buffer, "fork draft");
    assert!(app.fork_selector_visible);

    app.fork_selector_visible = false;
    app.replace_prompt_input("/clone".to_string());
    app.sync_slash_overlay();
    assert!(
        app.typed_slash_command().is_none(),
        "/clone should not type-dispatch while live work is active"
    );
    assert!(
        !app.slash_filtered.iter().any(|entry| entry == "clone"),
        "/clone should be hidden while live work is active"
    );
    app.execute_slash_command("clone", Some("clone draft".to_string()));
    assert_eq!(app.composer.prompt_buffer, "clone draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("Harness session clone blocked: live session has active work")
    );

    app.replace_prompt_input("/tree".to_string());
    app.sync_slash_overlay();
    assert_eq!(app.typed_slash_command(), Some("tree"));
    assert_eq!(app.slash_filtered, vec!["tree".to_string()]);
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_descriptions_use_harness_branding() {
    for command in ["tree", "clone"] {
        let description = crate::keybindings::slash_command_description(command);
        assert!(
            description.contains("Harness"),
            "{command} should use Harness branding: {description}"
        );

        let lower = description.to_lowercase();
        for forbidden in [
            ["open", "code"].concat(),
            ["open", "code"].join(" "),
            "codex".to_string(),
        ] {
            assert!(
                !lower.contains(&forbidden),
                "{command} description contains forbidden source brand: {description}"
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn exact_test_revert_workspace_palette_availability() {
    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay"), Vec::new());
    replay.focus = Focus::Prompt;
    assert!(!replay.palette_command_available("harness.revert_workspace"));

    let live_no_snapshot = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    assert!(!live_no_snapshot.palette_command_available("harness.revert_workspace"));

    let snapshot = EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_workspace_snapshot_001".to_string(),
        seq: 1,
        run_id: "run_snapshot".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_snapshot_001".to_string()),
        causation_id: None,
        stream_key: Some("req_snapshot_001".to_string()),
        payload: EventV1::WorkspaceSnapshot(harness_core::event::WorkspaceSnapshotEvent {
            request_id: "req_snapshot_001".into(),
            artifact_path: "artifacts/snapshot.json".to_string(),
            artifact_digest: "digest-snapshot".to_string(),
            file_count: 3,
        }),
    };
    let mut live_with_snapshot =
        AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    live_with_snapshot.ingest_event(snapshot);
    assert!(live_with_snapshot.palette_command_available("harness.revert_workspace"));
}

#[cfg(test)]
pub(crate) fn exact_test_most_recent_workspace_snapshot_request_id() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    assert!(app.most_recent_workspace_snapshot_request_id().is_none());

    app.ingest_event(EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_snapshot_first".to_string(),
        seq: 1,
        run_id: "run_snapshot".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_snapshot_first".to_string()),
        causation_id: None,
        stream_key: Some("req_snapshot_first".to_string()),
        payload: EventV1::WorkspaceSnapshot(harness_core::event::WorkspaceSnapshotEvent {
            request_id: "req_snapshot_first".into(),
            artifact_path: "artifacts/snapshot_first.json".to_string(),
            artifact_digest: "digest-first".to_string(),
            file_count: 1,
        }),
    });
    assert_eq!(
        app.most_recent_workspace_snapshot_request_id(),
        Some("req_snapshot_first".to_string())
    );

    app.ingest_event(EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_snapshot_latest".to_string(),
        seq: 2,
        run_id: "run_snapshot".into(),
        mono_ms: 2,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_snapshot_latest".to_string()),
        causation_id: None,
        stream_key: Some("req_snapshot_latest".to_string()),
        payload: EventV1::WorkspaceSnapshot(harness_core::event::WorkspaceSnapshotEvent {
            request_id: "req_snapshot_latest".into(),
            artifact_path: "artifacts/snapshot_latest.json".to_string(),
            artifact_digest: "digest-latest".to_string(),
            file_count: 2,
        }),
    });
    assert_eq!(
        app.most_recent_workspace_snapshot_request_id(),
        Some("req_snapshot_latest".to_string())
    );
}

#[cfg(test)]
pub(crate) fn exact_test_request_workspace_revert_emits_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.ingest_event(EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_snapshot_001".to_string(),
        seq: 1,
        run_id: "run_snapshot".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_snapshot_001".to_string()),
        causation_id: None,
        stream_key: Some("req_snapshot_001".to_string()),
        payload: EventV1::WorkspaceSnapshot(harness_core::event::WorkspaceSnapshotEvent {
            request_id: "req_snapshot_001".into(),
            artifact_path: "artifacts/snapshot.json".to_string(),
            artifact_digest: "digest-snapshot".to_string(),
            file_count: 3,
        }),
    });
    app.request_workspace_revert();
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::RevertWorkspace {
            snapshot_request_id: "req_snapshot_001".to_string(),
        }]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_compact_operator_rail_skips_focus_cycle() {
    let mut live = AppState::new_live(None, false, None);

    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    let mut live_overlay = AppState::new_live(None, false, None);
    live_overlay.focus = Focus::Details;
    live_overlay.live_details_drawer_open = true;
    assert!(live_overlay.details_drawer_open());

    live_overlay.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live_overlay.focus, Focus::List);
    assert!(live_overlay.details_drawer_open());

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    assert_eq!(replay.focus, Focus::Details);

    replay.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(replay.focus, Focus::Details);

    replay.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL));
    assert_eq!(replay.focus, Focus::Details);
}

#[cfg(test)]
pub(crate) fn exact_test_select_model_step_shows_models() {
    // arrange
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;
        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let providers = vec![crate::app::ConnectProviderOption {
            id: ProviderId::codex(),
            label: codex.label().to_string(),
            description: codex.description().to_string(),
            methods: codex.auth_methods().to_vec(),
            models: vec!["gpt-5.5".to_string(), "gpt-5.4-mini".to_string()],
        }];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    app.connect_dialog.step = crate::app::auth_dialog::ConnectDialogStep::Waiting;
    app.connect_dialog.selected_provider = Some(0);

    // act
    app.apply_connect_dialog_auth_result(true);

    // assert
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::SelectModel,
        "should advance to SelectModel when models are available"
    );
    assert_eq!(
        app.connect_dialog.models,
        vec!["gpt-5.5".to_string(), "gpt-5.4-mini".to_string()],
        "models should be populated from provider config"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_select_model_skip_goes_to_success() {
    // arrange
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;
        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let providers = vec![crate::app::ConnectProviderOption {
            id: ProviderId::codex(),
            label: codex.label().to_string(),
            description: codex.description().to_string(),
            methods: codex.auth_methods().to_vec(),
            models: vec!["gpt-5.5".to_string(), "gpt-5.4-mini".to_string()],
        }];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    app.connect_dialog.step = crate::app::auth_dialog::ConnectDialogStep::Waiting;
    app.connect_dialog.selected_provider = Some(0);
    app.apply_connect_dialog_auth_result(true);
    app.connect_dialog.selected = 2;

    // act
    app.handle_connect_dialog_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // assert
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::Success,
        "selecting Skip should go to Success"
    );
    assert!(
        app.connect_dialog.selected_model.is_none(),
        "selected_model should be None when skipping"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_select_model_select_goes_to_success() {
    // arrange
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;
        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let providers = vec![crate::app::ConnectProviderOption {
            id: ProviderId::codex(),
            label: codex.label().to_string(),
            description: codex.description().to_string(),
            methods: codex.auth_methods().to_vec(),
            models: vec!["gpt-5.5".to_string(), "gpt-5.4-mini".to_string()],
        }];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    app.connect_dialog.step = crate::app::auth_dialog::ConnectDialogStep::Waiting;
    app.connect_dialog.selected_provider = Some(0);
    app.apply_connect_dialog_auth_result(true);
    app.connect_dialog.selected = 0;

    // act
    app.handle_connect_dialog_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // assert
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::Success,
        "selecting a model should go to Success"
    );
    assert_eq!(
        app.connect_dialog.selected_model,
        Some(0),
        "selected_model should be set to the chosen index"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_toast_set_on_auth_success() {
    // arrange
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;
        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let providers = vec![crate::app::ConnectProviderOption {
            id: ProviderId::codex(),
            label: codex.label().to_string(),
            description: codex.description().to_string(),
            methods: codex.auth_methods().to_vec(),
            models: vec![],
        }];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    app.connect_dialog.step = crate::app::auth_dialog::ConnectDialogStep::Waiting;
    app.connect_dialog.selected_provider = Some(0);

    // act
    app.apply_connect_dialog_auth_result(true);

    // assert
    let toast = app.connect_dialog.toast.as_ref().unwrap_or_abort();
    assert!(toast.is_success, "toast should be success");
    assert_eq!(toast.message, "Connected successfully");
}

#[cfg(test)]
pub(crate) fn exact_test_toast_set_on_auth_failure() {
    // arrange
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;
        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let providers = vec![crate::app::ConnectProviderOption {
            id: ProviderId::codex(),
            label: codex.label().to_string(),
            description: codex.description().to_string(),
            methods: codex.auth_methods().to_vec(),
            models: vec![],
        }];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    app.connect_dialog.step = crate::app::auth_dialog::ConnectDialogStep::Waiting;
    app.connect_dialog.selected_provider = Some(0);

    // act
    app.apply_connect_dialog_auth_result(false);

    // assert
    let toast = app.connect_dialog.toast.as_ref().unwrap_or_abort();
    assert!(!toast.is_success, "toast should be failure");
    assert_eq!(toast.message, "Authentication failed");
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::Error,
        "should advance to Error on failure"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_any_key_closes_dialog_on_success() {
    // arrange
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    {
        use harness_core::auth::plugin::AuthPluginRegistry;
        use harness_core::auth::ProviderId;
        let registry = AuthPluginRegistry::with_builtins();
        let codex = registry.get(&ProviderId::codex()).unwrap_or_abort();
        let providers = vec![crate::app::ConnectProviderOption {
            id: ProviderId::codex(),
            label: codex.label().to_string(),
            description: codex.description().to_string(),
            methods: codex.auth_methods().to_vec(),
            models: vec![],
        }];
        app.set_connect_dialog_providers(providers);
    }
    app.open_connect_dialog();
    app.connect_dialog.step = crate::app::auth_dialog::ConnectDialogStep::Waiting;
    app.connect_dialog.selected_provider = Some(0);
    app.apply_connect_dialog_auth_result(true);
    assert_eq!(
        app.connect_dialog.step,
        crate::app::auth_dialog::ConnectDialogStep::Success,
        "should be on Success step"
    );

    // act
    app.handle_connect_dialog_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // assert
    assert!(
        !app.connect_dialog.visible,
        "dialog should be closed after any key on Success"
    );
    assert!(
        app.connect_dialog.toast.is_none(),
        "toast should be cleared after closing"
    );
}
