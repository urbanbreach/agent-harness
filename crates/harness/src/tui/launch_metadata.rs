// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
use std::collections::{BTreeMap, BTreeSet};

use harness_core::agent::{AgentModelSettings, AgentProfile};
use harness_core::config::{
    configured_model_catalog, resolve_profile_model_metadata, HarnessConfig,
};
use harness_core::event::{ActorKind, EventEnvelopeV1, EventV1};
use harness_core::proj::RecordedRuntimeContext;
use harness_tui::app::{LaunchMetadata, ModelOption};

pub(super) fn interactive_launch_metadata(
    config: Option<&HarnessConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    profile: &str,
) -> Result<LaunchMetadata, String> {
    let Some(selected_profile) = agent_profiles.get(profile) else {
        return Err(format!(
            "interactive mode requires a configured profile named `{profile}`"
        ));
    };

    let available_models = model_options_for_profiles(config, agent_profiles, profile);
    let launch_metadata = config
        .and_then(|config| resolve_profile_model_metadata(config, profile).ok())
        .map(|metadata| {
            LaunchMetadata::from_model_option(&ModelOption {
                profile: metadata.profile,
                provider: metadata.provider,
                provider_display_label: Some(metadata.provider_display_label),
                provider_backend_label: metadata.provider_backend_label,
                model: metadata.model,
                model_display_label: Some(metadata.model_display_label),
                variant: metadata.variant,
                variant_display_label: metadata.variant_display_label,
                display_label: Some(metadata.display_label),
                token_window_label: metadata.token_window_label,
                context_window_tokens: metadata.context_window_tokens,
                max_input_tokens: metadata.max_input_tokens,
                max_output_tokens: metadata.max_output_tokens,
                description: metadata.description,
                profile_description: metadata.profile_description,
                reasoning_effort: metadata.reasoning_effort,
                text_verbosity: metadata.text_verbosity,
                thinking: metadata.thinking,
                recommended_for: metadata.recommended_for,
            })
        })
        .unwrap_or_else(|| {
            LaunchMetadata::from_model_ref(
                selected_profile.name.clone(),
                &selected_profile.model_ref,
            )
        });

    Ok(launch_metadata
        .with_available_models(available_models)
        .with_switchable_profiles(switchable_profile_names(config, agent_profiles, profile)))
}

fn switchable_profile_names(
    config: Option<&HarnessConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    selected_profile: &str,
) -> Vec<String> {
    let mut profiles = config
        .map(|config| {
            config
                .agents
                .iter()
                .filter(|(name, profile)| {
                    agent_profiles.contains_key(name.as_str())
                        && !profile.hidden
                        && !profile.mode.is_subagent_only()
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if profiles.is_empty() {
        profiles = ["default"]
            .into_iter()
            .filter(|profile| agent_profiles.contains_key(*profile))
            .map(str::to_string)
            .collect();
    }

    if profiles.is_empty() && agent_profiles.contains_key(selected_profile) {
        profiles.push(selected_profile.to_string());
    }

    let mut ordered = Vec::new();
    if let Some(index) = profiles
        .iter()
        .position(|profile| profile == selected_profile)
    {
        ordered.push(profiles.remove(index));
    }
    for profile in profiles {
        if !ordered.contains(&profile) {
            ordered.push(profile);
        }
    }
    ordered
}

fn model_options_for_profiles(
    config: Option<&HarnessConfig>,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    selected_profile: &str,
) -> Vec<ModelOption> {
    config
        .map(|config| configured_profile_model_options(config, agent_profiles, selected_profile))
        .unwrap_or_else(|| model_options_from_profiles(agent_profiles))
}

fn configured_profile_model_options(
    config: &HarnessConfig,
    agent_profiles: &BTreeMap<String, AgentProfile>,
    selected_profile: &str,
) -> Vec<ModelOption> {
    let catalog_entries = configured_model_catalog(config);
    let mut options = Vec::new();

    if agent_profiles.contains_key(selected_profile) {
        let profile_description = resolve_profile_model_metadata(config, selected_profile)
            .ok()
            .and_then(|metadata| metadata.profile_description);
        for entry in &catalog_entries {
            let option = ModelOption {
                profile: selected_profile.to_string(),
                provider: entry.provider.clone(),
                provider_display_label: Some(entry.provider_display_label.clone()),
                provider_backend_label: entry.provider_backend_label.clone(),
                model: entry.model.clone(),
                model_display_label: Some(entry.model_display_label.clone()),
                variant: entry.variant.clone(),
                variant_display_label: entry.variant_display_label.clone(),
                display_label: Some(entry.display_label.clone()),
                token_window_label: entry.token_window_label.clone(),
                context_window_tokens: entry.context_window_tokens,
                max_input_tokens: entry.max_input_tokens,
                max_output_tokens: entry.max_output_tokens,
                description: entry.description.clone(),
                profile_description: profile_description.clone(),
                reasoning_effort: entry.reasoning_effort.clone(),
                text_verbosity: entry.text_verbosity.clone(),
                thinking: entry.thinking.clone(),
                recommended_for: entry.recommended_for.clone(),
            };

            if !options.iter().any(|existing| existing == &option) {
                options.push(option);
            }
        }
    }

    for profile in agent_profiles.keys() {
        if let Ok(metadata) = resolve_profile_model_metadata(config, profile) {
            let configured_provider = metadata.provider.clone();
            let configured_model = metadata.model.clone();
            let profile_description = metadata.profile_description.clone();

            for entry in catalog_entries.iter().filter(|entry| {
                entry.provider == configured_provider && entry.model == configured_model
            }) {
                let option = ModelOption {
                    profile: profile.clone(),
                    provider: entry.provider.clone(),
                    provider_display_label: Some(entry.provider_display_label.clone()),
                    provider_backend_label: entry.provider_backend_label.clone(),
                    model: entry.model.clone(),
                    model_display_label: Some(entry.model_display_label.clone()),
                    variant: entry.variant.clone(),
                    variant_display_label: entry.variant_display_label.clone(),
                    display_label: Some(entry.display_label.clone()),
                    token_window_label: entry.token_window_label.clone(),
                    context_window_tokens: entry.context_window_tokens,
                    max_input_tokens: entry.max_input_tokens,
                    max_output_tokens: entry.max_output_tokens,
                    description: entry.description.clone(),
                    profile_description: profile_description.clone(),
                    reasoning_effort: entry.reasoning_effort.clone(),
                    text_verbosity: entry.text_verbosity.clone(),
                    thinking: entry.thinking.clone(),
                    recommended_for: entry.recommended_for.clone(),
                };

                if !options.iter().any(|existing| existing == &option) {
                    options.push(option);
                }
            }

            let preferred = ModelOption {
                profile: profile.clone(),
                provider: metadata.provider,
                provider_display_label: Some(metadata.provider_display_label),
                provider_backend_label: metadata.provider_backend_label,
                model: metadata.model,
                model_display_label: Some(metadata.model_display_label),
                variant: metadata.variant,
                variant_display_label: metadata.variant_display_label,
                display_label: Some(metadata.display_label),
                token_window_label: metadata.token_window_label,
                context_window_tokens: metadata.context_window_tokens,
                max_input_tokens: metadata.max_input_tokens,
                max_output_tokens: metadata.max_output_tokens,
                description: metadata.description,
                profile_description: metadata.profile_description,
                reasoning_effort: metadata.reasoning_effort,
                text_verbosity: metadata.text_verbosity,
                thinking: metadata.thinking,
                recommended_for: metadata.recommended_for,
            };

            if !options.iter().any(|option| option == &preferred) {
                options.push(preferred);
            }
        }
    }

    options
}

fn model_options_from_profiles(
    agent_profiles: &BTreeMap<String, AgentProfile>,
) -> Vec<ModelOption> {
    agent_profiles
        .values()
        .map(|profile| ModelOption::from_model_ref(profile.name.clone(), &profile.model_ref))
        .collect()
}

pub(super) fn launch_metadata_for_connected_providers(
    launch_metadata: LaunchMetadata,
    connected_provider_ids: &[String],
    no_provider_connected: bool,
) -> LaunchMetadata {
    if no_provider_connected {
        return LaunchMetadata::new(launch_metadata.profile().to_string(), "local", None)
            .with_switchable_profiles(launch_metadata.switchable_profiles().to_vec());
    }
    if connected_provider_ids.is_empty() {
        return launch_metadata;
    }

    let connected = connected_provider_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let available = launch_metadata
        .available_models()
        .iter()
        .filter(|option| connected.contains(option.provider.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let current_connected = connected.contains(launch_metadata.provider());
    let mut selected = if current_connected {
        launch_metadata.clone()
    } else if let Some(first) = available
        .iter()
        .find(|option| option.profile == launch_metadata.profile())
        .or_else(|| available.first())
    {
        LaunchMetadata::from_model_option(first)
    } else {
        LaunchMetadata::new(launch_metadata.profile().to_string(), "local", None)
    };
    selected = selected
        .with_available_models(available)
        .with_switchable_profiles(launch_metadata.switchable_profiles().to_vec());
    selected
}

pub(super) fn launch_metadata_model_ref(launch_metadata: &LaunchMetadata) -> Option<String> {
    Some(format!(
        "{}:{}",
        launch_metadata.provider(),
        launch_metadata.model()?
    ))
}

pub(super) fn launch_metadata_model_settings(
    launch_metadata: &LaunchMetadata,
) -> AgentModelSettings {
    AgentModelSettings {
        variant: launch_metadata.variant().map(str::to_string),
        reasoning_effort: launch_metadata.reasoning_effort().map(str::to_string),
        text_verbosity: launch_metadata.text_verbosity().map(str::to_string),
        reasoning_summary: launch_metadata
            .reasoning_effort()
            .map(|_| "auto".to_string()),
        thinking: launch_metadata.thinking().cloned(),
    }
}

fn launch_metadata_from_recorded_runtime_context(
    recorded_runtime_context: &RecordedRuntimeContext,
) -> LaunchMetadata {
    LaunchMetadata::from_model_option(&ModelOption {
        profile: recorded_runtime_context.profile.clone(),
        provider: recorded_runtime_context.provider.clone(),
        provider_display_label: recorded_runtime_context.provider_display_label.clone(),
        provider_backend_label: recorded_runtime_context.provider_backend_label.clone(),
        model: recorded_runtime_context.model.clone(),
        model_display_label: recorded_runtime_context.model_display_label.clone(),
        variant: recorded_runtime_context.variant.clone(),
        variant_display_label: recorded_runtime_context.variant_display_label.clone(),
        display_label: Some(recorded_runtime_context.display_label.clone())
            .filter(|value| metadata_value_present(value)),
        token_window_label: recorded_runtime_context.token_window_label.clone(),
        context_window_tokens: recorded_runtime_context.context_window_tokens,
        max_input_tokens: recorded_runtime_context.max_input_tokens,
        max_output_tokens: recorded_runtime_context.max_output_tokens,
        description: recorded_runtime_context.description.clone(),
        profile_description: recorded_runtime_context.profile_description.clone(),
        reasoning_effort: recorded_runtime_context.reasoning_effort.clone(),
        text_verbosity: recorded_runtime_context.text_verbosity.clone(),
        thinking: recorded_runtime_context.thinking.clone(),
        recommended_for: recorded_runtime_context.recommended_for.clone(),
    })
}

fn metadata_value_present(value: &str) -> bool {
    !value.trim().is_empty()
}

pub(super) fn replay_launch_metadata(
    recorded_runtime_context: Option<&RecordedRuntimeContext>,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    let fallback = LaunchMetadata::default().with_mode_label("Replay");
    if let Some(recorded_runtime_context) = recorded_runtime_context {
        return launch_metadata_from_recorded_runtime_context(recorded_runtime_context)
            .with_mode_label("Replay");
    }
    if historical_events.is_empty() {
        return fallback;
    }

    let profile = historical_events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(payload) => Some(payload.profile.clone()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.profile().to_string());
    let (provider, model) = historical_events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(payload) => {
                Some((payload.provider_id.clone(), Some(payload.model_id.clone())))
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            (
                fallback.provider().to_string(),
                fallback.model().map(str::to_string),
            )
        });

    LaunchMetadata::new(profile, provider, model).with_mode_label("Replay")
}

pub(super) fn continue_launch_metadata(
    run_id: &str,
    recorded_runtime_context: Option<&RecordedRuntimeContext>,
    historical_events: &[EventEnvelopeV1],
    resume_agent_id: &str,
    resume_profile: Option<&str>,
) -> LaunchMetadata {
    let fallback =
        LaunchMetadata::from_model_ref("unknown", "unknown:unknown").with_mode_label("Continued");
    if let Some(recorded_runtime_context) = recorded_runtime_context {
        return launch_metadata_from_recorded_runtime_context(recorded_runtime_context)
            .with_mode_label("Continued");
    }
    if historical_events.is_empty() {
        return fallback;
    }

    let profile = resume_profile.map(str::to_string).or_else(|| {
        historical_events.iter().rev().find_map(|event| {
            let EventV1::AgentSpawned(data) = &event.payload else {
                return None;
            };
            (data.agent_id == resume_agent_id).then(|| data.profile.clone())
        })
    });
    let provider_started = historical_events.iter().rev().find_map(|event| {
        let EventV1::ProviderRequestStarted(data) = &event.payload else {
            return None;
        };
        if event.actor.kind != ActorKind::Worker
            || event.actor.agent_id.as_deref() != Some(resume_agent_id)
        {
            return None;
        }
        Some((data.provider_id.clone(), data.model_id.clone()))
    });

    let (provider, model) =
        provider_started.unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

    LaunchMetadata::new(
        profile.unwrap_or_else(|| format!("resumed:{run_id}")),
        provider,
        Some(model),
    )
    .with_mode_label("Continued")
}
