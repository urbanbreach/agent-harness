use super::*;

pub(super) fn toggles_slash_command_opens_command_styled_menu() {
    // Given: launch metadata still advertises legacy primary and child profiles.
    let mut app = AppState::new();
    app.focus = Focus::Prompt;
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()])
            .with_available_models(vec![
                ModelOption::from_model_ref("build", "default:gpt-5.4-mini"),
                ModelOption::from_model_ref("explore", "default:gpt-5.4-mini"),
            ]),
    );

    for ch in "/toggles".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    // When: the toggles menu is rendered.
    assert!(app.toggles_menu_visible);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::TogglesMenu));
    let rendered = render_debug(&app, 100, 40);
    assert!(rendered.contains("Built-in dynamic"));
    assert!(rendered.contains("Always approve mode"));
    // Then: primary profiles are absent while preserved subagents remain available.
    assert!(!rendered.contains("build"), "{rendered}");
    assert!(!rendered.contains("plan"), "{rendered}");
    assert!(rendered.contains("explore"), "{rendered}");
    assert!(app
        .toggle_menu_rows()
        .iter()
        .all(|row| row.section != "Agents"));
    assert!(app
        .toggle_menu_rows()
        .iter()
        .any(|row| row.section == "Subagents"));
}

pub(super) fn yolo_toggle_changes_coordinator_mode_after_confirmation() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            sink.lock().unwrap_or_abort().push(intent);
        })),
    );
    app.set_toggles_config(TogglesConfig {
        entries: vec![
            ToggleEntryConfig {
                kind: ToggleEntryKind::Hook {
                    id: "pre-submit".to_string(),
                },
                label: "Pre-submit hook".to_string(),
                description: "Run before submitting".to_string(),
                enabled: false,
            },
            ToggleEntryConfig {
                kind: ToggleEntryKind::YoloMode,
                label: "YOLO mode".to_string(),
                description: "Auto-approve ordinary tool permissions".to_string(),
                enabled: false,
            },
        ],
    });
    app.open_toggles_menu();
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    assert!(app.toggles_yolo_confirmation_visible());
    assert!(render_debug(&app, 100, 28).contains("Confirm always-approve mode"));

    app.handle_key(key(KeyCode::Enter));
    assert!(!app.toggles_yolo_confirmation_visible());
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::SetAlwaysApproveMode { enabled: true }]
    );
    assert!(!app.always_approve_mode());
    app.set_always_approve_mode(true);
    let rows = app.toggle_menu_rows();
    assert!(rows
        .iter()
        .any(|row| row.label == "YOLO mode" && row.enabled));
    assert!(rows
        .iter()
        .any(|row| row.label == "Pre-submit hook" && !row.enabled));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        intents.lock().unwrap_or_abort().last(),
        Some(&UiIntent::SetAlwaysApproveMode { enabled: false })
    );
    app.set_always_approve_mode(false);
    assert!(app
        .toggle_menu_rows()
        .iter()
        .any(|row| row.label == "YOLO mode" && !row.enabled));
}

pub(super) fn toggles_config_drops_primary_profiles_and_keeps_subagents() {
    let mut app = AppState::new();
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_switchable_profiles(vec!["build".to_string()])
            .with_available_models(vec![ModelOption::from_model_ref(
                "explore",
                "default:gpt-5.4-mini",
            )]),
    );

    app.set_toggles_config(TogglesConfig::default());

    let rows = app.toggle_menu_rows();
    assert!(rows
        .iter()
        .any(|row| row.label == "Built-in dynamic prompts"));
    assert!(rows.iter().all(|row| row.label != "build"));
    assert!(rows.iter().any(|row| row.label == "explore"));
}

pub(super) fn toggles_config_drops_primary_agents_and_keeps_subagents() {
    // Given: a runtime config still sends legacy agent and subagent toggles.
    let mut app = AppState::new();
    app.set_toggles_config(TogglesConfig {
        entries: vec![
            ToggleEntryConfig {
                kind: ToggleEntryKind::Agent {
                    name: "build".to_string(),
                },
                label: "build".to_string(),
                description: "Primary agent".to_string(),
                enabled: true,
            },
            ToggleEntryConfig {
                kind: ToggleEntryKind::Subagent {
                    name: "explore".to_string(),
                },
                label: "explore".to_string(),
                description: "Subagent profile".to_string(),
                enabled: true,
            },
            ToggleEntryConfig {
                kind: ToggleEntryKind::Hook {
                    id: "pre-submit".to_string(),
                },
                label: "Pre-submit hook".to_string(),
                description: "Run before submitting".to_string(),
                enabled: true,
            },
        ],
    });

    // When: visible toggle rows are projected.
    let rows = app.toggle_menu_rows();

    // Then: the generic hook and subagent remain while the primary agent is filtered out.
    assert!(rows.iter().all(|row| row.label != "build"));
    assert!(rows.iter().any(|row| row.label == "explore"));
    assert!(rows.iter().any(|row| row.label == "Pre-submit hook"));
}
pub(super) fn toggles_menu_sanitizes_config_derived_text() {
    let mut app = AppState::new();
    app.set_toggles_config(TogglesConfig {
        entries: vec![ToggleEntryConfig {
            kind: ToggleEntryKind::Hook {
                id: "hook\u{1b}".to_string(),
            },
            label: "hook\u{1b}[31m".to_string(),
            description: "first\nsecond".to_string(),
            enabled: true,
        }],
    });
    app.open_toggles_menu();

    let rendered = render_debug(&app, 140, 40);
    assert!(rendered.contains("hook[31m"));
    assert!(rendered.contains("first"));
    assert!(rendered.contains("second"));
    assert!(!rendered.contains('\u{1b}'));
    assert!(!rendered.contains("first\\nsecond"));
}

#[test]
fn approval_shortcuts_toggle_live_mode_and_preserve_overlay_ownership() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            sink.lock().unwrap_or_abort().push(intent);
        })),
    );
    let shortcut = key_with_modifiers(KeyCode::Char('o'), KeyModifiers::CONTROL);
    app.handle_key(shortcut);
    assert!(!app.always_approve_mode());
    app.set_always_approve_mode(true);
    super::super::palette_controller::dispatch_palette_command(&mut app, "model.always_approve");
    assert!(!app.toggles_menu_visible);
    app.set_always_approve_mode(false);
    for c in "/yolo".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[
            UiIntent::SetAlwaysApproveMode { enabled: true },
            UiIntent::SetAlwaysApproveMode { enabled: false },
            UiIntent::SetAlwaysApproveMode { enabled: true },
        ]
    );
    intents.lock().unwrap_or_abort().clear();
    app.execute_action(Action::OpenSettings);
    app.handle_key(shortcut);
    assert!(intents.lock().unwrap_or_abort().is_empty());
    app.handle_key(key(KeyCode::Esc));
    app.replay_mode = true;
    app.request_always_approve_mode_change(true);
    super::super::palette_controller::dispatch_palette_command(&mut app, "model.always_approve");
    assert!(intents.lock().unwrap_or_abort().is_empty());
}
