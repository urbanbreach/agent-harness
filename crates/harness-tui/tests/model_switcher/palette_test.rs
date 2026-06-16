use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::KeyCode;
use harness_core::config::load_config_from_str;
use harness_tui::app::{AppState, LaunchMetadata, UiIntent};
use ratatui::{backend::TestBackend, Terminal};

use crate::model_switcher_fixtures::*;

#[test]
fn model_switcher_ui_opens_from_slash_command() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let available_models = available_models();

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models.clone()),
    );

    assert_eq!(app.current_model_label(), "GPT-5.4 Mini · Deterministic");

    // act
    // act
    // act
    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.model_switcher_visible);
    assert_eq!(app.model_options.len(), 1);
    assert_eq!(app.model_options[0].variant(), None);
    assert!(intents.lock().expect("lock intents").is_empty());

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-models"), Vec::new());
    replay.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models),
    );
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
}

#[test]
fn model_switcher_populates_options_from_launch_metadata() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let available_models = available_models();

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models),
    );

    assert_eq!(app.current_model_label(), "GPT-5.4 Mini · Deterministic");

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.model_switcher_visible);
    assert_eq!(app.model_options.len(), 1);
    assert_eq!(app.model_filtered.len(), 1);
    assert_eq!(app.model_options[0].display_label(), Some("GPT-5.4 Mini"));
    assert_eq!(app.model_options[0].variant(), None);
}

#[test]
fn model_switcher_shows_base_models_without_variant_rows() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(same_profile_variant_options()),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.model_switcher_visible);
    assert_eq!(app.model_options.len(), 1);
    assert_eq!(app.model_options[0].display_label(), Some("GPT-5.4 Mini"));
    assert_eq!(app.model_options[0].variant(), None);

    for ch in "creative".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert!(app.model_filtered.is_empty());
}

#[test]
fn model_switcher_renders_harness_select_dialog_contract() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );
    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &app))
        .expect("draw frame");
    let rendered = format!("{:?}", terminal.backend().buffer());

    assert!(rendered.contains("Select model"), "{rendered}");
    assert!(rendered.contains("esc"), "{rendered}");
    assert!(rendered.contains("Search"), "{rendered}");
    assert!(rendered.contains("Anthropic"), "{rendered}");
    assert!(rendered.contains("OpenAI"), "{rendered}");
    assert!(rendered.contains("●"), "{rendered}");
    assert!(rendered.contains("GPT-5.4 Mini"), "{rendered}");
    assert!(!rendered.contains("Switch model ·"), "{rendered}");
    assert!(!rendered.contains("Filter models, providers"), "{rendered}");
}

#[test]
fn model_switcher_filter_flattens_to_title_and_provider_matches() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.model_filtered.len(), 2);
    assert_eq!(
        app.model_options[app.model_filtered[app.model_selected]].model,
        "gpt-5.4-mini"
    );

    for ch in "anth".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.model_filtered.len(), 1);
    assert_eq!(app.model_selected, 0);
    assert_eq!(
        app.model_options[app.model_filtered[0]].model,
        "claude-sonnet-4-5"
    );

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &app))
        .expect("draw frame");
    let rendered = format!("{:?}", terminal.backend().buffer());

    assert!(rendered.contains("Claude Sonnet 4.5"), "{rendered}");
    assert!(rendered.contains("Anthropic"), "{rendered}");
}

#[test]
fn model_switcher_renders_fallback_error_status() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );

    // act
    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &app))
        .expect("draw frame");
    let rendered = format!("{:?}", terminal.backend().buffer());

    // assert
    assert!(
        rendered.contains("No automatic model fallback"),
        "{rendered}"
    );
    assert!(
        rendered.contains("provider errors stay visible"),
        "{rendered}"
    );
}

#[test]
fn model_switcher_enter_emits_switch_intent_for_selected_model() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(build_plan_models())
            .with_mode_label("Continued"),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    assert!(!app.model_switcher_visible);
    assert_eq!(app.active_profile(), "build");
    assert_eq!(app.launch_mode_label(), Some("Continued"));
    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents.last().expect("switch intent should be emitted")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "build");
    assert_eq!(launch_metadata.variant(), None);
}

fn authenticated_builtin_models() -> Vec<harness_tui::app::ModelOption> {
    vec![
        harness_tui::app::ModelOption {
            profile: "build".to_string(),
            provider: "openai-codex".to_string(),
            provider_display_label: Some("OpenAI Codex".to_string()),
            provider_backend_label: Some("OpenAI-compatible".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT 5.4 Mini".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT 5.4 Mini".to_string()),
            token_window_label: Some("272k ctx · 272k in · 128k out".to_string()),
            context_window_tokens: Some(272000),
            max_input_tokens: Some(272000),
            max_output_tokens: Some(128000),
            description: None,
            profile_description: Some("Build".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
        harness_tui::app::ModelOption {
            profile: "build".to_string(),
            provider: "github-copilot".to_string(),
            provider_display_label: Some("GitHub Copilot".to_string()),
            provider_backend_label: Some("OpenAI-compatible".to_string()),
            model: "gpt-5.5".to_string(),
            model_display_label: Some("GPT 5.5".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT 5.5".to_string()),
            token_window_label: Some("272k ctx · 272k in · 128k out".to_string()),
            context_window_tokens: Some(272000),
            max_input_tokens: Some(272000),
            max_output_tokens: Some(128000),
            description: Some("offline fallback".to_string()),
            profile_description: Some("Build".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
    ]
}

#[test]
fn model_switcher_opens_no_provider_connect_state() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::new("build", "local", None));

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.model_switcher_visible);
    assert!(app.model_options.is_empty());

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &app))
        .expect("draw frame");
    let rendered = format!("{:?}", terminal.backend().buffer());
    assert!(rendered.contains("Connect a provider"), "{rendered}");
}

#[test]
fn model_switcher_groups_authenticated_builtin_provider_rows() {
    let mut app = AppState::new_live(None, false, None);
    let models = authenticated_builtin_models();
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&models[0]).with_available_models(models),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &app))
        .expect("draw frame");
    let rendered = format!("{:?}", terminal.backend().buffer());

    assert!(rendered.contains("OpenAI Codex"), "{rendered}");
    assert!(rendered.contains("GitHub Copilot"), "{rendered}");
    assert!(rendered.contains("GPT 5.4 Mini"), "{rendered}");
}

#[test]
fn model_switcher_search_matches_authenticated_provider_label_and_switches() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    let models = authenticated_builtin_models();
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&models[0]).with_available_models(models),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    for ch in "copilot".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.model_filtered.len(), 1);
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.launch_metadata().provider(), "github-copilot");
    assert_eq!(app.launch_metadata().model(), Some("gpt-5.5"));
    assert_eq!(
        app.current_source_label().as_deref(),
        Some("GitHub Copilot (OpenAI-compatible)")
    );
    assert_eq!(app.current_model_label(), "GPT 5.5");
    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        launch_metadata, ..
    } = intents.last().expect("switch intent")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(launch_metadata.provider(), "github-copilot");
    assert_eq!(launch_metadata.model(), Some("gpt-5.5"));
}

#[test]
fn no_provider_prompt_submission_blocks_with_connect_guidance() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(LaunchMetadata::new("build", "local", None));

    for ch in "hello".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(intents.lock().expect("lock intents").is_empty());
    assert_eq!(
        app.status_banner.as_deref(),
        Some("Connect a provider to send prompts")
    );
}

#[test]
fn auth_catalog_refresh_opens_model_picker_with_connected_models() {
    let mut app = AppState::new_live(None, false, None);
    let models = authenticated_builtin_models();
    app.set_launch_metadata(LaunchMetadata::new("build", "local", None));

    app.apply_auth_provider_catalog_refresh(
        LaunchMetadata::from_model_option(&models[0]).with_available_models(models),
    );

    assert!(app.model_switcher_visible);
    assert_eq!(
        app.status_banner.as_deref(),
        Some("Provider connected; choose a model")
    );
    assert_eq!(app.launch_metadata().provider(), "openai-codex");
    assert!(!app.model_options.is_empty());
}
