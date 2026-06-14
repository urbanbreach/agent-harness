use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, LaunchMetadata, ModelOption, UiIntent};
use ratatui::{backend::TestBackend, Terminal};

use crate::model_switcher_fixtures::*;

fn ctrl(code: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)
}

fn render(app: &AppState) -> String {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, app))
        .expect("draw frame");
    format!("{:?}", terminal.backend().buffer())
}

#[allow(clippy::type_complexity)]
fn intent_sink() -> (
    Arc<Mutex<Vec<UiIntent>>>,
    Arc<dyn Fn(UiIntent) + Send + Sync>,
) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    (intents, sink)
}

#[test]
fn ctrl_a_provider_jump_opens_provider_list_and_switches_provider() {
    let (intents, sink) = intent_sink();
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(ctrl('a'));

    let provider_dialog = render(&app);
    assert!(
        provider_dialog.contains("Select provider"),
        "{provider_dialog}"
    );
    assert!(provider_dialog.contains("Providers"), "{provider_dialog}");
    assert!(provider_dialog.contains("Anthropic"), "{provider_dialog}");

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.launch_metadata().provider(), "anthropic");
    assert_eq!(app.launch_metadata().model(), Some("claude-sonnet-4-5"));
    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents.last().expect("switch intent")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "build");
    assert_eq!(launch_metadata.provider(), "anthropic");
}

#[test]
fn variant_dialog_selects_default_and_named_variants() {
    let (intents, sink) = intent_sink();
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&same_profile_variant_options()[0])
            .with_available_models(same_profile_variant_options()),
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

    let variant_dialog = render(&app);
    assert!(
        variant_dialog.contains("Select variant"),
        "{variant_dialog}"
    );
    assert!(variant_dialog.contains("Default"), "{variant_dialog}");
    assert!(variant_dialog.contains("Creative"), "{variant_dialog}");
    assert!(variant_dialog.contains("●"), "{variant_dialog}");

    app.handle_key(key(KeyCode::Home));
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.launch_metadata().variant(), None);
    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        launch_metadata, ..
    } = intents.last().expect("switch intent")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(launch_metadata.variant(), None);
}

#[test]
fn leader_a_agent_list_uses_shared_dialog_and_switches_agent() {
    let (intents, sink) = intent_sink();
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(build_plan_models())
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('a')));

    let agent_dialog = render(&app);
    assert!(agent_dialog.contains("Select agent"), "{agent_dialog}");
    assert!(agent_dialog.contains("Agents"), "{agent_dialog}");
    assert!(agent_dialog.contains("Build"), "{agent_dialog}");
    assert!(agent_dialog.contains("Plan"), "{agent_dialog}");

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.active_profile(), "plan");
    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel { profile, .. } = intents.last().expect("switch intent") else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "plan");
}

#[test]
fn model_search_ranks_prefix_and_word_boundary_above_scattered_subsequence() {
    let mut app = AppState::new_live(None, false, None);
    let models = vec![
        ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("OpenAI".to_string()),
            provider_backend_label: None,
            model: "helper-g-p".to_string(),
            model_display_label: Some("Helper Graph Path".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("Helper Graph Path".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: Some("Build".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
        ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("OpenAI".to_string()),
            provider_backend_label: None,
            model: "gpt-fast".to_string(),
            model_display_label: Some("GPT Fast".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT Fast".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: Some("Build".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
    ];
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&models[0]).with_available_models(models),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    for ch in "gp".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(
        app.model_options[app.model_filtered[0]].model, "gpt-fast",
        "prefix match should outrank scattered subsequence"
    );
}

#[test]
fn provider_variant_and_agent_dialogs_render_empty_states() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::new("build", "local", None));

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(ctrl('a'));
    assert!(render(&app).contains("No providers found"));

    app.handle_key(key(KeyCode::Esc));
    app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert!(render(&app).contains("No variants found"));

    app.handle_key(key(KeyCode::Esc));
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('a')));
    assert!(render(&app).contains("No agents found"));
}
