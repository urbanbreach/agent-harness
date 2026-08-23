use std::collections::BTreeMap;
use std::path::PathBuf;

use crossterm::event::KeyCode;
use harness_core::config::{MaxInputSemantics, ModelLimitProvenanceKind};
use harness_core::UnwrapOrAbort;
use harness_tui::app::{AppState, LaunchMetadata};

use crate::model_switcher_fixtures::*;

#[test]
fn runtime_context_labels_distinguish_live_continue_and_replay() {
    // arrange
    // act
    // assert
    let available_models = same_profile_variant_options();
    let launch_metadata = LaunchMetadata::from_model_option(&available_models[0])
        .with_available_models(available_models);

    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(launch_metadata.clone());
    assert_eq!(
        startup.runtime_context_primary_summary(),
        "Launch: GPT-5.4 Mini · Deterministic"
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
        "Context: GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(live.runtime_context_summary_segment_text(), None);

    let mut continued = AppState::new_live(None, false, None);
    continued.set_launch_metadata(launch_metadata.clone().with_mode_label("Continued"));
    assert_eq!(
        continued.runtime_context_primary_summary(),
        "Context: GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(continued.runtime_context_summary_segment_text(), None);

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-runtime-context"), Vec::new());
    replay.set_launch_metadata(launch_metadata);
    assert_eq!(
        replay.runtime_context_primary_summary(),
        "Recorded runtime · read-only: GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay.runtime_context_summary_segment_text(), None);
    assert_eq!(
        replay.runtime_context_provider_display(),
        Some("default".to_string())
    );
}

#[test]
fn model_switcher_preserves_resolved_limits_and_provenance() {
    // arrange
    let option = config_backed_profile_model_options("default")
        .into_iter()
        .find(|option| option.variant() == Some("deterministic"))
        .unwrap_or_abort();

    // act
    let metadata = LaunchMetadata::from_model_option(&option);

    // assert
    assert_eq!(metadata.context_window_tokens(), Some(128_000));
    assert_eq!(metadata.max_input_tokens(), Some(128_000));
    assert_eq!(metadata.max_output_tokens(), Some(4_096));
    assert_eq!(
        metadata.model_limits().max_input_semantics,
        MaxInputSemantics::ProviderVisibleInputTokens
    );
    assert_eq!(
        metadata.model_limits().context_window.provenance.kind,
        ModelLimitProvenanceKind::ExplicitConfig
    );
    assert_eq!(
        metadata.model_limits().max_input.provenance.kind,
        ModelLimitProvenanceKind::ExplicitConfig
    );
    assert_eq!(
        metadata.model_limits().max_output.provenance.kind,
        ModelLimitProvenanceKind::ExplicitConfig
    );
}

#[test]
fn live_switch_model_labels_next_turn_only() {
    // arrange
    // act
    // assert
    let variant_cycle_overrides =
        BTreeMap::from([("variant_cycle".to_string(), "tab".to_string())]);
    let available_models = same_profile_variant_options();
    let launch_metadata = LaunchMetadata::from_model_option(&available_models[0])
        .with_available_models(available_models);

    let mut live = AppState::new_live(None, false, None);
    live.apply_keybindings(variant_cycle_overrides.clone());
    live.set_launch_metadata(launch_metadata.clone());

    live.handle_key(key(KeyCode::Tab));

    assert_eq!(
        live.runtime_context_primary_summary(),
        "Context: GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(
        live.runtime_context_summary_segment_text(),
        Some("Next turns: GPT-5.4 Mini · Creative".to_string())
    );

    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/replay-runtime-context-switch"),
        Vec::new(),
    );
    replay.apply_keybindings(variant_cycle_overrides);
    replay.set_launch_metadata(launch_metadata);

    replay.handle_key(key(KeyCode::Tab));

    assert_eq!(
        replay.runtime_context_primary_summary(),
        "Recorded runtime · read-only: GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay.runtime_context_summary_segment_text(), None);
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
}
