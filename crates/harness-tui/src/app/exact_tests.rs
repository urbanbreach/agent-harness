use super::*;
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
            "auth".to_string(),
            "exit".to_string(),
            "help".to_string(),
            "model".to_string(),
            "new".to_string(),
            "replay".to_string(),
            "resume".to_string(),
            "status".to_string(),
            "toggles".to_string(),
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

    assert!(!app.overlay_state.slash_visible);
    assert_eq!(app.overlay_stack().top(), None);
    assert!(app.composer.prompt_buffer.is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_replay_opens_history_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/replay".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("keep this draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.overlay_state.palette_visible);
    assert!(app.overlay_state.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ReplaySession
    );
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    assert!(!app.overlay_state.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_resume_opens_history_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/resume".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("resume this draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.overlay_state.palette_visible);
    assert!(app.overlay_state.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ContinueSession
    );
    assert_eq!(app.composer.prompt_buffer, "resume this draft");
    assert!(!app.overlay_state.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_events_opens_review_surface() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/events".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("keep events draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.active_review_surface, Some(ReviewSurface::Events));
    assert_eq!(app.composer.prompt_buffer, "keep events draft");
    assert!(!app.overlay_state.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_status_opens_status_dialog_and_restores_draft() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.composer.prompt_buffer = "/status".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("status draft".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.overlay_state.status_dialog_visible);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::StatusDialog));
    assert_eq!(app.composer.prompt_buffer, "status draft");
    assert!(!app.overlay_state.slash_visible);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.overlay_state.status_dialog_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_slash_shell_closes_review_surface() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.open_review_surface(ReviewSurface::Events);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "/shell".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("back to shell".to_string());
    app.sync_slash_overlay();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.active_review_surface, None);
    assert_eq!(app.composer.prompt_buffer, "back to shell");
    assert!(!app.overlay_state.slash_visible);
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
    assert!(!app.overlay_state.slash_visible);
}

#[cfg(test)]
pub(crate) fn exact_test_live_slash_compact_emits_ui_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
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
    assert!(!app.overlay_state.slash_visible);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::CompactSession]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_auth_slash_and_palette_emit_ui_intent_mid_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut slash = AppState::new_live(
        Some(PathBuf::from("/tmp/session")),
        false,
        Some(sink.clone()),
    );
    slash.composer.prompt_buffer = "/login codex --method device".to_string();
    slash.composer.prompt_cursor = slash.composer.prompt_buffer.chars().count();
    slash.slash_draft_snapshot = Some("draft after auth".to_string());
    slash.sync_slash_overlay();

    slash.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(slash.composer.prompt_buffer, "draft after auth");
    assert!(!slash.overlay_state.slash_visible);
    assert_eq!(
        slash.status_banner.as_deref(),
        Some("auth backend requested: harness auth login codex --method device")
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
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
    for ch in "auth".chars() {
        palette.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    palette.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(!palette.overlay_state.palette_visible);
    assert_eq!(
        palette.status_banner.as_deref(),
        Some("auth backend requested: harness auth list")
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::OpenAuthManager {
                args: vec![
                    "login".to_string(),
                    "codex".to_string(),
                    "--method".to_string(),
                    "device".to_string()
                ],
                stdin: None,
            },
            UiIntent::OpenAuthManager {
                args: vec!["list".to_string()],
                stdin: None,
            }
        ]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_inventory_has_focus_hints_redaction_and_skill_selection() {
    for step in OnboardingStep::INVENTORY {
        let screen = onboarding::screen_for(step, 0);
        let text = screen
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !text.to_lowercase().contains("reference implementation"),
            "onboarding screen {} should use Harness branding only:\n{text}",
            step.snapshot_name()
        );
        assert!(
            !text.contains("oauth-access")
                && !text.contains("refresh")
                && !text.contains("acct-")
                && !text.contains("sk-"),
            "onboarding screen {} should not include secret-like values:\n{text}",
            step.snapshot_name()
        );
        assert!(
            !screen.choices.is_empty(),
            "onboarding screen {} should expose at least one selectable row",
            step.snapshot_name()
        );
        assert!(
            !screen.footer.trim().is_empty(),
            "onboarding screen {} should include key hints",
            step.snapshot_name()
        );
        if matches!(
            step,
            OnboardingStep::CodexBrowser
                | OnboardingStep::CodexDevice
                | OnboardingStep::CopilotPublicDevice
                | OnboardingStep::CopilotEnterpriseDevice
                | OnboardingStep::ApiKeyEntry
                | OnboardingStep::LoginSuccess
        ) {
            assert!(
                text.contains("redacted"),
                "onboarding screen {} should explicitly redact sensitive auth metadata:\n{text}",
                step.snapshot_name()
            );
        }
    }

    let skill_screen = onboarding::screen_for(OnboardingStep::SkillSelection, 0);
    let skill_labels = skill_screen
        .choices
        .iter()
        .map(|choice| choice.label)
        .collect::<Vec<_>>();
    assert_eq!(skill_labels, vec!["build", "plan", "explore"]);
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_skip_is_launch_local_and_writes_no_auth_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.set_onboarding_required(true);
    assert!(app.onboarding_screen().is_some());

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.onboarding_screen().expect("skip screen").step,
        OnboardingStep::SkipConfirmation
    );

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.onboarding_screen().is_none());
    assert_eq!(
        app.status_banner.as_deref(),
        Some("onboarding skipped for this launch; no credential was written")
    );
    assert!(intents.lock().expect("lock intents").is_empty());

    app.set_onboarding_required(true);
    assert!(
        app.onboarding_screen().is_none(),
        "skip should suppress onboarding only for the current AppState launch"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_auth_waits_for_backend_result() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.set_onboarding_step_for_test(OnboardingStep::CodexDevice);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.onboarding_screen().expect("codex device screen").step,
        OnboardingStep::CodexDevice,
        "onboarding must not show success before the backend reports success"
    );
    assert!(app.onboarding_auth_in_progress);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
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

    app.apply_auth_backend_result(true);

    assert!(!app.onboarding_auth_in_progress);
    assert_eq!(
        app.onboarding_screen().expect("success screen").step,
        OnboardingStep::LoginSuccess
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_api_key_emits_hidden_stdin_without_visible_secret() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.set_onboarding_step_for_test(OnboardingStep::ApiKeyEntry);
    let secret = "sk-tui-onboarding-secret-value";

    app.handle_paste(secret);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        !app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains(secret),
        "onboarding status leaked the pasted API key"
    );
    assert!(
        app.onboarding_secret_input.is_empty(),
        "secret buffer should be cleared after auth request handoff"
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "codex".to_string(),
                "--method".to_string(),
                "api-key".to_string(),
                "--api-key-stdin".to_string()
            ],
            stdin: Some(secret.to_string()),
        }]
    );
    assert_eq!(
        app.onboarding_screen().expect("api key screen").step,
        OnboardingStep::ApiKeyEntry,
        "success must wait for backend result"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_onboarding_copilot_enterprise_is_reachable_and_redacts_domain() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    let enterprise_domain = "https://github.example.test";

    app.set_onboarding_required(true);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.onboarding_screen().expect("provider screen").step,
        OnboardingStep::ProviderPick
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let target_screen = app.onboarding_screen().expect("copilot target screen");
    assert_eq!(target_screen.step, OnboardingStep::CopilotTargetPick);
    assert_eq!(
        target_screen
            .choices
            .iter()
            .map(|choice| choice.label)
            .collect::<Vec<_>>(),
        vec!["GitHub.com", "Enterprise"]
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.onboarding_screen()
            .expect("enterprise device screen")
            .step,
        OnboardingStep::CopilotEnterpriseDevice
    );

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(!app.onboarding_auth_in_progress);
    assert_eq!(
        app.status_banner.as_deref(),
        Some("enterprise login requires a domain; input stays hidden")
    );
    assert!(
        intents.lock().expect("lock intents").is_empty(),
        "blank Enterprise domain must not emit a public-fallback auth request"
    );

    app.handle_paste(enterprise_domain);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        !app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains(enterprise_domain),
        "onboarding status leaked the enterprise domain"
    );
    assert!(
        app.status_banner
            .as_deref()
            .unwrap_or_default()
            .contains("--enterprise-url <redacted>"),
        "onboarding status should redact the enterprise-url value"
    );
    assert!(
        app.onboarding_secret_input.is_empty(),
        "enterprise domain buffer should be cleared after auth request handoff"
    );
    assert_eq!(
        app.onboarding_screen()
            .expect("enterprise device screen")
            .step,
        OnboardingStep::CopilotEnterpriseDevice,
        "success must wait for backend result"
    );
    assert!(app.onboarding_auth_in_progress);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "github-copilot".to_string(),
                "--method".to_string(),
                "device".to_string(),
                "--enterprise-url".to_string(),
                enterprise_domain.to_string(),
            ],
            stdin: None,
        }]
    );

    app.apply_auth_backend_result(true);

    assert!(!app.onboarding_auth_in_progress);
    assert_eq!(
        app.onboarding_screen().expect("success screen").step,
        OnboardingStep::LoginSuccess
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
            intents.lock().expect("lock intents").push(intent);
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
    assert!(app.overlay_state.lineage_browser_visible);

    assert!(intents.lock().expect("lock intents").is_empty());
}

#[cfg(test)]
pub(crate) fn exact_test_slash_lineage_write_commands_blocked_when_live_unstable() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.ingest_event(EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_live_lineage_unstable".to_string(),
        seq: 1,
        run_id: "run_live_lineage_unstable".to_string(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(ActorKind::Worker, Some("build".to_string())),
        correlation_id: Some("req_live_lineage_unstable".to_string()),
        causation_id: None,
        stream_key: Some("req_live_lineage_unstable".to_string()),
        payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_live_lineage_unstable".to_string(),
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
    assert!(app.overlay_state.fork_selector_visible);

    app.overlay_state.fork_selector_visible = false;
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
    assert!(intents.lock().expect("lock intents").is_empty());
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
pub(crate) fn exact_test_compact_operator_rail_skips_focus_cycle() {
    let mut live = AppState::new_live(None, false, None);

    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Details);
    assert!(live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Details);
    assert!(!live.details_drawer_open());

    live.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL));
    assert_eq!(live.focus, Focus::Prompt);
    assert!(!live.details_drawer_open());

    let mut live_overlay = AppState::new_live(None, false, None);
    live_overlay.focus = Focus::Details;
    live_overlay.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
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
