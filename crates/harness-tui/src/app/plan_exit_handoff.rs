use serde::Deserialize;
use serde_json::Value;

use super::{LaunchMetadata, ModelOption};

#[derive(Debug, Clone, Deserialize)]
struct PlanExitHandoffEnvelope {
    plan_exit_handoff: PlanExitHandoff,
}

#[derive(Debug, Clone, Deserialize)]
struct PlanExitHandoff {
    source_profile: String,
    target_profile: String,
    prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanExitHandoffAction {
    pub(super) target_profile: String,
    pub(super) prompt: String,
    pub(super) launch_metadata: LaunchMetadata,
}

pub(super) fn resolve_plan_exit_handoff(
    active_profile: &str,
    current_launch_metadata: &LaunchMetadata,
    output_json: &Value,
) -> Option<PlanExitHandoffAction> {
    let envelope = serde_json::from_value::<PlanExitHandoffEnvelope>(output_json.clone()).ok()?;
    let handoff = envelope.plan_exit_handoff;
    if handoff.source_profile != active_profile {
        return None;
    }

    let mut available_models = current_launch_metadata.available_models().to_vec();
    let mut launch_metadata = current_launch_metadata
        .available_models()
        .iter()
        .find(|option| option.profile == handoff.target_profile)
        .map(LaunchMetadata::from_model_option)
        .unwrap_or_else(|| {
            LaunchMetadata::new(
                handoff.target_profile.clone(),
                current_launch_metadata.provider().to_string(),
                current_launch_metadata.model().map(str::to_owned),
            )
        })
        .with_available_models({
            if !available_models
                .iter()
                .any(|option| option.profile == handoff.target_profile)
            {
                let mut inferred = current_launch_metadata
                    .to_model_option()
                    .unwrap_or_else(|| ModelOption {
                        profile: active_profile.to_string(),
                        provider: current_launch_metadata.provider().to_string(),
                        provider_display_label: current_launch_metadata
                            .provider_display_label()
                            .map(str::to_string),
                        provider_backend_label: current_launch_metadata
                            .provider_backend_label()
                            .map(str::to_string),
                        model: current_launch_metadata
                            .model()
                            .map(str::to_string)
                            .unwrap_or_default(),
                        model_display_label: current_launch_metadata
                            .model_display_label()
                            .map(str::to_string),
                        variant: current_launch_metadata.variant().map(str::to_string),
                        variant_display_label: current_launch_metadata
                            .variant_display_label()
                            .map(str::to_string),
                        display_label: current_launch_metadata.display_label().map(str::to_string),
                        token_window_label: current_launch_metadata
                            .token_window_label()
                            .map(str::to_string),
                        context_window_tokens: current_launch_metadata.context_window_tokens(),
                        max_input_tokens: current_launch_metadata.max_input_tokens(),
                        max_output_tokens: current_launch_metadata.max_output_tokens(),
                        description: current_launch_metadata.description().map(str::to_string),
                        profile_description: current_launch_metadata
                            .profile_description()
                            .map(str::to_string),
                        reasoning_effort: current_launch_metadata
                            .reasoning_effort()
                            .map(str::to_string),
                        text_verbosity: current_launch_metadata
                            .text_verbosity()
                            .map(str::to_string),
                        recommended_for: current_launch_metadata
                            .recommended_for()
                            .map(str::to_string),
                    });
                inferred.profile = handoff.target_profile.clone();
                available_models.push(inferred);
            }
            available_models
        });

    if let Some(mode_label) = current_launch_metadata.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }

    Some(PlanExitHandoffAction {
        target_profile: handoff.target_profile,
        prompt: handoff.prompt,
        launch_metadata,
    })
}
