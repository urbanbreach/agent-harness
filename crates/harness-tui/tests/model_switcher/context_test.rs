use harness_tui::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crossterm::event::KeyCode;
use harness_core::config::load_config_from_str;
use harness_tui::app::{AppState, LaunchMetadata};

use crate::model_switcher_fixtures::*;

#[test]
fn runtime_context_labels_distinguish_live_continue_and_replay() {
    let _config = load_config_from_str(rich_model_config()).unwrap_or_abort();

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
        "Context: deep · GPT-5.4 Mini · Deterministic"
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
    let _config = load_config_from_str(rich_model_config()).unwrap_or_abort();

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
