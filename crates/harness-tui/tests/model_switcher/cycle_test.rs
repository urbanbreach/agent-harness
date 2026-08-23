use harness_tui::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::config::load_config_from_str;
use harness_tui::app::{AppState, LaunchMetadata, UiIntent};

use crate::model_switcher_fixtures::*;

#[test]
fn ctrl_t_cycles_reasoning_variants_in_semantic_order() {
    // arrange
    // act
    // assert
    let variants = reasoning_order_variant_options();
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&variants[0]).with_available_models(variants),
    );

    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Medium");

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "default");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · High");
    assert_eq!(live.current_model_reasoning_label(), Some("high"));

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · XHigh");
    assert_eq!(live.current_model_reasoning_label(), Some("xhigh"));
}

#[test]
fn launch_mode_label_is_not_used_as_model_reasoning_fallback() {
    // arrange
    // act
    // assert
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1]).with_mode_label("Live"),
    );

    assert_eq!(live.current_model_label(), "GPT-5.4 Mini");
    assert_eq!(live.current_model_reasoning_label(), None);
    assert_eq!(live.launch_mode_label(), Some("Live"));
}

#[test]
fn model_switcher_deduplicates_profile_rows_and_preserves_primary_profile() {
    // arrange
    // act
    // assert
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("default", "default:gpt-5.4-mini")
            .with_available_models(duplicate_primary_subagent_models()),
    );

    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.model_switcher_visible);
    assert_eq!(app.model_options.len(), 1);
    assert_eq!(app.model_options[0].profile, "default");

    for ch in "general".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert!(app.model_filtered.is_empty());

    for _ in 0..7 {
        app.handle_key(key(KeyCode::Backspace));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.active_profile(), "default");
    let intents = intents.lock().unwrap_or_abort();
    let UiIntent::SwitchModel { profile, .. } = intents.last().unwrap_or_abort() else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "default");
}

#[test]
fn variant_cycle_updates_selected_model_without_losing_launch_metadata() {
    // arrange
    // act
    // assert
    let _config = load_config_from_str(rich_model_config()).unwrap_or_abort();

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let available_models = same_profile_variant_options();
    let variant_cycle_overrides =
        BTreeMap::from([("variant_cycle".to_string(), "tab".to_string())]);

    let mut live = AppState::new_live(None, false, Some(sink));
    live.apply_keybindings(variant_cycle_overrides.clone());
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("default", "default:gpt-5.4-mini")
            .with_available_models(available_models.clone())
            .with_mode_label("Demo"),
    );

    live.handle_key(key(KeyCode::Tab));

    assert_eq!(live.active_profile(), "default");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(live.launch_mode_label(), Some("Demo"));

    let intents = intents.lock().unwrap_or_abort();
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents.last().unwrap_or_abort()
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "default");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(launch_metadata.mode_label(), Some("Demo"));
    assert_eq!(launch_metadata.available_models().len(), 2);

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-variant-cycle"), Vec::new());
    replay.apply_keybindings(variant_cycle_overrides);
    replay.set_launch_metadata(
        LaunchMetadata::from_model_ref("default", "default:gpt-5.4-mini")
            .with_available_models(available_models)
            .with_mode_label("Demo"),
    );

    replay.handle_key(key(KeyCode::Tab));

    assert_eq!(replay.active_profile(), "default");
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
    assert_eq!(replay.launch_mode_label(), Some("Demo"));
}

#[test]
fn ctrl_t_cycles_thinking_variant_within_current_profile() {
    // arrange
    // act
    // assert
    let _config = load_config_from_str(rich_model_config()).unwrap_or_abort();

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut live = AppState::new_live(None, false, Some(sink));
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("default", "default:gpt-5.4-mini")
            .with_available_models(same_profile_variant_options())
            .with_oauth_authentication()
            .with_mode_label("Demo"),
    );

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "default");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(live.launch_mode_label(), Some("Demo"));
    assert!(live.launch_metadata().uses_oauth_authentication());

    let intents = intents.lock().unwrap_or_abort();
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents.last().unwrap_or_abort()
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "default");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(launch_metadata.reasoning_effort(), Some("high"));
    assert!(launch_metadata.uses_oauth_authentication());
}

#[test]
fn ctrl_t_includes_base_model_entries_in_config_backed_variant_cycle() {
    // arrange
    // act
    // assert
    let _config = load_config_from_str(rich_model_config()).unwrap_or_abort();

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let available_models = config_backed_profile_model_options("default");
    assert!(available_models
        .iter()
        .any(|option| option.variant().is_none()));

    let mut live = AppState::new_live(None, false, Some(sink));
    live.set_launch_metadata(
        LaunchMetadata::from_model_ref("default", "default:gpt-5.4-mini")
            .with_available_models(available_models)
            .with_mode_label("Demo"),
    );

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "default");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini · Creative");
    assert_eq!(
        live.runtime_context_summary_segment_text(),
        Some("Next turns: GPT-5.4 Mini · Creative".to_string())
    );

    let intents = intents.lock().unwrap_or_abort();
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents.last().unwrap_or_abort()
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "default");
    assert_eq!(launch_metadata.variant(), Some("creative"));
    assert_eq!(launch_metadata.reasoning_effort(), Some("high"));
}

#[test]
fn ctrl_t_cycles_from_last_variant_to_none() {
    // arrange
    // act
    // assert
    let _config = load_config_from_str(rich_model_config()).unwrap_or_abort();

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut available_models = same_profile_variant_options();
    available_models.push(
        config_backed_profile_model_options("default")
            .into_iter()
            .find(|option| option.variant().is_none())
            .unwrap_or_abort(),
    );
    let mut live = AppState::new_live(None, false, Some(sink));
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&available_models[1])
            .with_available_models(available_models)
            .with_mode_label("Demo"),
    );

    live.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert_eq!(live.active_profile(), "default");
    assert_eq!(live.current_model_label(), "GPT-5.4 Mini");
    assert_eq!(live.launch_mode_label(), Some("Demo"));

    let intents = intents.lock().unwrap_or_abort();
    let UiIntent::SwitchModel {
        profile,
        launch_metadata,
    } = intents.last().unwrap_or_abort()
    else {
        panic!("expected switch model intent");
    };
    assert_eq!(profile, "default");
    assert_eq!(launch_metadata.variant(), None);
    assert_eq!(launch_metadata.reasoning_effort(), None);
    assert_eq!(launch_metadata.mode_label(), Some("Demo"));
    assert_eq!(launch_metadata.context_window_tokens(), Some(128_000));
    assert_eq!(launch_metadata.max_input_tokens(), Some(128_000));
    assert_eq!(launch_metadata.max_output_tokens(), Some(16_384));
}
