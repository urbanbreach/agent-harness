use super::*;

#[test]
fn action_from_str_roundtrip() {
    // arrange
    // act
    // assert
    let action = Action::Quit;
    let s = action.as_str();
    let parsed = Action::from_str(s).unwrap();
    assert_eq!(action, parsed);
}

#[test]
fn legacy_primary_mode_action_is_rejected() {
    // arrange
    // Given: the retired primary-mode action and the supported model-variant action.
    // When: both identifiers are parsed from keybinding configuration.
    let legacy_mode = Action::from_str("cycle_mode");
    let model_variant = Action::from_str("variant_cycle");

    // act
    // Then: only the model control remains configurable.
    // assert
    assert!(legacy_mode.is_err());
    assert_eq!(model_variant, Ok(Action::VariantCycle));
}

#[test]
fn command_metadata_covers_palette_and_slash_commands() {
    // arrange
    let palette_commands = Action::palette_commands();
    let slash_commands = slash_commands();

    // act
    for command in palette_commands {
        let metadata = command_metadata(command.metadata_id)
            .unwrap_or_else(|| panic!("missing palette metadata for {}", command.id));

        // assert
        assert!(!metadata.label.trim().is_empty(), "{} label", command.id);
        assert!(
            !metadata.description.trim().is_empty(),
            "{} description",
            command.id
        );
        assert_eq!(Action::palette_command_label(command.id), metadata.label);
        assert_eq!(
            Action::palette_command_description(command.id),
            metadata.description
        );
    }

    for command in slash_commands {
        let metadata = command_metadata(command.metadata_id)
            .unwrap_or_else(|| panic!("missing slash metadata for {}", command.id));

        assert!(
            !metadata.description.trim().is_empty(),
            "{} description",
            command.id
        );
        assert_eq!(slash_command_description(command.id), metadata.description);
    }
}

#[test]
fn slash_and_palette_shared_commands_use_same_metadata() {
    // arrange
    let shared_commands = [
        ("models", "switch_model"),
        ("toggles", "toggles"),
        ("help", "help"),
        ("shell", "close_review_surface"),
        ("follow", "toggle_follow"),
        ("exit", "quit"),
    ];

    // act
    for (slash_command, palette_command) in shared_commands {
        let slash_description = slash_command_description(slash_command);
        let palette_description = Action::palette_command_description(palette_command);

        // assert
        assert_eq!(
            slash_description, palette_description,
            "{slash_command} should reuse {palette_command} metadata"
        );
    }
}

#[test]
fn help_actions_use_shared_command_metadata() {
    // arrange
    let help_actions = [
        Action::MoveDown,
        Action::MoveUp,
        Action::FocusNext,
        Action::FocusPrev,
        Action::ToggleFollow,
        Action::Reload,
        Action::CloseReviewSurface,
        Action::ToggleTerminalPanel,
        Action::SubmitPrompt,
        Action::InsertNewline,
        Action::ClearPrompt,
        Action::HistoryUp,
        Action::HistoryDown,
        Action::AllowPermission,
        Action::DenyPermission,
        Action::DismissModal,
        Action::DiffHunkNext,
        Action::DiffHunkPrevious,
        Action::Help,
        Action::Quit,
    ];

    // act
    for action in help_actions {
        let metadata_label = action.metadata_label();
        let metadata_description = action.metadata_description();

        // assert
        assert!(
            !metadata_label.trim().is_empty(),
            "{} label",
            action.as_str()
        );
        assert!(
            !metadata_description.trim().is_empty(),
            "{} description",
            action.as_str()
        );
    }
}

#[test]
fn key_binding_parses_ctrl_p() {
    // arrange
    // act
    // assert
    let binding = KeyBinding::from_str("ctrl+p").unwrap();
    assert_eq!(binding.code, KeyCode::Char('p'));
    assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
}

#[test]
fn key_binding_parses_single_char() {
    // arrange
    // act
    // assert
    let binding = KeyBinding::from_str("q").unwrap();
    assert_eq!(binding.code, KeyCode::Char('q'));
    assert_eq!(binding.modifiers, KeyModifiers::NONE);
}

#[test]
fn keymap_finds_default_binding() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    assert_eq!(keymap.get_action(&event), Some(Action::Quit));
}

#[test]
fn keymap_override_replaces_default_binding() {
    // arrange
    // act
    // assert
    let mut overrides = BTreeMap::new();
    overrides.insert("quit".to_string(), "x".to_string());

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    let event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(keymap.get_action(&event), Some(Action::Quit));

    // Default 'q' should no longer work
    let old_event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    assert_ne!(keymap.get_action(&old_event), Some(Action::Quit));
}

#[test]
fn keymap_invalid_override_preserves_default_binding() {
    // arrange
    // act
    // assert
    let mut overrides = BTreeMap::new();
    overrides.insert("quit".to_string(), "not-a-key".to_string());

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        Some(Action::Quit)
    );
    let quit_bindings = keymap.get_binding_strs(Action::Quit);
    assert!(
        quit_bindings
            .iter()
            .any(|b| b == "q" || b == "Ctrl+q" || b == "Ctrl+d"),
        "quit bindings include freeze chords: {quit_bindings:?}"
    );
}

#[test]
fn keymap_override_collision_removes_stale_session_label() {
    // arrange
    // act
    // assert
    let mut overrides = BTreeMap::new();
    overrides.insert("session_child_cycle".to_string(), "left".to_string());

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    assert_eq!(
        keymap.get_session_action(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        Some(Action::SessionChildCycle)
    );
    assert_eq!(keymap.get_binding_str(Action::SessionChildCycle), "←");
    assert_eq!(
        keymap.get_binding_str(Action::SessionChildCycleReverse),
        "-"
    );
}

#[test]
fn keymap_returns_binding_str() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let bindings = keymap.get_binding_strs(Action::Quit);
    assert!(
        bindings
            .iter()
            .any(|b| b == "q" || b == "Ctrl+q" || b == "Ctrl+d"),
        "quit bindings present: {bindings:?}"
    );
}

#[test]
fn keymap_returns_ctrl_binding_str() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let bindings = keymap.get_binding_strs(Action::Palette);
    assert!(
        bindings.iter().any(|b| b == "Ctrl+p" || b == "?"),
        "palette bindings present: {bindings:?}"
    );
}

#[test]
fn keymap_formats_binding_labels_from_overrides() {
    // arrange
    // act
    // assert
    let mut overrides = BTreeMap::new();
    overrides.insert("quit".to_string(), "x".to_string());

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    assert_eq!(keymap.get_binding_label(Action::Quit, "quit"), "x quit");
}

#[test]
fn keymap_binds_shift_enter_to_insert_newline() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_binds_ctrl_j_to_insert_newline() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_binds_ctrl_enter_to_insert_newline() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_binds_alt_enter_to_insert_newline() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_uses_ctrl_y_and_ctrl_n_for_permission_decisions() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();

    let allow = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
    let deny = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);

    assert_eq!(keymap.get_action(&allow), Some(Action::AllowPermission));
    assert_eq!(keymap.get_action(&deny), Some(Action::DenyPermission));
    assert_eq!(keymap.get_binding_str(Action::AllowPermission), "Ctrl+y");
    assert_eq!(keymap.get_binding_str(Action::DenyPermission), "Ctrl+n");
}

#[test]
fn keymap_uses_ctrl_o_for_always_approve_permission() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();

    let always = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert_eq!(
        keymap.get_action(&always),
        Some(Action::AlwaysApprovePermission)
    );
    assert_eq!(
        keymap.get_binding_str(Action::AlwaysApprovePermission),
        "Ctrl+o"
    );
}

#[test]
fn keymap_binds_child_session_navigation_to_default_bindings() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        Some(Action::SessionChildFirst)
    );
    assert_eq!(
        keymap.get_session_action(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        Some(Action::SessionChildCycle)
    );
    assert_eq!(
        keymap.get_session_action(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        Some(Action::SessionChildCycleReverse)
    );
    assert_eq!(
        keymap.get_session_action(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        Some(Action::SessionParent)
    );
    assert_eq!(
        keymap.get_session_action(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
        Some(Action::SessionBackground)
    );
    assert_eq!(
        keymap.get_binding_str(Action::SessionChildFirst),
        "Ctrl+x ↓"
    );
    assert_eq!(keymap.get_binding_str(Action::SessionChildCycle), "→");
    assert_eq!(
        keymap.get_binding_str(Action::SessionChildCycleReverse),
        "←"
    );
    assert_eq!(keymap.get_binding_str(Action::SessionParent), "↑");
    assert_eq!(keymap.get_binding_str(Action::SessionBackground), "Ctrl+b");
}

#[test]
fn keymap_binds_diff_hunk_navigation_to_default_bindings() {
    // arrange
    let keymap = KeyMap::with_defaults();

    // act
    let next_binding = keymap.get_action(&KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT));

    // assert
    assert_eq!(next_binding, Some(Action::DiffHunkNext));
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT)),
        Some(Action::DiffHunkPrevious)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char(']'), KeyModifiers::ALT)),
        Some(Action::DiffHunkNext)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('['), KeyModifiers::ALT)),
        Some(Action::DiffHunkPrevious)
    );
    assert_eq!(keymap.get_binding_str(Action::DiffHunkNext), "Alt+n");
    assert_eq!(keymap.get_binding_str(Action::DiffHunkPrevious), "Alt+p");
    assert_eq!(Action::from_str("diff_hunk_next"), Ok(Action::DiffHunkNext));
    assert_eq!(
        Action::from_str("diff_hunk_previous"),
        Ok(Action::DiffHunkPrevious)
    );
}

#[test]
fn ws8_keyboard_surfaces_use_registry_actions_instead_of_hardcoded_keys() {
    // arrange
    let keymap = KeyMap::with_defaults();
    let required_actions = [
        Action::MoveDown,
        Action::MoveUp,
        Action::HistoryDown,
        Action::HistoryUp,
        Action::SubmitPrompt,
        Action::SessionChildFirst,
        Action::SessionChildCycle,
        Action::SessionChildCycleReverse,
        Action::SessionParent,
        Action::SessionBackground,
        Action::DiffHunkNext,
        Action::DiffHunkPrevious,
    ];

    // act
    let key_interaction_source = include_str!("../app/key_interaction.rs");
    let mouse_interaction_source = include_str!("../app/mouse_interaction.rs");
    let sidebar_interaction_source = include_str!("../ui_secondary/sidebar_interaction.rs");

    // assert
    for action in required_actions {
        assert!(
            !keymap.get_bindings(action).is_empty(),
            "{} must have a configurable default binding",
            action.as_str()
        );
        assert_eq!(
            Action::from_str(action.as_str()),
            Ok(action),
            "{} must round-trip through config action ids",
            action.as_str()
        );
    }

    assert_no_key_checks(
        "operator sidebar keyboard navigation",
        source_between(
            mouse_interaction_source,
            "fn operator_sidebar_keyboard_active",
            "    pub(crate) fn set_frame_area",
        ),
    );
    assert_no_key_checks(
        "session child navigation dispatch",
        source_between(
            key_interaction_source,
            "Action::SessionChildFirst =>",
            "            Action::DiffHunkNext =>",
        ),
    );
    assert_no_key_checks(
        "diff hunk navigation dispatch",
        source_between(
            key_interaction_source,
            "Action::DiffHunkNext =>",
            "            Action::VariantCycle =>",
        ),
    );

    assert_no_key_checks(
        "operator sidebar keyboard rendering",
        source_between(
            sidebar_interaction_source,
            "pub(crate) fn operator_sidebar_keyboard_targets",
            "fn operator_sidebar_body_width_for_frame",
        ),
    );
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source
        .find(start)
        .unwrap_or_else(|| panic!("missing source marker: {start}"));
    let after_start = &source[start_index..];
    let end_index = after_start
        .find(end)
        .unwrap_or_else(|| panic!("missing source marker after {start}: {end}"));
    &after_start[..end_index]
}

fn assert_no_key_checks(surface: &str, source: &str) {
    for forbidden in ["KeyCode::", "KeyModifiers::"] {
        assert!(
            !source.contains(forbidden),
            "{surface} must route keys through Action/KeyMap, found {forbidden}"
        );
    }
}

#[test]
fn keymap_accepts_variant_cycle_overrides() {
    // arrange
    // act
    // assert
    let mut overrides = BTreeMap::new();
    overrides.insert("variant_cycle".to_string(), "tab".to_string());

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(Action::VariantCycle)
    );
}

#[test]
fn keymap_accepts_leader_sequence_overrides() {
    // arrange
    // act
    // assert
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "session_child_first".to_string(),
        "<leader>down".to_string(),
    );

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        Some(Action::SessionChildFirst)
    );
}

#[test]
fn keymap_leaves_control_tab_unbound_while_preserving_focus_and_variant_actions() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(Action::FocusNext)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)),
        Some(Action::VariantCycle)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL)),
        None
    );
}

#[test]
fn keymap_keeps_focus_prev_on_control_shift_tab() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )),
        Some(Action::FocusPrev)
    );
}

#[test]
fn keymap_binds_shift_tab_and_ctrl_t_to_variant_cycle() {
    // arrange
    // act
    // assert
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)),
        Some(Action::VariantCycle)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
        Some(Action::VariantCycle)
    );
    let bindings = keymap.get_binding_strs(Action::VariantCycle);
    assert!(
        bindings.iter().any(|b| b == "Shift+Tab" || b == "Ctrl+t"),
        "variant cycle bindings present: {bindings:?}"
    );
}

#[test]
fn leader_g_opens_lineage_browser() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    // assert
    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
        Some(Action::OpenLineageBrowser)
    );
    assert_eq!(
        Action::from_str("open_lineage_browser"),
        Ok(Action::OpenLineageBrowser)
    );
}

#[test]
fn open_status_dialog_is_canonical_and_toggle_operator_sidebar_aliases() {
    // arrange
    // act
    let canonical = Action::from_str("open_status_dialog");
    let alias = Action::from_str("toggle_operator_sidebar");
    // assert
    assert_eq!(canonical, Ok(Action::OpenStatusDialog));
    assert_eq!(
        alias,
        Ok(Action::OpenStatusDialog),
        "persisted toggle_operator_sidebar must alias open_status_dialog"
    );
    assert_eq!(Action::OpenStatusDialog.as_str(), "open_status_dialog");
    assert_ne!(
        Action::OpenStatusDialog.as_str(),
        "toggle_operator_sidebar",
        "canonical serialization must not emit the compatibility alias"
    );
}

#[test]
fn leader_s_opens_status_dialog_by_default() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    let action = keymap.leader_action(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    // assert
    assert_eq!(action, Some(Action::OpenStatusDialog));
    assert_eq!(keymap.get_binding_str(Action::OpenStatusDialog), "Ctrl+x s");
}

#[test]
fn open_status_dialog_override_remapping_is_preserved() {
    // arrange
    let mut overrides = BTreeMap::new();
    overrides.insert("open_status_dialog".to_string(), "<leader>z".to_string());
    let mut keymap = KeyMap::with_defaults();
    // act
    keymap.apply_overrides(&overrides);
    // assert
    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE)),
        Some(Action::OpenStatusDialog)
    );
}

#[test]
fn toggle_operator_sidebar_override_aliases_to_status_dialog_action() {
    // arrange
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "toggle_operator_sidebar".to_string(),
        "<leader>b".to_string(),
    );
    let mut keymap = KeyMap::with_defaults();
    // act
    keymap.apply_overrides(&overrides);
    // assert
    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
        Some(Action::OpenStatusDialog),
        "old config key must remap onto the status dialog action"
    );
}

#[test]
fn displaced_keybind_remaps_to_open_status_dialog() {
    // arrange — simple-mode defaults + displaced rematerialization
    let keymap = KeyMap::with_defaults();
    let example = include_str!("../../../../configs/tui.example.jsonc");
    let mut overrides = BTreeMap::new();
    overrides.insert("open_status_dialog".to_string(), "f9".to_string());
    let mut remapped = KeyMap::with_defaults();

    // act
    remapped.apply_overrides(&overrides);

    // assert — simple-mode defaults (Ctrl+P palette, leader+s status, leader+m model)
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        Some(Action::Palette),
        "simple-mode: Ctrl+P opens command palette"
    );
    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
        Some(Action::OpenStatusDialog),
        "simple-mode: leader+s opens status dialog"
    );
    assert_eq!(
        keymap.leader_action(&KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)),
        Some(Action::OpenModelSwitcher),
        "simple-mode: leader+m opens model switcher"
    );
    let esc = keymap.get_action(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        matches!(esc, Some(Action::ClearPrompt) | Some(Action::DismissModal)),
        "simple-mode: Esc is clear/dismiss, not cancel: {esc:?}"
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(Action::DismissModal),
        "freeze essentials: Ctrl+C dismisses modal / cancels turn"
    );
    // assert — displaced sidebar toggle rematerializes to status dialog
    assert_eq!(
        Action::from_str("toggle_operator_sidebar"),
        Ok(Action::OpenStatusDialog)
    );
    assert!(
        example.contains("\"open_status_dialog\""),
        "example must document open_status_dialog"
    );
    assert!(
        !example.contains("toggle_operator_sidebar"),
        "canonical example must omit toggle_operator_sidebar"
    );
    // assert — remapping preserved for terminal-normalized override (F9)
    assert_eq!(
        remapped.get_action(&KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE)),
        Some(Action::OpenStatusDialog)
    );
    let palette_bindings = keymap.get_binding_strs(Action::Palette);
    assert!(
        palette_bindings.iter().any(|b| b == "Ctrl+p" || b == "?"),
        "palette bindings present: {palette_bindings:?}"
    );
    assert_eq!(keymap.get_binding_str(Action::OpenStatusDialog), "Ctrl+x s");
    assert_eq!(
        keymap.get_binding_str(Action::OpenModelSwitcher),
        "Ctrl+x m"
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
        Some(Action::OpenStatusDialog)
    );
}

#[test]
fn palette_exposes_lineage_browser_and_child_session_commands() {
    // arrange
    let palette_commands = Action::palette_commands();
    let command_ids: Vec<&str> = palette_commands.iter().map(|cmd| cmd.id).collect();
    // act
    // assert
    assert!(command_ids.contains(&"open_lineage_browser"));
    assert!(command_ids.contains(&"session_child_first"));
    assert!(command_ids.contains(&"session_child_cycle"));
    assert!(command_ids.contains(&"session_child_cycle_reverse"));
    assert!(command_ids.contains(&"session_parent"));
    assert!(command_ids.contains(&"session_background"));

    for command_id in [
        "open_lineage_browser",
        "session_child_first",
        "session_child_cycle",
        "session_child_cycle_reverse",
        "session_parent",
        "session_background",
    ] {
        let label = Action::palette_command_label(command_id);
        let description = Action::palette_command_description(command_id);
        assert!(!label.trim().is_empty(), "{command_id} label");
        assert!(!description.trim().is_empty(), "{command_id} description");
    }
}

#[test]
fn simple_mode_defaults_open_command_palette_on_ctrl_p() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    let action = keymap.get_action(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    // assert
    assert_eq!(action, Some(Action::Palette));
    let bindings = keymap.get_binding_strs(Action::Palette);
    assert!(
        bindings.iter().any(|b| b == "Ctrl+p" || b == "?"),
        "palette bindings present: {bindings:?}"
    );
}

#[test]
fn simple_mode_defaults_open_status_dialog_on_leader_s() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    let action = keymap.leader_action(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    // assert
    assert_eq!(action, Some(Action::OpenStatusDialog));
    assert_eq!(keymap.get_binding_str(Action::OpenStatusDialog), "Ctrl+x s");
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Some(Action::OpenSessionHistory),
        "Ctrl+S opens session history; status remains leader+s / F2 / Ctrl+,"
    );
}

#[test]
fn simple_mode_defaults_open_model_switcher_on_leader_m() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    let action = keymap.leader_action(&KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
    // assert
    assert_eq!(action, Some(Action::OpenModelSwitcher));
    assert_eq!(
        keymap.get_binding_str(Action::OpenModelSwitcher),
        "Ctrl+x m"
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL)),
        None,
        "Ctrl+M is not a direct KeyMap binding; model switcher is leader+m rematerialization"
    );
}

#[test]
fn simple_mode_defaults_map_ctrl_c_to_dismiss_modal_not_interrupt_action() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    let ctrl_c = keymap.get_action(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    let esc = keymap.get_action(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    // assert
    assert_eq!(
        ctrl_c,
        Some(Action::DismissModal),
        "freeze essentials: Ctrl+C dismisses modal / cancels turn"
    );
    assert!(
        matches!(esc, Some(Action::ClearPrompt) | Some(Action::DismissModal)),
        "Esc is clear/dismiss in KeyMap, not a single-shot cancel action: {esc:?}"
    );
    assert!(
        !keymap
            .all_bindings()
            .iter()
            .any(|(_, action)| action.as_str().contains("interrupt")),
        "no interrupt Action exists in default KeyMap bindings"
    );
}

#[test]
fn simple_mode_additional_chords_are_mapped_rematerializations() {
    // arrange
    let keymap = KeyMap::with_defaults();
    // act
    let rematerializations = [
        (Action::OpenStatusDialog, "Ctrl+x s"),
        (Action::OpenModelSwitcher, "Ctrl+x m"),
        (Action::OpenThemeDialog, "Ctrl+x t"),
        (Action::OpenLineageBrowser, "Ctrl+x g"),
        (Action::SessionChildFirst, "Ctrl+x ↓"),
        (Action::SessionBackground, "Ctrl+b"),
    ];
    // assert
    for (action, expected_label) in rematerializations {
        assert_eq!(
            keymap.get_binding_str(action),
            expected_label,
            "{} must keep rematerialized binding {expected_label}",
            action.as_str()
        );
        assert!(
            !keymap.get_bindings(action).is_empty(),
            "{} must remain reachable",
            action.as_str()
        );
    }
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        Some(Action::Palette)
    );
}
