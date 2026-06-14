use super::*;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn keymap_binds_harness_leader_defaults() {
    let keymap = KeyMap::with_defaults();
    let leader = key(KeyCode::Char('x'), KeyModifiers::CONTROL);

    assert!(keymap.is_leader(&leader));
    assert_eq!(
        keymap.get_leader_action(&key(KeyCode::Char('m'), KeyModifiers::NONE)),
        Some(Action::SwitchModel)
    );
    assert_eq!(
        keymap.get_leader_action(&key(KeyCode::Char('b'), KeyModifiers::NONE)),
        Some(Action::ToggleOperatorSidebar)
    );
    assert_eq!(
        keymap.get_leader_action(&key(KeyCode::Char('s'), KeyModifiers::NONE)),
        Some(Action::OpenStatusDialog)
    );
    assert_eq!(keymap.get_binding_str(Action::SwitchModel), "Ctrl+x m");
    assert_eq!(keymap.palette_command_shortcut("switch_model"), "ctrl+x m");
}

#[test]
fn keymap_supports_rebound_leader_and_multiple_bindings() {
    let mut overrides = BTreeMap::new();
    overrides.insert("leader".to_string(), "ctrl+g".to_string());
    overrides.insert("switch_model".to_string(), "<leader>m, ctrl+m".to_string());
    let mut keymap = KeyMap::with_defaults();

    keymap.try_apply_overrides(&overrides).unwrap();

    assert!(keymap.is_leader(&key(KeyCode::Char('g'), KeyModifiers::CONTROL)));
    assert_eq!(
        keymap.get_leader_action(&key(KeyCode::Char('m'), KeyModifiers::NONE)),
        Some(Action::SwitchModel)
    );
    assert_eq!(
        keymap.get_action(&key(KeyCode::Char('m'), KeyModifiers::CONTROL)),
        Some(Action::SwitchModel)
    );
    assert_eq!(
        keymap.get_binding_strs(Action::SwitchModel),
        vec!["Ctrl+g m".to_string(), "Ctrl+m".to_string()]
    );
}

#[test]
fn keymap_rejects_invalid_leader_override_with_context() {
    let mut overrides = BTreeMap::new();
    overrides.insert("switch_model".to_string(), "<leader>".to_string());
    let mut keymap = KeyMap::with_defaults();

    let error = keymap.try_apply_overrides(&overrides).unwrap_err();

    assert!(error.contains("switch_model"));
    assert!(error.contains("<leader>"));
    assert!(error.contains("missing key"));
}
