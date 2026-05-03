use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::config::{configured_model_catalog, load_config_from_str};
use harness_tui::app::{AppState, LaunchMetadata, ModelOption, UiIntent};
use ratatui::{backend::TestBackend, Terminal};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn rich_model_config() -> &'static str {
    r#"
    {
      providers: {
        default: {
          type: "openai_compatible",
          base_url: "http://127.0.0.1:8317/v1",
          api_key: "test-key",
          api_mode: "responses",
          timeout_ms: 60000,
          models: {
            "gpt-5.4-mini": {
              display_name: "GPT-5.4 Mini",
              metadata: {
                context_window_tokens: 128000,
              },
              max_input_tokens: 128000,
              max_output_tokens: 16384,
              variants: {
                deterministic: {
                  display_name: "Deterministic",
                  max_output_tokens: 4096,
                  metadata: {
                    description: "Stable low-variance coding",
                    reasoning_effort: "minimal",
                    text_verbosity: "low",
                    recommended_for: "deep debugging",
                  },
                },
                creative: {
                  display_name: "Creative",
                  metadata: {
                    description: "Higher-variance drafting",
                    reasoning_effort: "high",
                    text_verbosity: "high",
                    recommended_for: "novel drafting",
                  },
                },
              },
            },
          },
        },
      },
      agents: {
        deep: {
          description: "Deep work",
          model_ref: "default:gpt-5.4-mini",
          variant: "deterministic",
          tools: ["fs.read"],
        },
        writer: {
          description: "Writer",
          model_ref: "default:gpt-5.4-mini",
          variant: "creative",
          tools: ["fs.read"],
        },
      },
      permissions: {
        defaults: {
          edit: "ask",
          shell: "ask",
          network: "deny",
        },
      },
      runtime: {
        background_tasks: {
          default_concurrency: 2,
          provider_concurrency: 2,
          model_concurrency: 2,
          stale_timeout_ms: 15000,
          message_staleness_timeout_ms: 5000,
        },
        session_dir: ".agent-harness/sessions",
      },
      integrations: {
        remote_search: {
          endpoint: "https://mcp.exa.ai/mcp",
        },
      },
    }
    "#
}

fn available_models() -> Vec<ModelOption> {
    vec![
        ModelOption::from_model_ref("deep", "default:gpt-5.4-mini"),
        ModelOption::from_model_ref("writer", "default:gpt-5.4-mini"),
    ]
}

fn build_plan_models() -> Vec<ModelOption> {
    vec![
        ModelOption::from_model_ref("build", "default:gpt-5.4-mini"),
        ModelOption {
            profile: "plan".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            context_window_tokens: Some(128000),
            max_input_tokens: Some(128000),
            max_output_tokens: Some(4096),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: Some("Writer".to_string()),
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            recommended_for: Some("planning".to_string()),
        },
    ]
}

fn multi_provider_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "build".to_string(),
            provider: "anthropic".to_string(),
            provider_display_label: Some("Anthropic".to_string()),
            provider_backend_label: None,
            model: "claude-sonnet-4-5".to_string(),
            model_display_label: Some("Claude Sonnet 4.5".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("Claude Sonnet 4.5".to_string()),
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
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT-5.4 Mini".to_string()),
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
    ]
}

fn duplicate_build_plan_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            context_window_tokens: Some(128000),
            max_input_tokens: Some(128000),
            max_output_tokens: Some(4096),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: Some("Build".to_string()),
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            recommended_for: Some("build".to_string()),
        },
        ModelOption {
            profile: "plan".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            context_window_tokens: Some(128000),
            max_input_tokens: Some(128000),
            max_output_tokens: Some(4096),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: Some("Plan".to_string()),
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            recommended_for: Some("planning".to_string()),
        },
    ]
}

fn same_profile_variant_options() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            context_window_tokens: Some(128000),
            max_input_tokens: Some(128000),
            max_output_tokens: Some(4096),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: Some("Deep work".to_string()),
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            recommended_for: Some("deep debugging".to_string()),
        },
        ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("creative".to_string()),
            variant_display_label: Some("Creative".to_string()),
            display_label: Some("GPT-5.4 Mini · Creative".to_string()),
            token_window_label: Some("128k ctx · 128k in · 16k out".to_string()),
            context_window_tokens: Some(128000),
            max_input_tokens: Some(128000),
            max_output_tokens: Some(16384),
            description: Some("Higher-variance drafting".to_string()),
            profile_description: Some("Deep work".to_string()),
            reasoning_effort: Some("high".to_string()),
            text_verbosity: Some("high".to_string()),
            recommended_for: Some("novel drafting".to_string()),
        },
    ]
}

fn reasoning_order_variant_options() -> Vec<ModelOption> {
    [
        ("medium", "Medium", "medium"),
        ("high", "High", "high"),
        ("xhigh", "XHigh", "xhigh"),
    ]
    .into_iter()
    .map(|(variant, label, reasoning_effort)| ModelOption {
        profile: "deep".to_string(),
        provider: "default".to_string(),
        provider_display_label: Some("default".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "gpt-5.4-mini".to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: Some(variant.to_string()),
        variant_display_label: Some(label.to_string()),
        display_label: Some(format!("GPT-5.4 Mini · {label}")),
        token_window_label: Some("128k ctx · 128k in · 16k out".to_string()),
        context_window_tokens: Some(128000),
        max_input_tokens: Some(128000),
        max_output_tokens: Some(16384),
        description: None,
        profile_description: Some("Deep work".to_string()),
        reasoning_effort: Some(reasoning_effort.to_string()),
        text_verbosity: Some("medium".to_string()),
        recommended_for: None,
    })
    .collect()
}

fn config_backed_profile_model_options(profile: &str) -> Vec<ModelOption> {
    let config = load_config_from_str(rich_model_config()).expect("config should parse");
    configured_model_catalog(&config)
        .into_iter()
        .map(|entry| ModelOption {
            profile: profile.to_string(),
            provider: entry.provider,
            provider_display_label: Some(entry.provider_display_label),
            provider_backend_label: entry.provider_backend_label,
            model: entry.model,
            model_display_label: Some(entry.model_display_label),
            variant: entry.variant,
            variant_display_label: entry.variant_display_label,
            display_label: Some(entry.display_label),
            token_window_label: entry.token_window_label,
            context_window_tokens: entry.context_window_tokens,
            max_input_tokens: entry.max_input_tokens,
            max_output_tokens: entry.max_output_tokens,
            description: entry.description,
            profile_description: None,
            reasoning_effort: entry.reasoning_effort,
            text_verbosity: entry.text_verbosity,
            recommended_for: entry.recommended_for,
        })
        .collect()
}

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
fn model_switcher_renders_opencode_select_dialog_contract() {
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

#[test]
fn ctrl_t_cycles_reasoning_variants_in_semantic_order() {
    let variants = reasoning_order_variant_options();
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&variants[0]).with_available_models(variants),
    );

    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Medium");

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "deep");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · High");
    assert_eq!(live.current_model_reasoning_label(), Some("high"));

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · XHigh");
    assert_eq!(live.current_model_reasoning_label(), Some("xhigh"));
}

#[test]
fn launch_mode_label_is_not_used_as_model_reasoning_fallback() {
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1]).with_mode_label("Live"),
    );

    assert_eq!(live.current_model_label(), "GPT-5.4 Mini");
    assert_eq!(live.current_model_reasoning_label(), None);
    assert_eq!(live.launch_mode_label(), Some("Live"));
}

#[test]
fn model_switcher_deduplicates_agent_rows_and_preserves_current_agent() {
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
            .with_available_models(duplicate_build_plan_models()),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.model_switcher_visible);
    assert_eq!(app.model_options.len(), 1);
    assert_eq!(app.model_options[0].profile, "build");

    for ch in "plan".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert!(app.model_filtered.is_empty());

    for _ in 0..4 {
        app.handle_key(key(KeyCode::Backspace));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.active_profile(), "build");
    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel { profile, .. } =
        intents.last().expect("switch intent should be emitted")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "build");
}

#[test]
fn variant_cycle_updates_selected_model_without_losing_launch_metadata() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let available_models = same_profile_variant_options();
    let variant_cycle_overrides =
        BTreeMap::from([("variant_cycle".to_string(), "tab".to_string())]);

    let mut live = AppState::new_live(None, false, Some(sink));
    live.apply_keybindings(variant_cycle_overrides.clone());
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models.clone())
            .with_mode_label("Demo"),
    );

    live.handle_key(key(KeyCode::Tab));

    assert_eq!(live.active_profile(), "deep");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(live.launch_mode_label(), Some("Demo"));

    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents
        .last()
        .expect("switch model intent should be emitted")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "deep");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(launch_metadata.mode_label(), Some("Demo"));
    assert_eq!(launch_metadata.available_models().len(), 2);

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-variant-cycle"), Vec::new());
    replay.apply_keybindings(variant_cycle_overrides);
    replay.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models)
            .with_mode_label("Demo"),
    );

    replay.handle_key(key(KeyCode::Tab));

    assert_eq!(replay.active_profile(), "deep");
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
    assert_eq!(replay.launch_mode_label(), Some("Demo"));
}

#[test]
fn ctrl_t_cycles_thinking_variant_within_current_profile() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut live = AppState::new_live(None, false, Some(sink));
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(same_profile_variant_options())
            .with_mode_label("Demo"),
    );

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "deep");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(live.launch_mode_label(), Some("Demo"));

    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents
        .last()
        .expect("switch model intent should be emitted")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "deep");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(launch_metadata.reasoning_effort(), Some("high"));
}

#[test]
fn ctrl_t_includes_base_model_entries_in_config_backed_variant_cycle() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let available_models = config_backed_profile_model_options("deep");
    assert!(available_models
        .iter()
        .any(|option| option.variant().is_none()));

    let mut live = AppState::new_live(None, false, Some(sink));
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models)
            .with_mode_label("Demo"),
    );

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "deep");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(
        live.runtime_context_summary_segment_text(),
        Some("Next turns: deep · GPT-5.4 Mini · Creative".to_string())
    );

    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents
        .last()
        .expect("switch model intent should be emitted")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "deep");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(launch_metadata.reasoning_effort(), Some("high"));
}

#[test]
fn ctrl_t_cycles_from_last_variant_to_none() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let available_models = same_profile_variant_options();
    let mut live = AppState::new_live(None, false, Some(sink));
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&available_models[1])
            .with_available_models(available_models)
            .with_mode_label("Demo"),
    );

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "deep");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini");
    assert_eq!(live.launch_mode_label(), Some("Demo"));

    let intents = intents.lock().expect("lock intents");
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents
        .last()
        .expect("switch model intent should be emitted")
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "deep");
    assert_eq!(launch_metadata.variant(), None);
    assert_eq!(launch_metadata.reasoning_effort(), None);
    assert_eq!(launch_metadata.mode_label(), Some("Demo"));
}

#[test]
fn runtime_context_labels_distinguish_live_continue_and_replay() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let launch_metadata = LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
        .with_available_models(available_models());

    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(launch_metadata.clone());
    assert_eq!(
        startup.runtime_context_primary_summary(),
        "Launch: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(startup.runtime_context_summary_segment_text(), None);
    assert_eq!(
        startup.runtime_context_provider_display(),
        Some("default".to_string())
    );

    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(launch_metadata.clone());
    assert_eq!(
        live.runtime_context_primary_summary(),
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(live.runtime_context_summary_segment_text(), None);

    let mut continued = AppState::new_live(None, false, None);
    continued.set_launch_metadata(launch_metadata.clone().with_mode_label("Continued"));
    assert_eq!(
        continued.runtime_context_primary_summary(),
        "Continued runtime: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(continued.runtime_context_summary_segment_text(), None);

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-runtime-context"), Vec::new());
    replay.set_launch_metadata(launch_metadata);
    assert_eq!(
        replay.runtime_context_primary_summary(),
        "Recorded runtime · read-only: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay.runtime_context_summary_segment_text(), None);
    assert_eq!(
        replay.runtime_context_provider_display(),
        Some("default".to_string())
    );
}

#[test]
fn live_switch_model_labels_next_turn_only() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let variant_cycle_overrides =
        BTreeMap::from([("variant_cycle".to_string(), "tab".to_string())]);

    let mut live = AppState::new_live(None, false, None);
    live.apply_keybindings(variant_cycle_overrides.clone());
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(same_profile_variant_options()),
    );

    live.handle_key(key(KeyCode::Tab));

    assert_eq!(
        live.runtime_context_primary_summary(),
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(
        live.runtime_context_summary_segment_text(),
        Some("Next turns: deep · GPT-5.4 Mini · Creative".to_string())
    );

    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/replay-runtime-context-switch"),
        Vec::new(),
    );
    replay.apply_keybindings(variant_cycle_overrides);
    replay.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(same_profile_variant_options()),
    );

    replay.handle_key(key(KeyCode::Tab));

    assert_eq!(
        replay.runtime_context_primary_summary(),
        "Recorded runtime · read-only: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay.runtime_context_summary_segment_text(), None);
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
}
