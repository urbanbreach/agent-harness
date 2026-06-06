use super::*;

#[test]
fn action_from_str_roundtrip() {
    let action = Action::Quit;
    let s = action.as_str();
    let parsed = Action::from_str(s).unwrap();
    assert_eq!(action, parsed);
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
        ("model", "switch_model"),
        ("toggles", "toggles"),
        ("events", "open_event_log"),
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
    let binding = KeyBinding::from_str("ctrl+p").unwrap();
    assert_eq!(binding.code, KeyCode::Char('p'));
    assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
}

#[test]
fn key_binding_parses_single_char() {
    let binding = KeyBinding::from_str("q").unwrap();
    assert_eq!(binding.code, KeyCode::Char('q'));
    assert_eq!(binding.modifiers, KeyModifiers::NONE);
}

#[test]
fn keymap_finds_default_binding() {
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    assert_eq!(keymap.get_action(&event), Some(Action::Quit));
}

#[test]
fn keymap_override_replaces_default_binding() {
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
fn keymap_returns_binding_str() {
    let keymap = KeyMap::with_defaults();
    let binding = keymap.get_binding_str(Action::Quit);
    assert_eq!(binding, "q");
}

#[test]
fn keymap_returns_ctrl_binding_str() {
    let keymap = KeyMap::with_defaults();
    let binding = keymap.get_binding_str(Action::Palette);
    assert_eq!(binding, "Ctrl+p");
}

#[test]
fn keymap_formats_binding_labels_from_overrides() {
    let mut overrides = BTreeMap::new();
    overrides.insert("quit".to_string(), "x".to_string());

    let mut keymap = KeyMap::with_defaults();
    keymap.apply_overrides(&overrides);

    assert_eq!(keymap.get_binding_label(Action::Quit, "quit"), "x quit");
}

#[test]
fn keymap_binds_shift_enter_to_insert_newline() {
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_binds_ctrl_j_to_insert_newline() {
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_binds_ctrl_enter_to_insert_newline() {
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_binds_alt_enter_to_insert_newline() {
    let keymap = KeyMap::with_defaults();
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
}

#[test]
fn keymap_uses_ctrl_y_and_ctrl_n_for_permission_decisions() {
    let keymap = KeyMap::with_defaults();

    let allow = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
    let deny = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);

    assert_eq!(keymap.get_action(&allow), Some(Action::AllowPermission));
    assert_eq!(keymap.get_action(&deny), Some(Action::DenyPermission));
    assert_eq!(keymap.get_binding_str(Action::AllowPermission), "Ctrl+y");
    assert_eq!(keymap.get_binding_str(Action::DenyPermission), "Ctrl+n");
}

#[test]
fn keymap_binds_child_session_navigation_to_default_bindings() {
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)),
        Some(Action::SessionChildFirst)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('\u{1d}'), KeyModifiers::NONE)),
        Some(Action::SessionChildFirst)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)),
        Some(Action::SessionChildCycle)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)),
        Some(Action::SessionChildCycleReverse)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL)),
        Some(Action::SessionParent)
    );
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
            "            Action::AgentCycle =>",
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
fn keymap_binds_tab_to_agent_cycle_by_default() {
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(Action::AgentCycle)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)),
        Some(Action::AgentCycleReverse)
    );
    assert_eq!(keymap.get_binding_str(Action::AgentCycle), "Tab");
    assert_eq!(
        keymap.get_binding_str(Action::AgentCycleReverse),
        "Shift-Tab"
    );
}

#[test]
fn keymap_keeps_focus_cycle_on_control_tab() {
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL)),
        Some(Action::FocusNext)
    );
    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::CONTROL)),
        Some(Action::FocusPrev)
    );
}

#[test]
fn keymap_binds_ctrl_t_to_variant_cycle() {
    let keymap = KeyMap::with_defaults();

    assert_eq!(
        keymap.get_action(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
        Some(Action::VariantCycle)
    );
    assert_eq!(keymap.get_binding_str(Action::VariantCycle), "Ctrl+t");
}
