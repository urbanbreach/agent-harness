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
    assert!(rendered.contains("YOLO mode"));
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

pub(super) fn yolo_toggle_requires_confirmation_and_enables_entries() {
    let mut app = AppState::new();
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
                description: "Enable all session toggles".to_string(),
                enabled: false,
            },
        ],
    });
    app.open_toggles_menu();
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    assert!(app.toggles_yolo_confirmation_visible());
    assert!(render_debug(&app, 100, 28).contains("Confirm YOLO mode"));

    app.handle_key(key(KeyCode::Enter));
    assert!(!app.toggles_yolo_confirmation_visible());
    assert!(app.toggle_menu_rows().iter().all(|row| row.enabled));
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
