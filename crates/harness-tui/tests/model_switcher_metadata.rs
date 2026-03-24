use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::config::load_config_from_str;
use harness_tui::app::{AppState, LaunchMetadata, ModelOption, UiIntent};

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
      profiles: {
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

#[test]
fn model_switcher_surfaces_variant_metadata_and_limits() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let available_models = vec![
        ModelOption::from_model_ref("deep", "default:gpt-5.4-mini"),
        ModelOption::from_model_ref("writer", "default:gpt-5.4-mini"),
    ];

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
    assert_eq!(app.model_options.len(), 2);

    let deterministic = app
        .model_options
        .iter()
        .find(|option| option.profile == "deep")
        .expect("deep option should exist");
    assert_eq!(deterministic.variant(), Some("deterministic"));
    assert_eq!(
        deterministic.display_label(),
        Some("GPT-5.4 Mini · Deterministic")
    );
    assert_eq!(
        deterministic.token_window_label(),
        Some("128k ctx · 128k in · 4k out")
    );
    assert_eq!(deterministic.reasoning_effort(), Some("minimal"));
    assert_eq!(deterministic.text_verbosity(), Some("low"));
    assert_eq!(deterministic.recommended_for(), Some("deep debugging"));

    for ch in "novel".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert_eq!(app.model_filtered.len(), 1);
    let selected = &app.model_options[app.model_filtered[0]];
    assert_eq!(selected.profile, "writer");
    assert_eq!(selected.variant(), Some("creative"));
    assert_eq!(
        selected.token_window_label(),
        Some("128k ctx · 128k in · 16k out")
    );

    app.handle_key(key(KeyCode::Enter));

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
    assert_eq!(profile, "writer");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(
        launch_metadata.display_label(),
        Some("GPT-5.4 Mini · Creative")
    );
    assert_eq!(
        launch_metadata.token_window_label(),
        Some("128k ctx · 128k in · 16k out")
    );

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-models"), Vec::new());
    replay.set_launch_metadata(
        LaunchMetadata::from_model_ref("deep", "default:gpt-5.4-mini")
            .with_available_models(available_models),
    );
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
}

#[test]
fn model_identity_rows_use_gpt_5_4_mini_defaults() {
    let _config = load_config_from_str(rich_model_config()).expect("config should parse");

    let available_models = vec![
        ModelOption::from_model_ref("deep", "default:gpt-5.4-mini"),
        ModelOption::from_model_ref("writer", "default:gpt-5.4-mini"),
    ];

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

    assert_eq!(app.model_options.len(), 2);

    let deterministic = app
        .model_options
        .iter()
        .find(|option| option.profile == "deep")
        .expect("deep option should exist");
    assert_eq!(
        deterministic.display_label(),
        Some("GPT-5.4 Mini · Deterministic")
    );

    let creative = app
        .model_options
        .iter()
        .find(|option| option.profile == "writer")
        .expect("writer option should exist");
    assert_eq!(creative.display_label(), Some("GPT-5.4 Mini · Creative"));
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

    let available_models = vec![
        ModelOption::from_model_ref("deep", "default:gpt-5.4-mini"),
        ModelOption::from_model_ref("writer", "default:gpt-5.4-mini"),
    ];
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

    assert_eq!(live.active_profile(), "writer");
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
    assert_eq!(profile, "writer");
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

    assert_eq!(replay.active_profile(), "writer");
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(replay.launch_mode_label(), Some("Demo"));
}
