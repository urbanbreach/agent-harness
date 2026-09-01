use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::keybindings::{palette_model, Action, KeyMap};

#[test]
fn cancel_and_replace_prompt_differs_from_interject_prompt() {
    assert_ne!(Action::CancelAndReplacePrompt, Action::InterjectPrompt);
}

#[test]
fn default_composer_keymap_preserves_newline_and_routes_distinct_actions() {
    // Given: the default public keymap.
    let keymap = KeyMap::with_defaults();

    // When: the composer shortcut keys are resolved.
    let newline = keymap.get_action(&KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    let interject = keymap.get_action(&KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    let cancel_replace = keymap.get_action(&KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    let toggle_multiline = keymap.get_action(&KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT));
    let terminal_safe_send =
        keymap.get_action(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
    let terminal_safe_interject =
        keymap.get_action(&KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    let terminal_safe_cancel_replace =
        keymap.get_action(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));

    // Then: the shipped newline key remains intact and every new action is distinct.
    assert_eq!(newline, Some(Action::InsertNewline));
    assert_eq!(interject, Some(Action::InterjectPrompt));
    assert_eq!(cancel_replace, Some(Action::CancelAndReplacePrompt));
    assert_eq!(toggle_multiline, Some(Action::ToggleMultiline));
    assert_eq!(terminal_safe_send, Some(Action::SubmitPrompt));
    assert_eq!(terminal_safe_interject, Some(Action::InterjectPrompt));
    assert_eq!(
        terminal_safe_cancel_replace,
        Some(Action::CancelAndReplacePrompt)
    );
}

#[test]
fn model_multiline_palette_command_dispatches_toggle_action() {
    // Given: the public model.multiline palette entry.
    let command = palette_model::find("model.multiline").unwrap_or_else(|| {
        panic!("missing model.multiline palette command");
    });

    // When: its public dispatch target is inspected.
    // Then: the palette routes to the multiline toggle action.
    assert_eq!(
        command.dispatch,
        palette_model::PaletteDispatch::Action(Action::ToggleMultiline)
    );
}
