use super::*;
use std::collections::BTreeMap;

fn leader_key() -> crossterm::event::KeyEvent {
    key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    )
}

fn char_key(ch: char) -> crossterm::event::KeyEvent {
    key(crossterm::event::KeyCode::Char(ch))
}

fn app_with_switchable_model() -> app::AppState {
    let model = app::ModelOption::from_model_ref("build", "default:gpt-5.4-mini");
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_option(&model).with_available_models(vec![model]),
    );
    app.focus = app::Focus::Prompt;
    app
}

pub(super) fn leader_m_opens_model_switcher_without_editing_prompt() {
    let mut app = app_with_switchable_model();

    app.handle_key(leader_key());
    assert!(app.leader_key_pending_for_test());
    app.handle_key(char_key('m'));

    assert!(app.overlay_state.model_switcher_visible);
    assert_eq!(app.composer.prompt_buffer, "");
    assert!(!app.leader_key_pending_for_test());
}

pub(super) fn leader_unbound_key_cancels_without_prompt_side_effect() {
    let mut app = app_with_switchable_model();
    app.handle_key(leader_key());

    app.handle_key(char_key('z'));

    assert_eq!(app.composer.prompt_buffer, "");
    assert!(!app.leader_key_pending_for_test());

    app.handle_key(char_key('a'));
    assert_eq!(app.composer.prompt_buffer, "a");
}

pub(super) fn leader_escape_and_timeout_cancel_sequence() {
    let mut escaped = app_with_switchable_model();
    escaped.handle_key(leader_key());

    escaped.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!escaped.leader_key_pending_for_test());
    assert_eq!(escaped.composer.prompt_buffer, "");
    assert!(!escaped.overlay_state.model_switcher_visible);

    let mut timed_out = app_with_switchable_model();
    timed_out.handle_key(leader_key());
    timed_out.force_leader_key_timeout_for_test();

    timed_out.handle_key(char_key('z'));

    assert_eq!(timed_out.composer.prompt_buffer, "z");
    assert!(!timed_out.leader_key_pending_for_test());
}

pub(super) fn replay_leader_model_switcher_is_absent() {
    let model = app::ModelOption::from_model_ref("build", "default:gpt-5.4-mini");
    let mut replay =
        app::AppState::new_replay(std::path::PathBuf::from("/tmp/leader-replay"), Vec::new());
    replay.set_launch_metadata(
        app::LaunchMetadata::from_model_option(&model).with_available_models(vec![model]),
    );

    replay.handle_key(leader_key());
    replay.handle_key(char_key('m'));

    assert!(!replay.overlay_state.model_switcher_visible);
    assert_eq!(replay.composer.prompt_buffer, "");
    assert!(!replay.leader_key_pending_for_test());
}

pub(super) fn palette_shortcuts_follow_runtime_leader_bindings() {
    let mut app = app_with_switchable_model();
    app.apply_keybindings(BTreeMap::from([
        ("leader".to_string(), "ctrl+g".to_string()),
        ("switch_model".to_string(), "<leader>m".to_string()),
    ]));

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let rendered = render_live_lines(&app, 120, 36);
    let switch_model_row = rendered
        .lines()
        .find(|line| line.contains("Switch model"))
        .expect("switch model row renders");

    assert!(rendered.contains("Switch model"));
    assert!(switch_model_row.contains("ctrl+g m"));
    assert!(!switch_model_row.trim_end().ends_with(" model"));
}
