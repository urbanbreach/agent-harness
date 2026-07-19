use super::*;
use crate::UnwrapOrAbort;

pub(super) fn config_backed_live_launch_starts_in_session_shell_without_details_drawer() {
    set_pending_live_launch_metadata(LaunchMetadata::new(
        "deep",
        "default",
        Some("gpt-5.4-mini".to_string()),
    ));

    let app = AppState::new_live(None, false, None);

    assert!(!app.details_drawer_open());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.launch_mode_label(), None);
    assert_eq!(app.current_model_reasoning_label(), None);
}

pub(super) fn historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(envelope(
        1,
        "req_resume_1",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_resume_1".into(),
            text: "previous question".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_resume_1",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_resume_1".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "previous question".to_string(),
            request_digest: "digest-resume-1".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_resume_1",
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: "req_resume_1".into(),
            delta: "previous answer".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_resume_1",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000123".to_string().into(),
            result_summary: "previous answer".to_string(),
            result_digest: "digest-task-123".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);

    for c in "next".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents
            .iter()
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text, .. } if text == "next")),
        "historical streaming residue should not block first resumed submit"
    );
}

pub(super) fn historical_terminal_events_stay_in_session_shell_after_live_finish() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume")),
        true,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        "req_resume_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "previous run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert!(!app.completed_session_shell_active());
    assert!(!app.should_quit);
    assert_eq!(app.events.len(), 1);

    app.ingest_event(envelope(
        2,
        "req_live_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "live run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(app.should_quit);
}

pub(super) fn continued_quiescent_bootstrap_stays_in_session_shell_without_handoff() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Continued"),
    );
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
        false,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        "req_resume_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "previous run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Prompt);
    assert!(!app.composer_disabled());
}

pub(super) fn startup_ctrl_w_empty_composer_requests_new_worktree_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    assert!(app.startup_mode);
    assert!(app.composer.prompt_buffer.is_empty());

    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));

    assert!(
        app.should_quit,
        "worktree handoff should leave startup shell"
    );
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewWorktreeSession]
        ),
        "empty startup Ctrl+W must request NewWorktreeSession, not word-delete"
    );
}

pub(super) fn startup_ctrl_w_with_draft_still_deletes_word() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for c in "hello world".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(app.composer.prompt_buffer, "hello world");

    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(app.composer.prompt_buffer, "hello ");
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "draft Ctrl+W must keep word-delete, not create a worktree"
    );
}

pub(super) fn palette_new_worktree_requests_new_worktree_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.request_new_worktree_session();

    assert!(app.should_quit);
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewWorktreeSession]
        ),
        "palette/worktree path must emit NewWorktreeSession"
    );
}

pub(super) fn startup_prompt_enter_echoes_prompt_and_selects_new_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));

    for c in "ship it".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit, "startup submit should leave the launcher");
    assert!(!app.startup_shell_visible());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.prompt_history, vec!["ship it".to_string()]);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    let next_live = AppState::new_live(None, false, None);
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewSession]
        ),
        "startup submit should select a fresh session after the local prompt echo"
    );
    assert_eq!(
        next_live.composer.prompt_history,
        vec!["ship it".to_string()]
    );
    assert_eq!(
        next_live
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
}

pub(super) fn slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    for ch in "/new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.startup_shell_visible());

    app.clear_prompt_input();
    for ch in "fresh run".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert!(!app.startup_shell_visible());
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewSession]
        ),
        "/new startup handoff must select a fresh session, not submit to the old live run"
    );

    let relaunched = AppState::new_live(None, false, None);
    assert_eq!(relaunched.composer.prompt_buffer, "");
    assert_eq!(
        relaunched.composer.prompt_history,
        vec!["fresh run".to_string()]
    );
    assert_eq!(
        relaunched
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("fresh run")
    );
}

pub(super) fn startup_mode_uses_pending_launch_metadata() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let app = AppState::new_startup(Vec::new(), None);

    assert_eq!(app.active_profile(), "worker");
    assert_eq!(app.active_provider(), "mock");
    assert_eq!(app.current_model_label(), "model-1");
    assert_eq!(app.launch_mode_label(), Some("Demo"));
}

pub(super) fn lifecycle_shell_state_transitions() {
    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.composer.prompt_buffer = "draft prompt".to_string();

    assert_eq!(
        startup.lifecycle_shell_state(),
        LifecycleShellState::Startup
    );
    assert!(startup.startup_shell_visible());
    assert!(!startup.post_run_handoff_visible());
    assert!(startup.lifecycle_shell_actions_visible());
    assert_eq!(startup.runtime_state().summary, "startup ready");

    let live = AppState::new_live(None, false, None);

    assert_eq!(live.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!live.startup_shell_visible());
    assert!(!live.post_run_handoff_visible());
    assert!(!live.lifecycle_shell_actions_visible());

    let mut finished = AppState::new_live(Some(PathBuf::from("/tmp/live-finished")), false, None);
    finished.ingest_event(envelope(
        1,
        "req_lifecycle_finished",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    assert_eq!(finished.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!finished.startup_shell_visible());
    assert!(!finished.post_run_handoff_visible());
    assert!(!finished.lifecycle_shell_actions_visible());
    assert!(finished.completed_session_shell_active());
    assert!(!finished.composer_disabled());

    let mut failed = AppState::new_live(Some(PathBuf::from("/tmp/live-failed")), false, None);
    failed.ingest_event(envelope(
        1,
        "req_lifecycle_failed",
        EventV1::RunFailed(RunFailedEvent {
            error: "boom".to_string(),
        }),
    ));

    assert_eq!(failed.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!failed.post_run_handoff_visible());
    assert!(!failed.lifecycle_shell_actions_visible());
    assert!(failed.completed_session_shell_active());

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut missing_session_path = AppState::new_live(None, false, Some(fallback_sink));
    missing_session_path.ingest_event(envelope(
        1,
        "req_lifecycle_missing_path",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done without persisted path".to_string(),
        }),
    ));

    assert_eq!(
        missing_session_path.lifecycle_shell_state(),
        LifecycleShellState::None
    );
    assert!(!missing_session_path.post_run_handoff_visible());
    assert!(missing_session_path.completed_session_shell_active());
    assert!(!missing_session_path.composer_disabled());

    let mut terminal_without_routing = AppState::new_live(None, false, None);
    terminal_without_routing.ingest_event(envelope(
        1,
        "req_lifecycle_without_routing",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done without lifecycle routing".to_string(),
        }),
    ));

    assert_eq!(
        terminal_without_routing.lifecycle_shell_state(),
        LifecycleShellState::None
    );
    assert!(!terminal_without_routing.post_run_handoff_visible());
    assert!(terminal_without_routing.completed_session_shell_active());
    assert!(!terminal_without_routing.composer_disabled());
}

pub(super) fn default_shell_registry_exposes_home_and_session_shell_only() {
    let live_registry = default_shell_registry(false);
    assert_eq!(
        live_registry,
        &[
            ShellDescriptor {
                kind: ShellKind::Home,
                label: "Home",
                read_only: false,
            },
            ShellDescriptor {
                kind: ShellKind::Session,
                label: "Session",
                read_only: false,
            },
        ]
    );

    let replay_registry = default_shell_registry(true);
    assert_eq!(
        replay_registry,
        &[
            ShellDescriptor {
                kind: ShellKind::Home,
                label: "Home",
                read_only: false,
            },
            ShellDescriptor {
                kind: ShellKind::Session,
                label: "Replay",
                read_only: true,
            },
        ]
    );
}

pub(super) fn post_run_handoff_ignores_completed_turns_without_terminal_event() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_completed_turn",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_completed_turn".into(),
            text: "status?".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_completed_turn",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_completed_turn".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "status?".to_string(),
            request_digest: "digest-completed-turn".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_completed_turn",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_completed_turn".to_string().into(),
            result_summary: "all done".to_string(),
            result_digest: "digest-task-completed-turn".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.startup_shell_visible());
    assert!(!app.post_run_handoff_visible());
    assert!(!app.lifecycle_shell_actions_visible());
}

pub(super) fn replay_mode_never_reports_lifecycle_shell_actions() {
    let replay = AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            "req_replay_terminal",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(replay.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(!replay.lifecycle_shell_actions_visible());
}

pub(super) fn seed_operator_host_probes_sets_binary_update_and_jujutsu() {
    // Given: live app with no operator host probes bound yet
    let mut app = AppState::new_live(None, false, None);
    assert!(app.binary_update_summary().is_none());
    assert!(app.jujutsu_probe().is_none());

    // When: seed with an explicit workspace root (no PATH dependence on jj)
    let root = std::env::temp_dir().join(format!(
        "harness-tui-seed-probes-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create seed workspace");
    app.seed_operator_host_probes(Some(root.as_path()));
    let bin_ver = app
        .binary_version_info()
        .expect("binary version info bound");
    assert!(
        bin_ver.one_line().contains("harness") || bin_ver.one_line().contains("binary:"),
        "expected binary version: {}",
        bin_ver.one_line()
    );

    // Then: binary update multi-policy offline checks are bound honestly
    let binary = app
        .binary_update_summary()
        .expect("binary update summary bound");
    assert!(
        binary.total >= 5 && binary.checks_unavailable >= 5,
        "expected multi-channel binary update checks: {binary:?}"
    );
    assert!(!binary.update_available);
    assert!(binary.all_unavailable());
    assert!(binary.one_line().contains("update_available=false"));
    let binary_policy = app
        .binary_update_policy()
        .expect("binary update policy bound");
    assert_eq!(
        binary_policy.channel.as_deref(),
        Some("offline"),
        "expected offline channel policy: {binary_policy:?}"
    );
    let binary_check = app
        .binary_update_check()
        .expect("binary update last check bound");
    assert!(binary_check.is_unavailable());
    assert!(
        binary_check.one_line().contains("unavailable")
            || binary_check.one_line().contains("offline")
            || binary_check.one_line().contains("not"),
        "expected unavailable last check: {}",
        binary_check.one_line()
    );

    let attr = app
        .edit_attribution_summary()
        .expect("edit attribution summary bound");
    assert!(
        attr.total >= 3 && attr.agent_tool >= 1 && attr.external >= 1 && attr.drift >= 1,
        "expected multi-path attribution with agent+external+drift: {attr:?}"
    );
    assert!(attr.has_agent_tool());
    assert!(attr.has_external());
    assert!(attr.one_line().contains("agent-tool"));
    assert!(attr.one_line().contains("external"));
    let attr_first = app
        .edit_attribution_first_line()
        .expect("edit attribution first line bound");
    assert!(
        attr_first.contains("source=agent_tool") && attr_first.contains("agent.rs"),
        "expected agent-tool first line: {attr_first}"
    );
    let attr_last = app
        .edit_attribution_last_line()
        .expect("edit attribution last line bound");
    assert!(
        attr_last.contains("source=external")
            && (attr_last.contains("external.rs") || attr_last.contains("drift.rs")),
        "expected external last line: {attr_last}"
    );

    let settings = app.settings_editor_summary();
    assert!(
        settings.bound,
        "expected project config bound: {settings:?}"
    );
    assert_eq!(settings.writable_paths, 6);
    assert_eq!(settings.editable, 6);
    assert!(settings.with_effective_value >= 6);
    assert!(settings.total >= 38);
    assert!(settings.one_line().contains("bound=true"));
    assert!(settings.one_line().contains("writable_paths=6"));
    assert!(
        app.settings_project_config_path()
            .is_some_and(|path| path.ends_with("harness.json")),
        "expected harness.json project config path"
    );
    assert!(app.settings_hashline_edit());
    assert!(app.settings_compaction_enabled());
    assert!(app.settings_compaction_auto_retry_overflow());
    assert!(app.settings_compaction_structured_summary_contract());
    assert!(app.settings_compaction_estimated_token_triggers());
    assert!(!app.settings_deterministic_enabled());
    let settings_path = app
        .settings_project_config_path()
        .expect("settings path bound")
        .to_path_buf();
    assert_eq!(
        harness_core::config::read_effective_hashline_edit(&settings_path).expect("hashline"),
        app.settings_hashline_edit()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_enabled(&settings_path)
            .expect("compaction"),
        app.settings_compaction_enabled()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_auto_retry_overflow(&settings_path)
            .expect("auto_retry"),
        app.settings_compaction_auto_retry_overflow()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_structured_summary_contract(&settings_path)
            .expect("structured_summary"),
        app.settings_compaction_structured_summary_contract()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_estimated_token_triggers(&settings_path)
            .expect("estimated_token"),
        app.settings_compaction_estimated_token_triggers()
    );
    assert_eq!(
        harness_core::config::read_effective_deterministic_enabled(&settings_path)
            .expect("deterministic"),
        app.settings_deterministic_enabled()
    );

    // Then: write→reset→write product path leaves final effective values bound,
    // and registry definitions/merge strategies are resolvable for all 6 writable paths.
    let registry_json =
        harness_core::config::settings_registry_json().expect("settings registry json");
    assert!(
        registry_json.contains("hashline_edit")
            && registry_json.contains("runtime.compaction.enabled")
            && registry_json.contains("runtime.deterministic.enabled"),
        "expected writable setting ids in registry json"
    );
    let writable_ids = [
        "hashline_edit",
        "runtime.compaction.enabled",
        "runtime.compaction.auto_retry_overflow",
        "runtime.compaction.structured_summary_contract",
        "runtime.compaction.estimated_token_triggers",
        "runtime.deterministic.enabled",
    ];
    let mut editable_defs = 0usize;
    let mut replace_merge = 0usize;
    for setting_id in writable_ids {
        let def = harness_core::config::setting_definition(setting_id)
            .unwrap_or_else(|| panic!("missing setting definition for {setting_id}"));
        assert!(
            def.is_editable(),
            "expected editable writable setting {setting_id}"
        );
        editable_defs += 1;
        if matches!(
            def.merge_strategy,
            harness_core::config::SettingMergeStrategy::Replace
        ) {
            replace_merge += 1;
        }
    }
    assert_eq!(editable_defs, 6);
    assert_eq!(
        replace_merge, 6,
        "expected Replace merge strategy for scalar writable settings"
    );

    // Then: worktree product defaults are metadata-only ReadOnly registry stubs
    for setting_id in ["worktree.relative_base", "worktree.branch_prefix"] {
        assert!(
            harness_core::config::is_metadata_only_setting(setting_id),
            "expected metadata-only worktree setting {setting_id}"
        );
        let def = harness_core::config::setting_definition(setting_id)
            .unwrap_or_else(|| panic!("missing worktree setting definition for {setting_id}"));
        assert!(
            !def.is_editable(),
            "expected read-only metadata worktree setting {setting_id}"
        );
        assert!(
            def.has_default(),
            "expected default for metadata worktree setting {setting_id}"
        );
        assert!(
            matches!(
                def.merge_strategy,
                harness_core::config::SettingMergeStrategy::Replace
            ),
            "expected Replace merge for {setting_id}"
        );
    }
    assert!(
        registry_json.contains("worktree.relative_base")
            && registry_json.contains("worktree.branch_prefix"),
        "expected worktree metadata ids in registry json"
    );

    let settings_registry = app
        .settings_registry_summary()
        .expect("settings registry summary bound");
    assert!(settings_registry.total >= 38);
    assert!(settings_registry.runtime > 0);
    assert!(settings_registry.tui > 0);
    assert!(settings_registry.editable > 0);
    assert!(settings_registry.read_only > 0);
    assert!(settings_registry.secret > 0);
    assert!(settings_registry.with_default > 0);
    assert!(
        settings_registry.metadata_only >= 2,
        "expected worktree metadata-only stubs in registry: {settings_registry:?}"
    );
    assert_eq!(
        settings_registry.editable + settings_registry.read_only,
        settings_registry.total
    );
    assert!(settings_registry
        .one_line()
        .starts_with("settings registry: "));
    assert!(settings_registry.one_line().contains("runtime="));
    assert!(settings_registry.one_line().contains("tui="));

    let plan_summary = app.plan_view_summary();
    assert!(
        plan_summary.total >= 5,
        "expected multi-plan seed with active-run plan: {:?}",
        plan_summary
    );
    assert!(
        plan_summary.existing >= 5,
        "expected existing plan files: {:?}",
        plan_summary
    );
    assert!(
        plan_summary.active >= 1,
        "expected active-run plan binding: {:?}",
        plan_summary
    );
    assert!(
        plan_summary.total_bytes > 0,
        "expected plan bytes: {:?}",
        plan_summary
    );
    assert!(plan_summary.one_line().starts_with("plan view: "));
    assert!(plan_summary.one_line().contains("existing="));
    assert!(plan_summary.one_line().contains("active="));
    assert_eq!(app.run_id(), Some("harness-probe-run"));
    let plan_rows = app.plan_view_rows();
    assert!(
        plan_rows.len() >= 5,
        "expected multi-plan rows: {}",
        plan_rows.len()
    );
    assert!(
        plan_rows
            .iter()
            .any(|row| { row.slug.contains("harness-probe-plan") && row.exists }),
        "expected probe plan row: {:?}",
        plan_rows
            .iter()
            .map(|row| row.one_line())
            .collect::<Vec<_>>()
    );
    assert!(
        plan_rows
            .iter()
            .any(|row| row.slug.contains("harness-probe-run") && row.exists && row.is_active),
        "expected active-run plan row: {:?}",
        plan_rows
            .iter()
            .map(|row| row.one_line())
            .collect::<Vec<_>>()
    );

    // Then: multi-report crash scan is seeded under workspace/.harness-sessions-probe
    let crash = app
        .crash_recovery_scan_summary()
        .expect("crash recovery scan summary bound");
    assert!(
        crash.scanned >= 5,
        "expected multi-report crash probe fixtures: {crash:?}"
    );
    assert!(
        crash.previous_crash >= 1 && crash.clean >= 1,
        "expected previous-crash + clean mix: {crash:?}"
    );
    assert!(crash.one_line().contains("previous-crash"));
    let crash_first = app
        .crash_recovery_first_report()
        .expect("crash recovery first report bound");
    assert!(crash_first.previous_crash_detected);
    assert!(!crash_first.events_log_present);
    let crash_action = app
        .crash_recovery_resolved_action()
        .expect("crash recovery resolved action bound");
    assert_eq!(crash_action.as_str(), "reopen_session");

    // Then: offline mock ACP connect+bind success path is seeded honestly
    let acp = app
        .acp_connection_summary()
        .expect("acp connection summary bound");
    let acp_connect = app.acp_last_connect().expect("acp last connect bound");
    assert!(
        acp_connect.one_line().contains("ok") || acp_connect.is_connected(),
        "expected mock ACP connect ok: {}",
        acp_connect.one_line()
    );
    let acp_bind = app.acp_last_bind().expect("acp last bind bound");
    assert!(
        acp_bind.one_line().contains("ok") && acp_bind.one_line().contains("harness.probe.agent"),
        "expected mock ACP bind ok: {}",
        acp_bind.one_line()
    );
    assert!(acp.is_bound());
    assert!(
        acp.one_line().contains("harness.probe.agent")
            || acp.agent_name.as_deref() == Some("harness.probe.agent"),
        "expected bound ACP agent: {}",
        acp.one_line()
    );
    let acp_session = app.acp_session_info().expect("acp session info bound");
    assert_eq!(acp_session.agent_name, "harness.probe.agent");
    assert!(!acp_session.session_id.is_empty());
    let fallback = app
        .auto_fallback_summary()
        .expect("auto fallback summary bound");
    // Full chain walk: primary → fb1 → fb2 → fb3 → fb4 → Exhausted (remaining=0).
    assert_eq!(fallback.remaining, 0);
    assert!(
        fallback.chain_len >= 5,
        "expected longer multi-fallback chain: {fallback:?}"
    );
    assert!(fallback.exhausted);
    let fallback_outcome = app
        .auto_fallback_last_outcome()
        .expect("auto fallback last outcome bound");
    assert!(
        fallback_outcome.is_exhausted(),
        "expected Exhausted after full chain walk: {}",
        harness_core::auto_fallback::describe_auto_fallback_outcome(&fallback_outcome)
    );
    let banner = app
        .auto_fallback_last_banner()
        .expect("auto fallback last banner bound");
    assert!(
        banner.contains("exhausted") && banner.contains("(probe):fb2"),
        "expected exhausted banner after fb2: {banner}"
    );
    let models = app
        .auto_fallback_chain_label()
        .expect("auto fallback chain label bound");
    assert!(
        models.contains("(probe):primary")
            && models.contains("(probe):fb1")
            && models.contains("(probe):fb2"),
        "expected full probe chain label: {models}"
    );
    let plugins = app
        .plugin_lifecycle_summary()
        .expect("plugin lifecycle summary bound");
    assert!(
        plugins.installed >= 2 && plugins.enabled >= 1 && plugins.disabled >= 1,
        "expected multi-plugin lifecycle installed/enabled/disabled: {plugins:?}"
    );
    let plugin_install = app
        .plugin_last_install()
        .expect("plugin last install bound");
    assert!(
        plugin_install.one_line().contains("plugin install: ok"),
        "expected successful probe install: {}",
        plugin_install.one_line()
    );
    assert!(
        plugin_install.one_line().contains("harness.probe.plugin"),
        "expected probe plugin id (primary or secondary): {}",
        plugin_install.one_line()
    );
    let plugin_activate = app
        .plugin_last_activate()
        .expect("plugin last activate bound");
    assert!(
        plugin_activate.one_line().contains("plugin activate: ok"),
        "expected successful probe activate: {}",
        plugin_activate.one_line()
    );
    let plugin_deactivate = app
        .plugin_last_deactivate()
        .expect("plugin last deactivate bound");
    assert!(
        plugin_deactivate
            .one_line()
            .contains("plugin deactivate: ok"),
        "expected successful probe deactivate: {}",
        plugin_deactivate.one_line()
    );
    let plugin_remove = app.plugin_last_remove().expect("plugin last remove bound");
    assert!(
        plugin_remove.one_line().contains("plugin remove: failed"),
        "missing-remove-probe should fail closed: {}",
        plugin_remove.one_line()
    );
    let plugin_first = app.plugin_first_line().expect("plugin first line bound");
    assert!(
        plugin_first.contains("harness.probe.plugin"),
        "expected first plugin line: {plugin_first}"
    );
    assert!(
        plugin_first.contains("enablement=enabled") || plugin_first.contains("enablement=disabled"),
        "expected enablement state on multi-plugin first line: {plugin_first}"
    );
    assert!(
        plugin_deactivate
            .one_line()
            .contains("harness.probe.plugin.secondary")
            || plugin_deactivate.one_line().contains("secondary"),
        "expected secondary deactivate last: {}",
        plugin_deactivate.one_line()
    );

    // Then: multi-descriptor discover + primary probe loaded (descriptor-only; no code load)
    let discover = app
        .extension_discover_summary()
        .expect("extension discover summary bound");
    assert!(
        discover.discovered >= 3,
        "expected multi-descriptor discover (primary+alt+tools[+plugin]): discovered={}",
        discover.discovered
    );
    assert!(!discover.loads_external_code);
    let summary = app
        .extension_manifest_summary()
        .expect("extension manifest summary bound");
    assert_eq!(summary.extension_id, "harness.probe.extension");
    assert!(
        summary.one_line().contains("harness.probe.extension")
            || summary.one_line().contains("extension descriptor:"),
        "expected probe descriptor one_line: {}",
        summary.one_line()
    );
    assert!(
        summary.capabilities >= 1 && summary.enabled_capabilities >= 1,
        "expected primary probe capability counts: caps={} enabled={}",
        summary.capabilities,
        summary.enabled_capabilities
    );
    assert!(
        summary.tools >= 1,
        "expected primary probe tool count: tools={}",
        summary.tools
    );
    assert!(!summary.loads_external_code);
    let load = app
        .extension_last_load()
        .expect("extension last load bound");
    assert!(
        load.one_line().contains("ok") && load.one_line().contains("harness.probe.extension"),
        "expected Loaded primary probe load: {}",
        load.one_line()
    );

    // Then: empty team/demote/remote-auth outcome tallies are seeded; cron seeds probe
    // schedules with executes=true product honesty.
    let teams = app
        .team_registry_summary()
        .expect("team registry summary bound");
    assert!(
        teams.teams >= 2 && teams.active >= 1 && teams.cancelled >= 1,
        "expected multi-team registry with active+cancelled: {teams:?}"
    );
    assert!(teams.members >= 2, "expected multi-member teams: {teams:?}");
    assert!(
        teams.mailbox_messages >= 1,
        "expected multi-message mailbox: {teams:?}"
    );
    let team_create = app.team_last_create().expect("team last create bound");
    assert!(
        team_create.one_line().contains("ok") || team_create.one_line().contains("(probe)"),
        "expected probe team create: {}",
        team_create.one_line()
    );
    let team_first = app.team_first_line().expect("team first bound");
    assert!(
        team_first.contains("(probe)") && team_first.contains("cancelled"),
        "expected cancelled probe team first after cancel success: {team_first}"
    );
    let team_send = app.team_last_send().expect("team last send bound");
    assert!(
        team_send.one_line().contains("ok") || team_send.one_line().contains("probe"),
        "expected probe team send: {}",
        team_send.one_line()
    );
    let team_msg = app
        .team_last_message_line()
        .expect("team last message bound");
    assert!(
        team_msg.contains("probe") || team_msg.contains("mailbox"),
        "expected probe mailbox message: {team_msg}"
    );
    let team_add = app
        .team_last_add_member()
        .expect("team last add-member bound");
    assert!(
        team_add.one_line().contains("ok") || team_add.one_line().contains("probe-agent"),
        "expected probe team add-member: {}",
        team_add.one_line()
    );
    let team_cancel = app.team_last_cancel().expect("team last cancel bound");
    assert!(
        team_cancel.one_line().contains("team cancel: ok"),
        "expected successful cancel of probe team: {}",
        team_cancel.one_line()
    );
    let cron = app
        .cron_schedule_summary()
        .expect("cron schedule summary bound");
    assert!(
        cron.registered >= 4 && cron.with_label >= 3,
        "expected multi-schedule cron registry after remove: {cron:?}"
    );
    assert!(!cron.executes_schedules);
    let cron_reg = app.cron_last_register().expect("cron last register bound");
    assert!(
        cron_reg.one_line().contains("ok") && cron_reg.one_line().contains("(probe-5)"),
        "expected last multi-register outcome for probe-5: {}",
        cron_reg.one_line()
    );
    let cron_remove = app.cron_last_remove().expect("cron last remove bound");
    assert!(
        cron_remove.one_line().contains("cron remove: ok")
            && cron_remove.one_line().contains("(probe)"),
        "expected successful remove of first probe schedule: {}",
        cron_remove.one_line()
    );
    let cron_first = app
        .cron_first_schedule_line()
        .expect("cron first schedule bound");
    assert!(
        cron_first.contains("(probe-2)") && cron_first.contains("executes=true"),
        "expected remaining probe-2 first schedule: {cron_first}"
    );
    let demote = app
        .demote_outcome_summary()
        .expect("demote outcome summary bound");
    // Multi demote probes: shell unavailable + multi reject + multi demote (total>=5)
    assert!(
        demote.total >= 5 && demote.demoted >= 2,
        "expected multi-handle demote batch: {demote:?}"
    );
    assert!(
        demote.unavailable >= 1 && demote.rejected >= 2,
        "expected unavailable+rejected+demoted mix: {demote:?}"
    );
    assert_eq!(
        demote.demoted + demote.unavailable + demote.rejected,
        demote.total
    );
    let demote_last = app.demote_last_result().expect("demote last result bound");
    assert!(
        demote_last.is_unavailable() || demote_last.is_rejected(),
        "expected shell probe unavailable/rejected: {}",
        demote_last.one_line()
    );
    let demote_task = app
        .demote_last_task_result()
        .expect("demote last task result bound");
    assert!(
        demote_task.is_demoted(),
        "expected demotable task success path: {}",
        demote_task.one_line()
    );
    assert!(
        demote_task.one_line().contains("probe-task-ok")
            || demote_task.one_line().contains("demoted"),
        "expected demoted task one_line: {}",
        demote_task.one_line()
    );
    let hub = app
        .workspace_hub_outcome_summary()
        .expect("workspace hub outcome summary bound");
    assert_eq!(hub.total, 4);
    assert_eq!(hub.connect_unavailable, 1);
    assert_eq!(hub.bind_unavailable, 1);
    assert_eq!(hub.upload_unavailable, 1);
    assert_eq!(hub.recover_unavailable, 1);
    assert!(hub.all_unavailable());
    let hub_connect = app
        .workspace_hub_last_connect()
        .expect("workspace hub last connect bound");
    assert!(
        hub_connect.one_line().contains("unavailable"),
        "expected unavailable connect: {}",
        hub_connect.one_line()
    );
    let hub_bind = app
        .workspace_hub_last_bind()
        .expect("workspace hub last bind bound");
    assert!(
        hub_bind.one_line().contains("unavailable")
            && hub_bind.one_line().contains("(probe-workspace-2)"),
        "expected multi-endpoint last unavailable bind: {}",
        hub_bind.one_line()
    );
    let hub_upload = app
        .workspace_hub_last_upload()
        .expect("workspace hub last upload bound");
    assert!(
        hub_upload.one_line().contains("unavailable")
            && hub_upload.one_line().contains("(probe-bundle)"),
        "expected multi-endpoint last unavailable upload: {}",
        hub_upload.one_line()
    );
    let hub_recover = app
        .workspace_hub_last_recover()
        .expect("workspace hub last recover bound");
    assert!(
        hub_recover.one_line().contains("unavailable")
            && hub_recover.one_line().contains("(probe-session-stale)"),
        "expected multi-endpoint last unavailable recover: {}",
        hub_recover.one_line()
    );
    let hub_avail = app
        .workspace_hub_availability()
        .expect("workspace hub availability bound");
    assert!(hub_avail.is_unavailable());
    assert!(hub_avail.one_line().contains("unavailable"));
    let oidc = app
        .browser_oidc_outcome_summary()
        .expect("browser oidc outcome summary bound");
    assert_eq!(oidc.total, 2);
    assert_eq!(oidc.start_unavailable, 1);
    assert_eq!(oidc.complete_unavailable, 1);
    assert!(oidc.all_unavailable());
    let oidc_start = app
        .browser_oidc_last_start()
        .expect("browser oidc last start bound");
    assert!(
        oidc_start
            .one_line()
            .contains("issuer=`https://issuer.example`")
            && oidc_start.one_line().contains("client=`(client-web)`"),
        "expected multi-endpoint last OIDC start: {}",
        oidc_start.one_line()
    );
    let oidc_complete = app
        .browser_oidc_last_complete()
        .expect("browser oidc last complete bound");
    assert!(
        oidc_complete.one_line().contains("code=`(pro…`"),
        "expected redacted multi-endpoint OIDC complete: {}",
        oidc_complete.one_line()
    );
    assert!(!oidc_complete.one_line().contains("probe-device"));
    let oidc_avail = app
        .browser_oidc_availability()
        .expect("browser oidc availability bound");
    assert!(oidc_avail.is_unavailable());
    assert!(oidc_avail.one_line().contains("unavailable"));
    let mcp = app
        .mcp_oauth_outcome_summary()
        .expect("mcp oauth outcome summary bound");
    assert_eq!(mcp.total, 3);
    assert_eq!(mcp.begin_unavailable, 1);
    assert_eq!(mcp.exchange_unavailable, 1);
    assert_eq!(mcp.open_unavailable, 1);
    assert!(mcp.all_unavailable());
    let mcp_begin = app
        .mcp_oauth_last_begin()
        .expect("mcp oauth last begin bound");
    assert!(
        mcp_begin.one_line().contains("server=`(probe-remote)`")
            && mcp_begin
                .one_line()
                .contains("url=`https://mcp-auth.example/authorize`"),
        "expected multi-endpoint last MCP OAuth begin: {}",
        mcp_begin.one_line()
    );
    let mcp_exchange = app
        .mcp_oauth_last_exchange()
        .expect("mcp oauth last exchange bound");
    assert!(
        mcp_exchange.one_line().contains("server=`(probe-remote)`")
            && mcp_exchange.one_line().contains("code=`(pro…`"),
        "expected multi-endpoint last MCP OAuth exchange: {}",
        mcp_exchange.one_line()
    );
    assert!(!mcp_exchange.one_line().contains("probe-device"));
    let mcp_open = app
        .mcp_oauth_last_open()
        .expect("mcp oauth last open bound");
    assert!(
        mcp_open.one_line().contains("server=`(probe-remote)`")
            && mcp_open
                .one_line()
                .contains("endpoint=`https://mcp.example/message`"),
        "expected multi-endpoint last MCP open: {}",
        mcp_open.one_line()
    );
    let mcp_avail = app
        .mcp_oauth_remote_availability()
        .expect("mcp oauth remote availability bound");
    assert!(mcp_avail.is_unavailable());
    assert!(mcp_avail.one_line().contains("unavailable"));
    let sleep = app
        .sleep_wake_observation_summary()
        .expect("sleep/wake observation summary bound");
    assert!(
        sleep.total >= 8 && sleep.recorded >= 8,
        "expected dual-cycle sleep/wake observations: {sleep:?}"
    );
    assert_eq!(sleep.recorded_noop, 0);
    assert!(
        app.sleep_wake_observation_log().len() >= 8,
        "expected dual-cycle observation log: {}",
        app.sleep_wake_observation_log().len()
    );
    let sleep_last = app
        .sleep_wake_last_observation()
        .expect("sleep/wake last observation bound");
    assert!(
        sleep_last.one_line().contains("suspend") || sleep_last.one_line().contains("recorded"),
        "expected last multi-event observation (suspend): {}",
        sleep_last.one_line()
    );
    assert!(sleep_last.is_recorded());
    assert!(!sleep_last.is_recorded_noop());
    let sleep_decision = app
        .sleep_wake_last_decision()
        .expect("sleep/wake last decision bound");
    assert!(sleep_decision.is_skip());
    assert!(!sleep_decision.claims_refresh());
    assert!(
        sleep_decision.one_line().contains("skip refresh")
            && sleep_decision.one_line().contains("suspend"),
        "expected skip decision for last suspend: {}",
        sleep_decision.one_line()
    );
    let sleep_policy = app
        .sleep_wake_credential_policy()
        .expect("sleep/wake credential policy bound");
    assert!(sleep_policy.is_active());
    assert!(
        sleep_policy.one_line().contains("active (strategy=hook)"),
        "expected active hook policy: {}",
        sleep_policy.one_line()
    );
    let sleep_avail = app
        .sleep_wake_availability()
        .expect("sleep/wake availability bound");
    assert!(
        sleep_avail.one_line().contains("active"),
        "expected active availability: {}",
        sleep_avail.one_line()
    );

    // When: apply one more host event through product API (not seed-only; no expiry)
    let decision =
        app.apply_sleep_wake_host_event(harness_core::sleep_wake_auth::SleepWakeHostEvent::Wake);
    // Then: decision + summary advance; still skip without expiry snapshot
    assert!(decision.is_skip());
    assert!(!decision.claims_refresh());
    assert!(decision.one_line().contains("wake"));
    let sleep_after = app
        .sleep_wake_observation_summary()
        .expect("sleep/wake summary after apply");
    assert!(sleep_after.total >= 9);
    assert!(sleep_after.recorded >= 9);
    assert!(
        app.sleep_wake_last_decision()
            .is_some_and(|d| d.one_line().contains("wake")),
        "expected last decision for wake"
    );

    // When: wake with credentials near expiry
    let now = 1_700_000_000_000i64;
    let near_expiry = harness_core::sleep_wake_auth::CredentialExpirySnapshot {
        expires_at_unix_ms: Some(now + 30_000),
        now_unix_ms: now,
        leeway_ms: harness_core::sleep_wake_auth::DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS,
    };
    let refresh_decision = app.apply_sleep_wake_host_event_with_expiry(
        harness_core::sleep_wake_auth::SleepWakeHostEvent::Wake,
        Some(&near_expiry),
    );
    // Then: policy recommends refresh; infrastructure is Active hook strategy
    assert!(refresh_decision.is_refresh());
    assert!(refresh_decision.claims_refresh());
    assert!(
        refresh_decision.one_line().contains("refresh recommended")
            && refresh_decision.one_line().contains("remaining_ms=30000"),
        "expected near-expiry refresh recommendation: {}",
        refresh_decision.one_line()
    );
    let policy_after = app
        .sleep_wake_credential_policy()
        .expect("sleep/wake policy after near-expiry wake");
    assert!(policy_after.is_active());
    assert!(policy_after.one_line().contains("strategy=hook"));

    // Then: jujutsu probe is bound with .jj marker (repo workspace; CLI may be available or not)
    let probe = app.jujutsu_probe().expect("jujutsu probe bound");
    assert!(
        probe.one_line().contains("ready="),
        "expected ready flag in jujutsu one_line: {}",
        probe.one_line()
    );
    assert!(
        probe.workspace.is_repo(),
        "expected .jj marker repo workspace: {}",
        probe.one_line()
    );
    assert!(
        !probe.is_ready() || probe.cli.is_available(),
        "ready only when CLI available: {}",
        probe.one_line()
    );
    let jj_cli = app.jujutsu_cli().expect("jujutsu cli bound");
    assert!(
        jj_cli.one_line().contains("jujutsu") || jj_cli.one_line().contains("jj"),
        "expected jujutsu cli one_line: {}",
        jj_cli.one_line()
    );
    let jj_ws = app.jujutsu_workspace().expect("jujutsu workspace bound");
    assert!(
        jj_ws.is_repo(),
        "expected jujutsu workspace repo: {}",
        jj_ws.one_line()
    );
    // Then: multi-command walk ends on jj status (ok or unavailable; structured)
    let jj_cmd = app
        .jujutsu_last_command()
        .expect("jujutsu last command bound");
    assert!(
        jj_cmd.one_line().contains("jujutsu command:") && jj_cmd.one_line().contains("status"),
        "expected last jujutsu command status: {}",
        jj_cmd.one_line()
    );
    assert!(
        jj_cmd.is_ok() || jj_cmd.is_unavailable(),
        "expected structured ok|unavailable: {}",
        jj_cmd.one_line()
    );

    // Then: COW worktree availability is probed for the workspace root
    let cow = app
        .cow_worktree_availability()
        .expect("cow worktree availability bound");
    assert!(
        cow.one_line().contains("COW worktree fastpath:"),
        "expected COW fastpath one_line: {}",
        cow.one_line()
    );
    // Then: multi-path COW clone batch is bound (src + missing + dest-exists)
    let cow_last = app
        .cow_clone_last_result()
        .expect("cow clone last result bound");
    assert!(
        cow_last.one_line().contains("COW clone:"),
        "expected COW clone last one_line: {}",
        cow_last.one_line()
    );
    assert!(
        cow_last.is_unavailable(),
        "expected last clone dest-exists unavailable: {}",
        cow_last.one_line()
    );
    assert!(
        cow_last.one_line().contains("dst-exists.bin")
            || cow_last.one_line().contains("already exists"),
        "expected dest-exists last path: {}",
        cow_last.one_line()
    );
    let cow_summary = app
        .cow_clone_outcome_summary()
        .expect("cow clone outcome summary bound");
    assert!(
        cow_summary.total >= 5 && cow_summary.unavailable >= 3,
        "expected multi-path COW clone batch: {cow_summary:?}"
    );
    assert_eq!(
        cow_summary.cloned + cow_summary.unavailable,
        cow_summary.total
    );
    assert!(cow_summary.one_line().contains("total"));

    // Then: persistent graph product builds simple index + multi-kind batch
    let graph = app
        .persistent_graph_availability()
        .expect("persistent graph availability bound");
    assert!(
        graph.is_available() || graph.one_line().contains("persistent graph: unavailable"),
        "expected structured persistent graph one_line: {}",
        graph.one_line()
    );
    let batch = app
        .graph_query_batch_summary()
        .expect("multi-kind graph batch summary bound");
    // Multi-symbol multi-kind probe batch (3 symbols × 4 kinds)
    assert!(
        batch.total >= 8,
        "expected multi-symbol multi-kind graph batch: {batch:?}"
    );
    // symbol_def can hit; callers/callees/references stay unavailable on simple index
    assert!(
        batch.unavailable >= 1 || batch.hit_results >= 1,
        "expected structured batch counts: {batch:?}"
    );
    let batch_first = app
        .graph_query_batch_first_line()
        .expect("graph batch first line bound");
    assert!(
        batch_first.contains("symbol_def") && batch_first.contains("(probe)"),
        "expected first batch one_line: {batch_first}"
    );
    let graph_last = app
        .graph_query_last_result()
        .expect("graph query last result bound");
    assert!(
        graph_last.one_line().contains("references")
            || graph_last.one_line().contains("graph query unavailable")
            || graph_last.one_line().contains("graph query hit"),
        "expected last query one_line: {}",
        graph_last.one_line()
    );
    assert!(
        batch.one_line().contains("graph batch:")
            && (batch.one_line().contains("total")
                || batch.one_line().contains("unavailable")
                || batch.one_line().contains("hit")),
        "expected multi-symbol graph batch one_line: {}",
        batch.one_line()
    );

    // Then: Landlock host support is probed (presence ≠ confinement)
    let landlock = app.landlock_support().expect("landlock support bound");
    assert!(
        landlock.one_line().contains("Landlock:"),
        "expected Landlock one_line: {}",
        landlock.one_line()
    );

    // Then: sandbox FS plan is bound after multi-policy walk (last non-Off = Strict; plan-only, not enforcement)
    let sandbox = app
        .sandbox_fs_plan_summary()
        .expect("sandbox fs plan summary bound");
    let os_profiles = app
        .os_sandbox_profiles_summary()
        .expect("os sandbox profiles summary bound");
    assert_eq!(
        os_profiles.total,
        harness_core::sandbox::OS_SANDBOX_POLICIES.len()
    );
    assert_eq!(
        os_profiles.available + os_profiles.unavailable,
        os_profiles.total
    );
    assert!(os_profiles.total >= 4, "expected full OS policy inventory");
    let os_first = app
        .os_sandbox_first_profile_line()
        .expect("os sandbox first profile bound");
    assert!(
        os_first.contains("policy=off") || os_first.contains("OS sandbox profile"),
        "expected first profile (off): {os_first}"
    );
    let sandbox_prep = app
        .sandbox_last_prepare()
        .expect("sandbox last prepare bound");
    assert!(
        sandbox_prep.one_line().contains("sandbox prepare")
            && sandbox_prep.one_line().contains("strict"),
        "expected multi-policy last prepare (strict): {}",
        sandbox_prep.one_line()
    );
    assert_eq!(sandbox.policy, harness_core::sandbox::SandboxPolicy::Strict);
    assert!(sandbox.read_root_count >= 1);
    assert!(sandbox.write_root_count >= 1);
    assert!(
        sandbox.one_line().contains("strict") || sandbox.one_line().contains("read_roots="),
        "expected Strict multi-policy last plan one_line: {}",
        sandbox.one_line()
    );

    let _ = std::fs::remove_dir_all(&root);
}

pub(super) fn seed_operator_host_probes_binds_crash_scan_and_foreign_discover() {
    // Given: isolated sessions root with one clean run and one previous-crash run
    let root = std::env::temp_dir().join(format!(
        "harness-tui-seed-session-probes-{}-{}",
        std::process::id(),
        "sessions"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create sessions root");

    let clean = root.join("run_clean");
    std::fs::create_dir_all(&clean).expect("create clean run");
    std::fs::write(clean.join("events.jsonl"), b"").expect("events");

    let crashed = root.join("run_crashed");
    std::fs::create_dir_all(&crashed).expect("create crashed run");
    std::fs::write(crashed.join(".writer.lock.recovering"), b"").expect("recovery marker");

    // Given: foreign scan root with one importable events.jsonl candidate
    let foreign_root = std::env::temp_dir().join(format!(
        "harness-tui-seed-foreign-{}-{}",
        std::process::id(),
        "scan"
    ));
    let _ = std::fs::remove_dir_all(&foreign_root);
    let foreign_session = foreign_root.join("foreign_events");
    std::fs::create_dir_all(&foreign_session).expect("create foreign session");
    std::fs::write(
        foreign_session.join("events.jsonl"),
        br#"{"schema_version":1,"event_id":"evt_foreign_1","seq":1,"run_id":"run_foreign","mono_ms":1,"actor":{"kind":"system"},"payload":{"event_type":"run_finished","data":{"summary":"imported"}}}
"#,
    )
    .expect("foreign events marker");

    let mut app = AppState::new_live(None, false, None);
    assert!(app.crash_recovery_scan_summary().is_none());
    assert!(app.foreign_discover_summary().is_none());

    // When: seed with explicit sessions + foreign roots
    app.seed_operator_host_probes_with_roots(
        Some(root.as_path()),
        Some(root.as_path()),
        Some(foreign_root.as_path()),
    );

    // Then: crash scan summary reflects multi-report root (test fixtures + probe fixtures)
    let crash = app
        .crash_recovery_scan_summary()
        .expect("crash recovery scan summary bound");
    assert!(
        crash.scanned >= 3,
        "expected multi-report scan (clean+crashed+stale[+test fixtures]): {crash:?}"
    );
    assert!(
        crash.previous_crash >= 1,
        "expected previous-crash count: {crash:?}"
    );
    assert!(crash.clean >= 1, "expected clean count: {crash:?}");
    assert!(crash.one_line().contains("previous-crash"));
    let crash_action = app
        .crash_recovery_resolved_action()
        .expect("crash recovery resolved action bound from first report");
    assert_eq!(
        crash_action.as_str(),
        "reopen_session",
        "crashed fixture has no events.jsonl → not resumable"
    );
    let run_id = crash_action.operator_hint("run_crashed");
    assert!(
        run_id.contains("run_crashed"),
        "operator hint should carry run id: {run_id}"
    );
    let crash_first = app
        .crash_recovery_first_report()
        .expect("crash recovery first report bound");
    assert!(crash_first.previous_crash_detected);
    assert!(
        crash_first.recovery_marker_present || crash_first.stale_writer_lock,
        "expected recovery marker or stale lock: {crash_first:?}"
    );
    assert!(!crash_first.events_log_present);
    assert!(
        crash_first
            .run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name == "run_crashed"
                    || name == "harness_probe_crashed"
                    || name == "harness_probe_stale"
            }),
        "first crash report should be a previous-crash fixture: {:?}",
        crash_first.run_dir
    );

    // Then: foreign discover summary sees multi-source importable markers
    let foreign = app
        .foreign_discover_summary()
        .expect("foreign discover summary bound");
    assert!(
        foreign.total >= 3,
        "expected multi-source foreign discover: {foreign:?}"
    );
    assert!(
        foreign.discoverable >= 3 && foreign.importable >= 3,
        "expected multi-source importable: {foreign:?}"
    );
    assert!(foreign.has_importable());
    assert!(foreign.one_line().contains("importable"));
    // Then: first importable candidate is imported into probe dest (Imported outcome)
    let first = app
        .foreign_import_first_candidate()
        .expect("foreign import first candidate bound");
    assert!(first.is_importable(), "expected importable first candidate");
    let import_last = app
        .foreign_import_last_outcome()
        .expect("foreign import last outcome bound");
    assert!(
        import_last.one_line().contains("foreign import:")
            && (import_last.one_line().contains("ok")
                || import_last.one_line().contains("imported")
                || import_last.one_line().contains("run=")),
        "expected successful import one_line: {}",
        import_last.one_line()
    );

    // Then: binary + jujutsu + sandbox still seeded
    assert!(app.binary_update_summary().is_some());
    assert!(app.jujutsu_probe().is_some());
    let sandbox = app
        .sandbox_fs_plan_summary()
        .expect("sandbox fs plan summary bound");
    assert_eq!(
        sandbox.policy,
        harness_core::sandbox::SandboxPolicy::Strict,
        "multi-policy FS plan walk binds last non-Off plan (Strict)"
    );
    assert!(sandbox.one_line().contains("read_roots="));
    assert!(sandbox.one_line().contains("write_roots="));

    let _ = std::fs::remove_dir_all(&root);
}
