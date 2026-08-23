// allow: SIZE_OK — session management (lineage + projection + inspection)
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentModelRef;
use crate::config::{
    registered_profile_model_metadata, ResolvedModelLimits, ResolvedModelTarget,
    ResolvedProfileModelMetadata,
};
use crate::event::{first_lineage_parent_session_id, EventEnvelopeV1, EventV1};
use crate::session_paths::META_FILE_NAME;

use super::{project_resume_plan, ProjectionError, ResumePlan, RunStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionModeSource {
    InteractiveLive,
    InteractiveMock,
    Prompt,
    ScenarioFixture,
    ReplayOnly,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecordedRuntimeContext {
    pub profile: String,
    #[serde(default)]
    pub profile_description: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub provider_display_label: Option<String>,
    #[serde(default)]
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub variant: Option<String>,
    pub display_label: String,
    #[serde(default)]
    pub model_display_label: Option<String>,
    #[serde(default)]
    pub variant_display_label: Option<String>,
    pub token_window_label: Option<String>,
    #[serde(default)]
    pub model_limits: ResolvedModelLimits,
    // Compatibility mirrors consumed by the pre-M03 compaction path.
    pub context_window_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub recommended_for: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    #[serde(default)]
    pub thinking: Option<Value>,
}

impl RecordedRuntimeContext {
    pub fn from_model_target(profile: &str, target: &ResolvedModelTarget) -> Self {
        let registered = registered_profile_model_metadata(profile);
        let matching_identity = registered.as_ref().is_some_and(|metadata| {
            metadata.provider == target.provider && metadata.model == target.model
        });
        let matching_variant = matching_identity
            && registered
                .as_ref()
                .is_some_and(|metadata| metadata.variant == target.variant);
        let profile_description = registered
            .as_ref()
            .and_then(|metadata| metadata.profile_description.clone());
        let selected_catalog_entry = target.catalog_entry.as_deref().filter(|entry| {
            entry.provider == target.provider
                && entry.model == target.model
                && entry.variant == target.variant
        });
        let mut recorded = if let Some(entry) = selected_catalog_entry {
            Self {
                profile: profile.to_string(),
                profile_description,
                provider: entry.provider.clone(),
                provider_display_label: Some(entry.provider_display_label.clone()),
                provider_backend_label: entry.provider_backend_label.clone(),
                model: entry.model.clone(),
                variant: entry.variant.clone(),
                display_label: entry.display_label.clone(),
                model_display_label: Some(entry.model_display_label.clone()),
                variant_display_label: entry.variant_display_label.clone(),
                token_window_label: entry.token_window_label.clone(),
                model_limits: entry.limits.clone(),
                context_window_tokens: entry.limits.context_window_tokens(),
                max_input_tokens: entry.limits.max_input_tokens(),
                max_output_tokens: entry.limits.max_output_tokens(),
                description: entry.description.clone(),
                recommended_for: entry.recommended_for.clone(),
                reasoning_effort: entry.reasoning_effort.clone(),
                text_verbosity: entry.text_verbosity.clone(),
                thinking: entry.thinking.clone(),
            }
        } else if let Some(metadata) = registered.filter(|_| matching_identity) {
            Self::from(metadata)
        } else {
            let display_label = target.variant.as_ref().map_or_else(
                || target.model.clone(),
                |variant| format!("{} · {variant}", target.model),
            );
            Self {
                profile: profile.to_string(),
                profile_description,
                provider: target.provider.clone(),
                provider_display_label: None,
                provider_backend_label: None,
                model: target.model.clone(),
                variant: target.variant.clone(),
                display_label,
                model_display_label: Some(target.model.clone()),
                variant_display_label: target.variant.clone(),
                token_window_label: None,
                model_limits: target.limits.clone(),
                context_window_tokens: target.limits.context_window_tokens(),
                max_input_tokens: target.limits.max_input_tokens(),
                max_output_tokens: target.limits.max_output_tokens(),
                description: None,
                recommended_for: None,
                reasoning_effort: target.reasoning_effort.clone(),
                text_verbosity: target.text_verbosity.clone(),
                thinking: target.thinking.clone(),
            }
        };
        recorded.provider = target.provider.clone();
        recorded.model = target.model.clone();
        recorded.variant = target.variant.clone();
        recorded.model_limits = target.limits.clone();
        recorded.context_window_tokens = target.limits.context_window_tokens();
        recorded.max_input_tokens = target.limits.max_input_tokens();
        recorded.max_output_tokens = target.limits.max_output_tokens();
        recorded.reasoning_effort = target.reasoning_effort.clone();
        recorded.text_verbosity = target.text_verbosity.clone();
        recorded.thinking = target.thinking.clone();
        if selected_catalog_entry.is_none() && !matching_variant {
            let model_label = recorded
                .model_display_label
                .clone()
                .unwrap_or_else(|| target.model.clone());
            recorded.display_label = target.variant.as_ref().map_or_else(
                || model_label.clone(),
                |variant| format!("{model_label} · {variant}"),
            );
            recorded.variant_display_label = target.variant.clone();
            recorded.token_window_label = None;
            recorded.description = None;
            recorded.recommended_for = None;
        }
        recorded
    }

    pub fn from_profile_model(profile: &str, model_ref: &str) -> Self {
        let model_ref = AgentModelRef::parse(model_ref);
        if let Some(metadata) = registered_profile_model_metadata(profile) {
            if metadata.provider == model_ref.provider_id && metadata.model == model_ref.model_id {
                return Self::from(metadata);
            }
        }

        let display_label = model_ref.model_id.clone();

        Self {
            profile: profile.to_string(),
            profile_description: None,
            provider: model_ref.provider_id,
            provider_display_label: None,
            provider_backend_label: None,
            model: model_ref.model_id,
            variant: None,
            display_label,
            model_display_label: None,
            variant_display_label: None,
            token_window_label: None,
            model_limits: ResolvedModelLimits::default(),
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            recommended_for: None,
            reasoning_effort: None,
            text_verbosity: None,
            thinking: None,
        }
    }

    pub fn effective_model_limits(&self) -> ResolvedModelLimits {
        if self.model_limits == ResolvedModelLimits::default()
            && (self.context_window_tokens.is_some()
                || self.max_input_tokens.is_some()
                || self.max_output_tokens.is_some())
        {
            return ResolvedModelLimits::compatibility_mirror(
                self.context_window_tokens,
                self.max_input_tokens,
                self.max_output_tokens,
            );
        }
        self.model_limits.clone()
    }

    fn hydrate_legacy_model_limits(&mut self) {
        self.model_limits = self.effective_model_limits();
    }
}

impl From<ResolvedProfileModelMetadata> for RecordedRuntimeContext {
    fn from(metadata: ResolvedProfileModelMetadata) -> Self {
        Self {
            profile: metadata.profile,
            profile_description: metadata.profile_description,
            provider: metadata.provider,
            provider_display_label: Some(metadata.provider_display_label),
            provider_backend_label: metadata.provider_backend_label,
            model: metadata.model,
            variant: metadata.variant,
            display_label: metadata.display_label,
            model_display_label: Some(metadata.model_display_label),
            variant_display_label: metadata.variant_display_label,
            token_window_label: metadata.token_window_label,
            context_window_tokens: metadata.limits.context_window_tokens(),
            max_input_tokens: metadata.limits.max_input_tokens(),
            max_output_tokens: metadata.limits.max_output_tokens(),
            model_limits: metadata.limits,
            description: metadata.description,
            recommended_for: metadata.recommended_for,
            reasoning_effort: metadata.reasoning_effort,
            text_verbosity: metadata.text_verbosity,
            thinking: metadata.thinking,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetadata {
    pub run_id: String,
    pub run_name: String,
    pub workspace_root: String,
    #[serde(default)]
    pub created_at: Option<String>,
    pub config_digest: String,
    pub harness_version: String,
    #[serde(default)]
    pub recorded_runtime_context: Option<RecordedRuntimeContext>,
    #[serde(default)]
    pub mode_source: Option<SessionModeSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionCatalogMetadata {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_name: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub profile_preset: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub recorded_runtime_context: Option<RecordedRuntimeContext>,
    #[serde(default)]
    pub mode_source: Option<SessionModeSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCatalogEntry {
    pub run_id: String,
    pub run_name: Option<String>,
    pub status: Option<RunStatus>,
    pub last_updated_at: Option<String>,
    pub workspace_root: Option<String>,
    pub profile_preset: Option<String>,
    pub provider_model: Option<String>,
    pub mode_source: SessionModeSource,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
    pub artifact_count: usize,
    pub child_session_count: usize,
    pub parent_session_id: Option<String>,
}

impl SessionCatalogEntry {
    pub fn is_default_picker_candidate(&self) -> bool {
        matches!(
            self.mode_source,
            SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock
        )
    }
}

pub fn load_run_metadata(run_dir: &Path) -> Option<RunMetadata> {
    let body = fs::read_to_string(run_dir.join(META_FILE_NAME)).ok()?;
    let mut metadata: RunMetadata = serde_json::from_str(&body).ok()?;
    if let Some(context) = metadata.recorded_runtime_context.as_mut() {
        context.hydrate_legacy_model_limits();
    }
    Some(metadata)
}

pub fn project_session_catalog_entry<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    fallback_run_id: &str,
    metadata: Option<&SessionCatalogMetadata>,
    last_updated_at: Option<String>,
    degraded_reason: Option<String>,
) -> Result<SessionCatalogEntry, ProjectionError> {
    let collected = events.into_iter().collect::<Vec<_>>();
    let recorded_runtime_context = metadata.and_then(|meta| meta.recorded_runtime_context.as_ref());

    let run_started = collected.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(data) => Some(data),
        _ => None,
    });
    let spawned = collected.iter().find_map(|event| match &event.payload {
        EventV1::AgentSpawned(data) => Some(data),
        _ => None,
    });
    let provider_started = collected
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data) => Some(data),
            _ => None,
        });

    let latest_title = collected
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::SessionTitleUpdated(data) => Some(data.title.clone()),
            _ => None,
        });

    let run_name = latest_title
        .or_else(|| run_started.map(|data| data.run_name.to_string()))
        .or_else(|| metadata.and_then(|meta| meta.run_name.clone()));
    let workspace_root = run_started
        .map(|data| data.workspace_root.clone())
        .or_else(|| metadata.and_then(|meta| meta.workspace_root.clone()));
    let profile_preset = spawned
        .map(|data| data.profile.clone())
        .or_else(|| recorded_runtime_context.map(|context| context.profile.clone()))
        .or_else(|| metadata.and_then(|meta| meta.profile_preset.clone()));
    let provider = provider_started
        .map(|data| data.provider_id.clone())
        .or_else(|| recorded_runtime_context.map(|context| context.provider.clone()))
        .or_else(|| metadata.and_then(|meta| meta.provider.clone()));
    let model = provider_started
        .map(|data| data.model_id.clone())
        .or_else(|| recorded_runtime_context.map(|context| context.model.clone()))
        .or_else(|| metadata.and_then(|meta| meta.model.clone()));

    let provider_model = match (provider.as_deref(), model.as_deref()) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (Some(provider), None) => Some(format!("{provider}/<unavailable>")),
        (None, Some(model)) => Some(format!("<unavailable>/{model}")),
        (None, None) => None,
    };

    let mode_source = metadata
        .and_then(|meta| meta.mode_source)
        .unwrap_or_else(|| infer_mode_source(run_name.as_deref(), provider.as_deref()));

    let run_id = collected
        .first()
        .map(|event| event.run_id.to_string())
        .or_else(|| metadata.and_then(|meta| meta.run_id.clone()))
        .unwrap_or_else(|| fallback_run_id.to_string());

    let resume_plan = project_resume_plan(collected.iter().copied(), fallback_run_id)?;
    let status = resume_plan.run_status();
    let artifact_count = resume_plan_artifact_count(&resume_plan);
    let child_session_count = resume_plan_child_session_count(&resume_plan);
    let parent_session_id =
        first_lineage_parent_session_id(collected.iter().copied()).map(str::to_string);

    let resume_disabled_reason = resume_disabled_reason(
        mode_source,
        &resume_plan,
        profile_preset.as_deref(),
        provider_model.as_deref(),
        degraded_reason,
    );

    Ok(SessionCatalogEntry {
        run_id,
        run_name,
        status,
        last_updated_at,
        workspace_root,
        profile_preset,
        provider_model,
        mode_source,
        is_resumable: resume_disabled_reason.is_none(),
        resume_disabled_reason,
        artifact_count,
        child_session_count,
        parent_session_id,
    })
}

fn resume_plan_artifact_count(plan: &ResumePlan) -> usize {
    plan.session_artifacts.len()
}

fn resume_plan_child_session_count(plan: &ResumePlan) -> usize {
    plan.child_sessions
        .values()
        .filter(|child| {
            child.parent_session_id.is_some()
                || child.parent_tool_call_id.is_some()
                || child.parent_task_id.is_some()
                || child.parent_request_id.is_some()
        })
        .count()
}

fn infer_mode_source(run_name: Option<&str>, provider: Option<&str>) -> SessionModeSource {
    match run_name.unwrap_or_default() {
        "interactive" => {
            if provider == Some("mock") {
                SessionModeSource::InteractiveMock
            } else {
                SessionModeSource::InteractiveLive
            }
        }
        "prompt" => SessionModeSource::Prompt,
        "replay" => SessionModeSource::ReplayOnly,
        "golden_path" | "golden_path_interactive" => SessionModeSource::ScenarioFixture,
        _ => SessionModeSource::Unknown,
    }
}

fn resume_disabled_reason(
    mode_source: SessionModeSource,
    resume_plan: &ResumePlan,
    profile_preset: Option<&str>,
    provider_model: Option<&str>,
    degraded_reason: Option<String>,
) -> Option<String> {
    if let Some(reason) = degraded_reason {
        return Some(reason);
    }

    match mode_source {
        SessionModeSource::ScenarioFixture => {
            return Some("scenario fixture runs are excluded from resume".to_string());
        }
        SessionModeSource::ReplayOnly => {
            return Some("replay-only launches are not resumable".to_string());
        }
        SessionModeSource::Prompt => {
            return Some("prompt runs are not resumable".to_string());
        }
        SessionModeSource::Unknown => {
            return Some("session mode source is unavailable".to_string());
        }
        SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock => {}
    }

    if let Some(reason) = &resume_plan.resume_disabled_reason {
        return Some(reason.clone());
    }
    if profile_preset.is_none() {
        return Some("profile preset is unavailable".to_string());
    }
    if provider_model.is_none() {
        return Some("provider/model is unavailable".to_string());
    }

    None
}
