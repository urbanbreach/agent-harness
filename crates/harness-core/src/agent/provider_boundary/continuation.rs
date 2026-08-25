use harness_providers::{ToolChoice, ToolDef};
use serde::Serialize;

use super::{
    apply_provider_request_context, convert_projected_context_to_provider_messages,
    transform_context_for_provider, ProviderBoundaryContext, ProviderBoundaryInput,
    ProviderBoundaryOutput,
};
use crate::agent::{AgentModelRef, AgentModelSettings, AgentProfile, ProviderConversationTurn};
use crate::config::ResolvedModelLimits;
use crate::session::{CanonicalProviderView, CanonicalRuntimeSelection};

mod projection;

#[derive(Debug)]
pub struct LowerProviderContinuationInput<'a> {
    pub view: &'a CanonicalProviderView,
    pub transient_operational_turns: &'a [ProviderConversationTurn],
    pub profile: &'a AgentProfile,
    pub tools: Option<Vec<ToolDef>>,
    pub tool_choice: Option<ToolChoice>,
    pub fresh_request_id: &'a str,
}

pub struct CanonicalRuntimeSelectionInput<'a> {
    pub profile: &'a AgentProfile,
    pub model: &'a AgentModelRef,
    pub settings: AgentModelSettings,
    pub resolved_limits: ResolvedModelLimits,
    pub tools: &'a [ToolDef],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProviderContinuationLoweringError {
    #[error("provider profile/tool shape mismatch: persisted {persisted}, current {current}")]
    ProfileToolShapeMismatch { persisted: String, current: String },
    #[error("canonical attachment metadata is invalid: {reason}")]
    InvalidAttachment { reason: String },
    #[error("provider profile/tool shape serialization failed: {reason}")]
    ProfileToolShapeSerialization { reason: String },
    #[error("canonical runtime selection is invalid: {reason}")]
    InvalidRuntimeSelection { reason: String },
}

pub fn profile_tool_shape_digest(
    profile: &AgentProfile,
    tools: &[ToolDef],
) -> Result<String, ProviderContinuationLoweringError> {
    #[derive(Serialize)]
    struct ProfileToolShape<'a> {
        profile_name: &'a str,
        system_prompt: &'a str,
        temperature_bits: Option<u32>,
        cache_retention: harness_providers::CacheRetention,
        max_iters: Option<usize>,
        tool_failure_mode: &'a crate::config::ToolFailureMode,
        permission_ruleset: &'a crate::perm::PermissionRuleset,
        tools: &'a [ToolDef],
    }

    let shape = ProfileToolShape {
        profile_name: &profile.name,
        system_prompt: &profile.system_prompt,
        temperature_bits: profile.temperature.map(f32::to_bits),
        cache_retention: profile.cache_retention,
        max_iters: profile.max_iters,
        tool_failure_mode: &profile.tool_failure_mode,
        permission_ruleset: &profile.permission_ruleset,
        tools,
    };
    let bytes = serde_json::to_vec(&shape).map_err(|error| {
        ProviderContinuationLoweringError::ProfileToolShapeSerialization {
            reason: error.to_string(),
        }
    })?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

pub fn canonical_runtime_selection(
    input: CanonicalRuntimeSelectionInput<'_>,
) -> Result<CanonicalRuntimeSelection, ProviderContinuationLoweringError> {
    let CanonicalRuntimeSelectionInput {
        profile,
        model,
        settings,
        resolved_limits,
        tools,
    } = input;
    CanonicalRuntimeSelection::new(
        Some(profile.name.clone()),
        model.provider_id.clone(),
        model.model_id.clone(),
        settings,
        resolved_limits,
        profile_tool_shape_digest(profile, tools)?,
    )
    .map_err(
        |error| ProviderContinuationLoweringError::InvalidRuntimeSelection {
            reason: error.to_string(),
        },
    )
}

pub fn lower_provider_continuation(
    input: LowerProviderContinuationInput<'_>,
) -> Result<ProviderBoundaryOutput, ProviderContinuationLoweringError> {
    let LowerProviderContinuationInput {
        view,
        transient_operational_turns,
        profile,
        tools,
        tool_choice,
        fresh_request_id,
    } = input;
    let current_digest = profile_tool_shape_digest(profile, tools.as_deref().unwrap_or_default())?;
    if current_digest != view.runtime_selection.profile_tool_shape_digest {
        return Err(
            ProviderContinuationLoweringError::ProfileToolShapeMismatch {
                persisted: view.runtime_selection.profile_tool_shape_digest.clone(),
                current: current_digest,
            },
        );
    }

    validate_attachments(view)?;

    let messages = provider_messages(
        view,
        profile,
        projection::conversation_messages_with_transient_overlay(view, transient_operational_turns),
    );
    let model = AgentModelRef {
        provider_id: view.runtime_selection.provider_id.clone(),
        model_id: view.runtime_selection.model_id.clone(),
    };
    let model_settings = AgentModelSettings {
        variant: view.runtime_selection.variant.clone(),
        reasoning_effort: view.runtime_selection.reasoning_effort.clone(),
        text_verbosity: view.runtime_selection.text_verbosity.clone(),
        reasoning_summary: view.runtime_selection.reasoning_summary.clone(),
        thinking: view.runtime_selection.thinking.clone(),
    };
    let mut output = transform_context_for_provider(ProviderBoundaryInput {
        profile,
        model,
        model_settings,
        context: ProviderBoundaryContext::ProviderMessages {
            messages: &messages,
        },
        tools,
        tool_choice,
    });
    apply_provider_request_context(
        &mut output.request,
        Some(view.owner.session_id().as_str()),
        Some(fresh_request_id),
    );
    output.request.context.has_media = has_media(view);
    Ok(output)
}

pub fn canonical_provider_messages(
    view: &CanonicalProviderView,
    profile: &AgentProfile,
) -> Vec<harness_providers::CompletionMessage> {
    provider_messages(view, profile, projection::conversation_messages(view))
}

pub(crate) fn canonical_recovery_messages(
    view: &CanonicalProviderView,
) -> Vec<crate::conversation::ConversationMessage> {
    projection::recovery_conversation_messages(view)
}

fn provider_messages(
    view: &CanonicalProviderView,
    profile: &AgentProfile,
    projected: Vec<crate::conversation::ConversationMessage>,
) -> Vec<harness_providers::CompletionMessage> {
    let mut messages = convert_projected_context_to_provider_messages(profile, &projected);
    let provider_ids = projection::provider_tool_call_ids(view);
    let mut function_names = std::collections::BTreeMap::new();
    for message in &mut messages {
        if let Some(calls) = &mut message.assistant_tool_calls {
            for call in calls {
                if let Some(provider_id) = provider_ids.get(&call.tool_call_id) {
                    call.tool_call_id.clone_from(provider_id);
                }
                function_names.insert(call.tool_call_id.clone(), call.function_name.clone());
            }
        }
        if let Some(tool_call_id) = &mut message.tool_call_id {
            if let Some(provider_id) = provider_ids.get(tool_call_id) {
                tool_call_id.clone_from(provider_id);
            }
            if message.name.is_none() {
                message.name = function_names.get(tool_call_id).cloned();
            }
        }
    }
    messages
}

pub(crate) fn canonical_historical_attachment_tokens(
    view: &CanonicalProviderView,
) -> Result<u32, harness_providers::ProviderRequestCostError> {
    crate::attachment_transport::historical_attachment_tokens(
        projection::visible_historical_attachment_groups(view)
            .into_iter()
            .flatten(),
    )
}

fn validate_attachments(
    view: &CanonicalProviderView,
) -> Result<(), ProviderContinuationLoweringError> {
    for group in projection::visible_historical_attachment_groups(view) {
        let attachments = group.into_iter().cloned().collect::<Vec<_>>();
        crate::attachment_transport::validate_provider_attachments(&attachments).map_err(
            |error| ProviderContinuationLoweringError::InvalidAttachment {
                reason: error.to_string(),
            },
        )?;
    }
    if let Some(prompt) = &view.pending_prompt {
        crate::attachment_transport::validate_provider_attachments(&prompt.attachments).map_err(
            |error| ProviderContinuationLoweringError::InvalidAttachment {
                reason: error.to_string(),
            },
        )?;
    }
    Ok(())
}

fn has_media(view: &CanonicalProviderView) -> bool {
    !projection::visible_historical_attachment_groups(view).is_empty()
        || view
            .pending_prompt
            .as_ref()
            .is_some_and(|prompt| !prompt.attachments.is_empty())
}
