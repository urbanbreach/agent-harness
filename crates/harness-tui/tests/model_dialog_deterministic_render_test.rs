use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

#[path = "support/model_switcher_fixtures.rs"]
#[allow(dead_code)]
mod model_switcher_fixtures;

use model_switcher_fixtures::*;

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn ctrl(code: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)
}

#[test]
fn model_dialog_renders_favorite_group_without_pty() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );
    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(ctrl('f'));

    let rendered = render_text(&app, 100, 30);

    insta::assert_snapshot!(rendered.as_str());
    assert!(rendered.contains("Select model"));
    assert!(rendered.contains("Favorites"));
    assert!(rendered.contains("★"));
    assert!(rendered.contains("GPT-5.4 Mini"));
}

#[test]
fn variant_dialog_renders_default_and_named_variants_without_pty() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&same_profile_variant_options()[0])
            .with_available_models(same_profile_variant_options()),
    );

    app.handle_key(ctrl('v'));

    let rendered = render_text(&app, 100, 30);

    insta::assert_snapshot!(rendered.as_str());
    assert!(rendered.contains("Select variant"));
    assert!(rendered.contains("Default"));
    assert!(rendered.contains("Creative"));
    assert!(rendered.contains("●"));
}

#[test]
fn agent_dialog_renders_shared_select_surface_without_pty() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(build_plan_models())
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('a')));

    let rendered = render_text(&app, 100, 30);

    insta::assert_snapshot!(rendered.as_str());
    assert!(rendered.contains("Select agent"));
    assert!(rendered.contains("Agents"));
    assert!(rendered.contains("Build"));
    assert!(rendered.contains("Plan"));
}
