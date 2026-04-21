use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::agent::AgentModelRef;
use harness_core::config::{registered_profile_model_metadata, ResolvedProfileModelMetadata};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{
    inspect_resume_plan, RunMetadata, SessionCatalogEntry, SessionModeSource,
};

use super::{
    json_string_field, set_pending_live_launch_metadata, set_pending_live_prompt_draft, AppState,
    Focus, PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
    PostRunHandoffAction, ReviewSurface, StartupLauncherAction, Tab, UiIntent, SLASH_COMMANDS,
};
use crate::keybindings::Action;

#[derive(Debug, Clone)]
pub(super) struct SessionNavigationSnapshot {
    pub(super) session_path: PathBuf,
    pub(super) events: Vec<EventEnvelopeV1>,
    pub(super) launch_metadata: LaunchMetadata,
    pub(super) child_session_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
}

pub(crate) fn session_history_run_name(entry: &SessionHistoryEntry) -> &str {
    entry.catalog.run_name.as_deref().unwrap_or("<unavailable>")
}

pub(crate) fn session_history_status_label(entry: &SessionHistoryEntry) -> &'static str {
    match entry.catalog.status {
        Some(harness_core::proj::RunStatus::Running) => "running",
        Some(harness_core::proj::RunStatus::Finished) => "finished",
        Some(harness_core::proj::RunStatus::Failed) => "failed",
        None => "<unavailable>",
    }
}

pub(crate) fn session_history_recency_label(entry: &SessionHistoryEntry) -> String {
    entry
        .catalog
        .last_updated_at
        .as_deref()
        .map(format_session_history_timestamp)
        .unwrap_or_else(|| "updated <unavailable>".to_string())
}

pub(crate) fn session_history_profile_label(entry: &SessionHistoryEntry) -> &str {
    entry
        .catalog
        .profile_preset
        .as_deref()
        .unwrap_or("<unavailable>")
}

pub(crate) fn session_history_provider_model_label(entry: &SessionHistoryEntry) -> &str {
    entry
        .catalog
        .provider_model
        .as_deref()
        .unwrap_or("<unavailable>")
}

pub(crate) fn session_history_resumability_label(entry: &SessionHistoryEntry) -> String {
    if entry.catalog.is_resumable {
        "continue ready".to_string()
    } else {
        entry
            .catalog
            .resume_disabled_reason
            .as_deref()
            .map(|reason| format!("continue blocked · {reason}"))
            .unwrap_or_else(|| "continue blocked".to_string())
    }
}

fn artifact_count_label(count: usize) -> String {
    match count {
        0 => "no artifacts".to_string(),
        1 => "1 artifact".to_string(),
        count => format!("{count} artifacts"),
    }
}

fn lineage_label(child_session_count: usize, parent_session_id: Option<&str>) -> String {
    let mut parts = Vec::new();
    if child_session_count > 0 {
        let child_label = if child_session_count == 1 {
            "1 child".to_string()
        } else {
            format!("{child_session_count} children")
        };
        parts.push(child_label);
    }
    if let Some(parent_session_id) = parent_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("parent {parent_session_id}"));
    }

    if parts.is_empty() {
        "root session".to_string()
    } else {
        parts.join(" · ")
    }
}

pub(crate) fn session_history_artifact_label(entry: &SessionHistoryEntry) -> String {
    artifact_count_label(entry.catalog.artifact_count)
}

pub(crate) fn session_history_lineage_label(entry: &SessionHistoryEntry) -> String {
    lineage_label(
        entry.catalog.child_session_count,
        entry.catalog.parent_session_id.as_deref(),
    )
}

fn session_history_entry_matches_action(
    entry: &SessionHistoryEntry,
    action: StartupLauncherAction,
) -> bool {
    match action {
        StartupLauncherAction::ContinueSession => matches!(
            entry.catalog.mode_source,
            SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock
        ),
        StartupLauncherAction::ReplaySession | StartupLauncherAction::NewSession => !matches!(
            entry.catalog.mode_source,
            SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
        ),
    }
}

const fn session_history_action_sort_bucket(
    entry: &SessionHistoryEntry,
    action: StartupLauncherAction,
) -> u8 {
    match action {
        StartupLauncherAction::ContinueSession if !entry.catalog.is_resumable => 1,
        _ => 0,
    }
}

fn format_session_history_timestamp(timestamp: &str) -> String {
    let trimmed = timestamp.trim();
    if trimmed.len() >= 16 && trimmed.as_bytes().get(10) == Some(&b'T') {
        format!("updated {}", trimmed[..16].replace('T', " "))
    } else if trimmed.is_empty() {
        "updated <unavailable>".to_string()
    } else {
        format!("updated {trimmed}")
    }
}

fn session_history_filter_matches(entry: &SessionHistoryEntry, input: &str) -> bool {
    if input.is_empty() {
        return true;
    }

    let candidates = [
        session_history_run_name(entry).to_lowercase(),
        entry.catalog.run_id.to_lowercase(),
        session_history_status_label(entry).to_string(),
        session_history_recency_label(entry).to_lowercase(),
        session_history_profile_label(entry).to_lowercase(),
        session_history_provider_model_label(entry).to_lowercase(),
        session_history_resumability_label(entry).to_lowercase(),
        session_history_artifact_label(entry).to_lowercase(),
        session_history_lineage_label(entry).to_lowercase(),
    ];

    candidates.iter().any(|candidate| candidate.contains(input))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchMetadata {
    profile: Option<String>,
    profile_description: Option<String>,
    provider: Option<String>,
    provider_display_label: Option<String>,
    provider_backend_label: Option<String>,
    model: Option<String>,
    model_display_label: Option<String>,
    variant: Option<String>,
    variant_display_label: Option<String>,
    display_label: Option<String>,
    token_window_label: Option<String>,
    context_window_tokens: Option<u32>,
    max_input_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    description: Option<String>,
    reasoning_effort: Option<String>,
    text_verbosity: Option<String>,
    recommended_for: Option<String>,
    mode_label: Option<String>,
    available_models: Vec<ModelOption>,
}

impl LaunchMetadata {
    pub fn new(
        profile: impl Into<String>,
        provider: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        let profile = profile.into();
        let provider = provider.into();
        let model = model.filter(|value| !value.trim().is_empty());
        let mut metadata = Self {
            profile: Some(profile.clone()),
            profile_description: None,
            provider: Some(provider.clone()),
            provider_display_label: None,
            provider_backend_label: None,
            model,
            model_display_label: None,
            variant: None,
            variant_display_label: None,
            display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
            mode_label: None,
            available_models: Vec::new(),
        };
        metadata.apply_registered_metadata();
        metadata
    }

    pub fn from_model_ref(profile: impl Into<String>, model_ref: &str) -> Self {
        let profile = profile.into();
        let model_ref = AgentModelRef::parse(model_ref);
        Self::new(profile, model_ref.provider_id, Some(model_ref.model_id))
    }

    pub fn from_model_option(option: &ModelOption) -> Self {
        Self {
            profile: Some(option.profile.clone()),
            profile_description: option.profile_description.clone(),
            provider: Some(option.provider.clone()),
            provider_display_label: option.provider_display_label.clone(),
            provider_backend_label: option.provider_backend_label.clone(),
            model: Some(option.model.clone()),
            model_display_label: option.model_display_label.clone(),
            variant: option.variant.clone(),
            variant_display_label: option.variant_display_label.clone(),
            display_label: option.display_label.clone(),
            token_window_label: option.token_window_label.clone(),
            context_window_tokens: option.context_window_tokens,
            max_input_tokens: option.max_input_tokens,
            max_output_tokens: option.max_output_tokens,
            description: option.description.clone(),
            reasoning_effort: option.reasoning_effort.clone(),
            text_verbosity: option.text_verbosity.clone(),
            recommended_for: option.recommended_for.clone(),
            mode_label: None,
            available_models: Vec::new(),
        }
    }

    pub fn with_mode_label(mut self, mode_label: impl Into<String>) -> Self {
        self.mode_label = Some(mode_label.into());
        self
    }

    pub fn without_mode_label(mut self) -> Self {
        self.mode_label = None;
        self
    }

    pub fn with_available_models(mut self, available_models: Vec<ModelOption>) -> Self {
        self.available_models = available_models;
        self
    }

    pub fn profile(&self) -> &str {
        self.profile
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("default")
    }

    pub(super) fn configured_profile(&self) -> Option<&str> {
        self.profile
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn provider(&self) -> &str {
        self.provider
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("local")
    }

    pub fn profile_description(&self) -> Option<&str> {
        self.profile_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn provider_display_label(&self) -> Option<&str> {
        self.provider_display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.provider_display_label())
            })
    }

    pub fn provider_backend_label(&self) -> Option<&str> {
        self.provider_backend_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.provider_backend_label())
            })
    }

    pub fn model_display_label(&self) -> Option<&str> {
        self.model_display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.model_display_label())
            })
    }

    pub fn model(&self) -> Option<&str> {
        self.model
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn variant(&self) -> Option<&str> {
        self.variant
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.variant())
            })
    }

    pub fn variant_display_label(&self) -> Option<&str> {
        self.variant_display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.variant_display_label())
            })
    }

    pub fn display_label(&self) -> Option<&str> {
        self.display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.display_label())
            })
    }

    pub fn token_window_label(&self) -> Option<&str> {
        self.token_window_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.token_window_label())
            })
    }

    pub fn context_window_tokens(&self) -> Option<u32> {
        self.context_window_tokens.or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.context_window_tokens)
        })
    }

    pub fn max_input_tokens(&self) -> Option<u32> {
        self.max_input_tokens.or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.max_input_tokens)
        })
    }

    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens.or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.max_output_tokens)
        })
    }

    pub fn description(&self) -> Option<&str> {
        self.description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.description())
            })
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.reasoning_effort())
            })
    }

    pub fn text_verbosity(&self) -> Option<&str> {
        self.text_verbosity
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.text_verbosity())
            })
    }

    pub fn recommended_for(&self) -> Option<&str> {
        self.recommended_for
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.matching_available_model()
                    .and_then(|option| option.recommended_for())
            })
    }

    pub fn mode_label(&self) -> Option<&str> {
        self.mode_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn available_models(&self) -> &[ModelOption] {
        &self.available_models
    }

    pub(super) fn to_model_option(&self) -> Option<ModelOption> {
        Some(ModelOption {
            profile: self.profile().to_string(),
            provider: self.provider().to_string(),
            provider_display_label: self.provider_display_label().map(str::to_string),
            provider_backend_label: self.provider_backend_label().map(str::to_string),
            model: self.model()?.to_string(),
            model_display_label: self.model_display_label().map(str::to_string),
            variant: self.variant().map(str::to_string),
            variant_display_label: self.variant_display_label().map(str::to_string),
            display_label: self.display_label().map(str::to_string),
            token_window_label: self.token_window_label().map(str::to_string),
            context_window_tokens: self.context_window_tokens(),
            max_input_tokens: self.max_input_tokens(),
            max_output_tokens: self.max_output_tokens(),
            description: self.description().map(str::to_string),
            profile_description: self.profile_description().map(str::to_string),
            reasoning_effort: self.reasoning_effort().map(str::to_string),
            text_verbosity: self.text_verbosity().map(str::to_string),
            recommended_for: self.recommended_for().map(str::to_string),
        })
    }

    fn apply_registered_metadata(&mut self) {
        let profile = self.profile();
        let provider = self.provider();
        let model = self.model();
        let Some(metadata) = metadata_for_profile_identity(profile, provider, model) else {
            return;
        };
        self.apply_resolved_metadata(&metadata);
    }

    fn apply_resolved_metadata(&mut self, metadata: &ResolvedProfileModelMetadata) {
        self.profile_description = metadata.profile_description.clone();
        self.variant = metadata.variant.clone();
        self.provider_display_label = Some(metadata.provider_display_label.clone());
        self.provider_backend_label = metadata.provider_backend_label.clone();
        self.model_display_label = Some(metadata.model_display_label.clone());
        self.variant_display_label = metadata.variant_display_label.clone();
        self.display_label = Some(metadata.display_label.clone());
        self.token_window_label = metadata.token_window_label.clone();
        self.context_window_tokens = metadata.context_window_tokens;
        self.max_input_tokens = metadata.max_input_tokens;
        self.max_output_tokens = metadata.max_output_tokens;
        self.description = metadata.description.clone();
        self.reasoning_effort = metadata.reasoning_effort.clone();
        self.text_verbosity = metadata.text_verbosity.clone();
        self.recommended_for = metadata.recommended_for.clone();
    }

    fn matching_available_model(&self) -> Option<&ModelOption> {
        let profile = self.profile();
        let provider = self.provider();
        let model = self.model();
        let variant = self
            .variant
            .as_deref()
            .filter(|value| !value.trim().is_empty());

        let mut exact_profile_matches = self.available_models.iter().filter(|option| {
            option.profile == profile
                && option.provider == provider
                && model.is_some_and(|model_id| option.model == model_id)
                && option.variant() == variant
        });
        if let Some(first) = exact_profile_matches.next() {
            return Some(first);
        }

        let mut exact_variant_matches = self.available_models.iter().filter(|option| {
            option.provider == provider
                && model.is_some_and(|model_id| option.model == model_id)
                && option.variant() == variant
        });
        if let Some(first) = exact_variant_matches.next() {
            if exact_variant_matches.next().is_none() {
                return Some(first);
            }
        }

        let mut profile_matches = self.available_models.iter().filter(|option| {
            option.profile == profile
                && option.provider == provider
                && model.is_some_and(|model_id| option.model == model_id)
        });
        if let Some(first) = profile_matches.next() {
            if profile_matches.next().is_none() {
                return Some(first);
            }
        }

        let mut matches = self.available_models.iter().filter(|option| {
            option.provider == provider && model.is_some_and(|model_id| option.model == model_id)
        });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
}

#[derive(Debug, Clone)]
pub struct ModelOption {
    pub profile: String,
    pub provider: String,
    pub provider_display_label: Option<String>,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: Option<String>,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
    pub display_label: Option<String>,
    pub token_window_label: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub profile_description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
}

impl PartialEq for ModelOption {
    fn eq(&self, other: &Self) -> bool {
        self.profile == other.profile
            && self.provider == other.provider
            && self.model == other.model
            && self.variant == other.variant
    }
}

impl Eq for ModelOption {}

impl ModelOption {
    pub fn from_model_ref(profile: impl Into<String>, model_ref: &str) -> Self {
        let profile = profile.into();
        let model_ref = AgentModelRef::parse(model_ref);
        let mut option = Self {
            profile,
            provider: model_ref.provider_id,
            provider_display_label: None,
            provider_backend_label: None,
            model: model_ref.model_id,
            model_display_label: None,
            variant: None,
            variant_display_label: None,
            display_label: None,
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        };
        option.apply_registered_metadata();
        option
    }

    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = profile.into();
        self
    }

    fn matches(&self, input: &str) -> bool {
        if input.is_empty() {
            return true;
        }

        let input = input.to_lowercase();
        self.profile.to_lowercase().contains(&input)
            || self.provider.to_lowercase().contains(&input)
            || self
                .provider_display_label()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self.model.to_lowercase().contains(&input)
            || self
                .model_display_label()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .variant()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .display_label()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .token_window_label()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .description()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .reasoning_effort()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .text_verbosity()
                .is_some_and(|value| value.to_lowercase().contains(&input))
            || self
                .recommended_for()
                .is_some_and(|value| value.to_lowercase().contains(&input))
    }

    pub fn variant(&self) -> Option<&str> {
        self.variant
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn provider_display_label(&self) -> Option<&str> {
        self.provider_display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn provider_backend_label(&self) -> Option<&str> {
        self.provider_backend_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn model_display_label(&self) -> Option<&str> {
        self.model_display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn variant_display_label(&self) -> Option<&str> {
        self.variant_display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn display_label(&self) -> Option<&str> {
        self.display_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn token_window_label(&self) -> Option<&str> {
        self.token_window_label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn description(&self) -> Option<&str> {
        self.description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn profile_description(&self) -> Option<&str> {
        self.profile_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn text_verbosity(&self) -> Option<&str> {
        self.text_verbosity
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    pub fn recommended_for(&self) -> Option<&str> {
        self.recommended_for
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }

    fn apply_registered_metadata(&mut self) {
        let Some(metadata) = metadata_for_profile_identity(
            self.profile.as_str(),
            self.provider.as_str(),
            Some(self.model.as_str()),
        ) else {
            return;
        };
        self.variant = metadata.variant;
        self.provider_display_label = Some(metadata.provider_display_label);
        self.provider_backend_label = metadata.provider_backend_label;
        self.model_display_label = Some(metadata.model_display_label);
        self.variant_display_label = metadata.variant_display_label;
        self.display_label = Some(metadata.display_label);
        self.token_window_label = metadata.token_window_label;
        self.context_window_tokens = metadata.context_window_tokens;
        self.max_input_tokens = metadata.max_input_tokens;
        self.max_output_tokens = metadata.max_output_tokens;
        self.description = metadata.description;
        self.profile_description = metadata.profile_description;
        self.reasoning_effort = metadata.reasoning_effort;
        self.text_verbosity = metadata.text_verbosity;
        self.recommended_for = metadata.recommended_for;
    }
}

impl PartialOrd for ModelOption {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModelOption {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.provider
            .cmp(&other.provider)
            .then_with(|| self.model.cmp(&other.model))
            .then_with(|| self.variant.cmp(&other.variant))
            .then_with(|| self.profile.cmp(&other.profile))
    }
}

fn metadata_for_profile_identity(
    profile: &str,
    provider: &str,
    model: Option<&str>,
) -> Option<ResolvedProfileModelMetadata> {
    let metadata = registered_profile_model_metadata(profile)?;
    if metadata.provider != provider {
        return None;
    }
    if let Some(model_id) = model {
        if metadata.model != model_id {
            return None;
        }
    }
    Some(metadata)
}

fn runtime_identity_for_metadata(metadata: &LaunchMetadata) -> String {
    let model_label = metadata
        .display_label()
        .or_else(|| metadata.model())
        .unwrap_or("-");
    format!("{} · {model_label}", metadata.profile())
}

impl AppState {
    pub(in crate::app) fn post_run_can_reopen(&self) -> bool {
        self.post_run_reopen_target().is_some()
    }

    fn post_run_reopen_target(&self) -> Option<(&str, &PathBuf)> {
        let run_id = self.run_id().filter(|run_id| !run_id.trim().is_empty())?;
        let session_path = self.session_path.as_ref()?;
        Some((run_id, session_path))
    }

    fn default_post_run_handoff_action(&self) -> PostRunHandoffAction {
        if self.post_run_can_reopen() {
            PostRunHandoffAction::ContinueSession
        } else {
            PostRunHandoffAction::StartAnotherSession
        }
    }

    pub(crate) fn selected_post_run_handoff_action(&self) -> PostRunHandoffAction {
        let selected = self.post_run_handoff_action;
        if self.post_run_handoff_actions().contains(&selected) {
            selected
        } else {
            self.default_post_run_handoff_action()
        }
    }

    fn reset_post_run_handoff_selection(&mut self) {
        self.post_run_handoff_action = self.default_post_run_handoff_action();
    }

    pub fn active_profile(&self) -> &str {
        let profile = self.launch_metadata.profile();
        if Self::launch_value_is_unknown(profile) {
            "default"
        } else {
            profile
        }
    }

    pub fn active_provider(&self) -> &str {
        let provider = self.launch_metadata.provider();
        if !Self::launch_value_is_unknown(provider) && provider != "local" {
            provider
        } else {
            self.activities
                .back()
                .and_then(|activity| {
                    (!activity.provider_id.trim().is_empty())
                        .then_some(activity.provider_id.as_str())
                })
                .filter(|value| !Self::launch_value_is_unknown(value))
                .unwrap_or("local")
        }
    }

    fn current_model_id(&self) -> &str {
        self.launch_metadata
            .model()
            .or_else(|| {
                self.activities.back().and_then(|activity| {
                    (!activity.model_id.trim().is_empty()).then_some(activity.model_id.as_str())
                })
            })
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or("-")
    }

    pub(crate) fn current_model_variant(&self) -> Option<&str> {
        self.launch_metadata.variant()
    }

    pub fn current_model_label(&self) -> &str {
        self.launch_metadata
            .display_label()
            .unwrap_or_else(|| self.current_model_id())
    }

    pub fn current_model_base_label(&self) -> &str {
        self.launch_metadata
            .model_display_label()
            .or_else(|| self.launch_metadata.model())
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or_else(|| self.current_model_id())
    }

    pub fn current_model_reasoning_label(&self) -> Option<&str> {
        self.launch_metadata
            .reasoning_effort()
            .or_else(|| self.launch_metadata.variant_display_label())
            .or_else(|| self.launch_metadata.variant())
            .or_else(|| self.launch_metadata.mode_label())
            .filter(|value| !Self::launch_value_is_unknown(value))
    }

    pub(crate) fn current_context_window_tokens(&self) -> Option<u32> {
        self.launch_metadata.context_window_tokens().or_else(|| {
            self.activities
                .back()
                .and_then(|activity| {
                    (!activity.model_id.trim().is_empty()).then(|| {
                        LaunchMetadata::new(
                            self.active_profile(),
                            self.active_provider(),
                            Some(activity.model_id.clone()),
                        )
                        .context_window_tokens()
                    })
                })
                .flatten()
        })
    }

    pub fn current_source_label(&self) -> Option<String> {
        let provider = self
            .launch_metadata
            .provider_display_label()
            .or_else(|| {
                let provider = self.launch_metadata.provider();
                (!Self::launch_value_is_unknown(provider) && provider != "local")
                    .then_some(provider)
            })
            .or_else(|| {
                let provider = self.active_provider();
                (!Self::launch_value_is_unknown(provider) && provider != "local")
                    .then_some(provider)
            })?;
        let backend = self
            .launch_metadata
            .provider_backend_label()
            .filter(|value| !Self::launch_value_is_unknown(value));
        Some(match backend {
            Some(backend) => format!("{provider} ({backend})"),
            None => provider.to_string(),
        })
    }

    pub fn current_agent_label(&self) -> Option<String> {
        let profile = self
            .launch_metadata
            .configured_profile()
            .or_else(|| {
                let profile = self.active_profile();
                (!Self::launch_value_is_unknown(profile) && profile != "default").then_some(profile)
            })
            .filter(|value| !Self::launch_value_is_unknown(value))?;
        Some(humanize_profile_label(profile))
    }

    pub(in crate::app) fn runtime_context_metadata(&self) -> &LaunchMetadata {
        self.runtime_context_metadata
            .as_ref()
            .unwrap_or(&self.launch_metadata)
    }

    pub(in crate::app) fn runtime_context_profile(&self) -> &str {
        let profile = self.runtime_context_metadata().profile();
        if Self::launch_value_is_unknown(profile) {
            self.active_profile()
        } else {
            profile
        }
    }

    pub(in crate::app) fn runtime_context_provider(&self) -> &str {
        let provider = self.runtime_context_metadata().provider();
        if Self::launch_value_is_unknown(provider) || provider == "local" {
            self.active_provider()
        } else {
            provider
        }
    }

    pub(in crate::app) fn runtime_context_model_label(&self) -> String {
        self.runtime_context_metadata()
            .display_label()
            .or_else(|| self.runtime_context_metadata().model())
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or_else(|| self.current_model_label())
            .to_string()
    }

    pub(in crate::app) fn runtime_context_identity(&self) -> String {
        format!(
            "{} · {}",
            self.runtime_context_profile(),
            self.runtime_context_model_label()
        )
    }

    pub(in crate::app) fn runtime_context_label(&self) -> crate::view_model::RuntimeContextLabel {
        if self.startup_shell_visible() {
            crate::view_model::RuntimeContextLabel::Launch
        } else if self.replay_mode {
            crate::view_model::RuntimeContextLabel::RecordedRuntimeReadOnly
        } else if self.continued_live_run() {
            crate::view_model::RuntimeContextLabel::ContinuedRuntime
        } else {
            crate::view_model::RuntimeContextLabel::CurrentRuntime
        }
    }

    pub(in crate::app) fn runtime_provider_context(&self) -> Option<String> {
        let provider = self.runtime_context_provider().trim();
        (!provider.is_empty()).then(|| provider.to_string())
    }

    pub(in crate::app) fn next_turn_identity(&self) -> Option<String> {
        if self.startup_shell_visible() || self.replay_mode {
            return None;
        }

        let current = self.runtime_context_metadata();
        let next = &self.launch_metadata;
        let changed = current.profile() != next.profile()
            || current.provider() != next.provider()
            || current.model() != next.model()
            || current.variant() != next.variant();
        changed.then(|| runtime_identity_for_metadata(next))
    }

    pub(in crate::app) fn handle_navigation_overlay_key(&mut self, key: &KeyEvent) -> bool {
        if self.session_history_visible {
            return self.handle_session_history_key(key);
        }

        if self.model_switcher_visible {
            return self.handle_model_key(key);
        }

        if self.palette_visible {
            return self.handle_palette_key(key);
        }

        self.slash_overlay_should_render() && self.handle_slash_key(key)
    }

    fn active_slash_query(&self) -> Option<&str> {
        let query = self.prompt_buffer.strip_prefix('/')?;
        (!query.chars().any(char::is_whitespace)).then_some(query)
    }

    pub(in crate::app) fn clear_slash_menu(&mut self) {
        self.slash_visible = false;
        self.slash_filtered.clear();
        self.slash_selected = 0;
    }

    pub(in crate::app) fn slash_overlay_should_render(&self) -> bool {
        false
    }

    pub(in crate::app) fn sync_slash_overlay(&mut self) {
        if self.focus != Focus::Prompt
            || self.composer_disabled()
            || self.active_slash_query().is_none()
            || self.palette_visible
            || self.session_history_visible
            || self.model_switcher_visible
            || self.active_permission().is_some()
        {
            if !self.prompt_buffer.starts_with('/') {
                self.slash_draft_snapshot = None;
            }
            self.clear_slash_menu();
            return;
        }

        let slash_query = self.active_slash_query().unwrap_or_default().to_lowercase();

        self.slash_visible = true;
        self.slash_filtered = SLASH_COMMANDS
            .iter()
            .filter(|(command, _)| self.slash_command_available(command))
            .filter(|(command, description)| {
                slash_query.is_empty()
                    || command.starts_with(&slash_query)
                    || description.to_lowercase().contains(&slash_query)
            })
            .map(|(command, _)| (*command).to_string())
            .collect();
        self.slash_selected = self
            .slash_selected
            .min(self.slash_filtered.len().saturating_sub(1));
    }

    pub(in crate::app) fn typed_slash_command(&self) -> Option<&'static str> {
        self.prompt_buffer
            .trim()
            .strip_prefix('/')
            .and_then(|command| {
                SLASH_COMMANDS.iter().find_map(|(name, _)| {
                    (*name == command && self.slash_command_available(name)).then_some(*name)
                })
            })
    }

    fn slash_command_available(&self, command: &str) -> bool {
        match command {
            "new" | "exit" => true,
            "resume" | "replay" | "model" => !self.replay_mode,
            "events" => !self.startup_mode,
            "shell" => self.active_review_surface.is_some(),
            "follow" => !self.replay_mode && !self.startup_mode,
            _ => false,
        }
    }

    fn restore_slash_draft(&mut self, preserved_draft: Option<String>) {
        self.replace_prompt_input(preserved_draft.unwrap_or_default());
    }

    fn navigate_to_home_shell(&mut self, draft: String) {
        self.projection.reset();
        self.selected_event_index = 0;
        self.selected_activity_index = 0;
        self.follow_mode = true;
        self.active_tab = Tab::Run;
        self.live_details_drawer_open = false;
        self.startup_mode = true;
        self.startup_launcher_action = StartupLauncherAction::NewSession;
        self.status_banner = None;
        self.details_scroll = 0;
        self.transcript_scroll = 0;
        self.prompt_history.clear();
        self.prompt_history_index = None;
        self.replay_mode = false;
        self.session_path = None;
        self.palette_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.palette_selected = 0;
        self.palette_focus_return = None;
        self.session_history_visible = false;
        self.session_history_selected = 0;
        self.model_switcher_visible = false;
        self.model_filtered.clear();
        self.model_selected = 0;
        self.continued_post_run_handoff_active = false;
        self.continued_live_reopen_surface_active = false;
        self.continue_disabled_banner = None;
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.permission_modal_permission_id = None;
        self.permission_modal_stage = PermissionModalStage::Decision;
        self.permission_modal_selection = PermissionModalSelection::AllowOnce;
        self.permission_modal_confirm_selection = PermissionConfirmSelection::Confirm;
        self.question_prompt_tab = 0;
        self.question_prompt_selection = 0;
        self.question_prompt_answers.clear();
        self.question_prompt_custom.clear();
        self.question_prompt_editing = false;
        self.reload_requested = false;
        self.should_quit = false;
        self.focus = Focus::Prompt;
        self.replace_prompt_input(draft);
    }

    pub(in crate::app) fn execute_slash_command(
        &mut self,
        command: &str,
        preserved_draft: Option<String>,
    ) {
        self.clear_slash_menu();
        match command {
            "new" => self.navigate_to_home_shell(preserved_draft.unwrap_or_default()),
            "resume" => {
                self.restore_slash_draft(preserved_draft);
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            "replay" => {
                self.restore_slash_draft(preserved_draft);
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            "model" => {
                self.restore_slash_draft(preserved_draft);
                self.open_model_switcher();
            }
            "events" => {
                self.restore_slash_draft(preserved_draft);
                self.open_review_surface(ReviewSurface::Events);
            }
            "shell" => {
                self.restore_slash_draft(preserved_draft);
                self.close_review_surface();
            }
            "follow" => {
                self.restore_slash_draft(preserved_draft);
                self.execute_action(Action::ToggleFollow);
            }
            "exit" => self.execute_action(Action::Quit),
            _ => {}
        }
    }

    pub(in crate::app) fn apply_selected_slash_completion(&mut self) {
        let Some(command) = self.slash_filtered.get(self.slash_selected).cloned() else {
            return;
        };
        self.execute_slash_command(&command, self.slash_draft_snapshot.clone());
    }

    pub(in crate::app) fn handle_model_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_model();
                true
            }
            KeyCode::Up => {
                if self.model_selected > 0 {
                    self.model_selected -= 1;
                }
                true
            }
            KeyCode::Down => {
                if self.model_selected + 1 < self.model_filtered.len() {
                    self.model_selected += 1;
                }
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_model_filter);
                true
            }
            KeyCode::Char(c) => {
                self.overlay_insert_char(c, Self::update_model_filter);
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn handle_slash_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.clear_slash_menu();
                true
            }
            KeyCode::Enter | KeyCode::Tab => {
                self.apply_selected_slash_completion();
                true
            }
            KeyCode::Up => {
                if self.slash_selected > 0 {
                    self.slash_selected -= 1;
                }
                true
            }
            KeyCode::Down => {
                if self.slash_selected + 1 < self.slash_filtered.len() {
                    self.slash_selected += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn handle_palette_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_palette_command();
                true
            }
            KeyCode::PageUp => {
                self.move_palette_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.move_palette_selection(10);
                true
            }
            KeyCode::Home => {
                self.palette_selected = 0;
                true
            }
            KeyCode::End => {
                self.palette_selected = self.palette_filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.move_palette_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_palette_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_palette_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_palette_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_palette_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_palette_selection(1);
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_palette_filter);
                true
            }
            _ => false,
        }
    }

    fn update_palette_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let filtered = self
            .palette_commands()
            .iter()
            .enumerate()
            .filter_map(|palette_command| {
                let (index, palette_command) = palette_command;
                let label = palette_command.label.to_lowercase();
                let id = palette_command.id.to_lowercase();
                let description = palette_command.description.to_lowercase();
                let section = palette_command.section.label().to_lowercase();
                let prefix_match = input.is_empty()
                    || label.starts_with(&input)
                    || id.starts_with(&input)
                    || section.starts_with(&input);
                let contains_match = prefix_match
                    || label.contains(&input)
                    || id.contains(&input)
                    || description.contains(&input)
                    || section.contains(&input);
                contains_match.then_some((
                    prefix_match,
                    palette_command.section,
                    index,
                    palette_command.id.to_string(),
                ))
            })
            .collect::<Vec<_>>();
        let has_prefix_matches = filtered.iter().any(|(prefix_match, _, _, _)| *prefix_match);
        let mut filtered = filtered
            .into_iter()
            .filter(|(prefix_match, _, _, _)| !has_prefix_matches || *prefix_match)
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| left.2.cmp(&right.2))
                .then_with(|| left.3.cmp(&right.3))
        });
        self.palette_filtered = filtered
            .into_iter()
            .map(|(_, _, _, command)| command)
            .collect();
        self.palette_selected = 0;
    }

    fn move_palette_selection(&mut self, delta: isize) {
        let len = self.palette_filtered.len();
        if len == 0 {
            self.palette_selected = 0;
            return;
        }

        if delta == -1 {
            self.palette_selected = if self.palette_selected == 0 {
                len - 1
            } else {
                self.palette_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.palette_selected = (self.palette_selected + 1) % len;
            return;
        }

        let current = self.palette_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        self.palette_selected = usize::try_from(next).unwrap_or(0);
    }

    fn execute_palette_command(&mut self) {
        let Some(cmd) = self.palette_filtered.get(self.palette_selected) else {
            self.close_palette();
            return;
        };

        match cmd.as_str() {
            "new_session" => {
                self.startup_launcher_action = StartupLauncherAction::NewSession;
                self.apply_new_session_launcher_selection();
            }
            "resume_session" => {
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            "replay_session" => {
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            "switch_model" => {
                self.open_model_switcher();
            }
            "cycle_variant" => self.execute_action(Action::VariantCycle),
            "close_review_surface" => self.execute_action(Action::CloseReviewSurface),
            "open_event_log" => self.execute_action(Action::OpenEventLog),
            "toggle_follow" => self.execute_action(Action::ToggleFollow),
            "show_thinking" => self.show_transcript_thinking = true,
            "hide_thinking" => self.show_transcript_thinking = false,
            "show_timestamps" => self.show_transcript_timestamps = true,
            "hide_timestamps" => self.show_transcript_timestamps = false,
            "show_tool_details" => self.show_tool_details = true,
            "hide_tool_details" => self.show_tool_details = false,
            "show_generic_tool_output" => self.show_generic_tool_output = true,
            "hide_generic_tool_output" => self.show_generic_tool_output = false,
            "expand_selected_turn_results" => self.set_selected_activity_expandable_outputs(true),
            "collapse_selected_turn_results" => {
                self.set_selected_activity_expandable_outputs(false)
            }
            "stack_transcript_diffs" => self.stacked_transcript_diffs = true,
            "split_transcript_diffs" => self.stacked_transcript_diffs = false,
            "quit" => self.execute_action(Action::Quit),
            _ => {}
        }
        if !self.session_history_visible && !self.model_switcher_visible {
            self.close_palette();
        }
    }

    pub(in crate::app) fn close_palette(&mut self) {
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.session_history_filtered.clear();
        self.model_filtered.clear();
        self.palette_selected = 0;
        self.session_history_selected = 0;
        self.model_selected = 0;
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
        self.sync_slash_overlay();
    }

    pub(in crate::app) fn open_palette(&mut self) {
        if !self.palette_visible {
            self.palette_focus_return = Some(self.focus);
        }
        self.palette_visible = true;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered = self
            .palette_commands()
            .iter()
            .map(|palette_command| palette_command.id.to_string())
            .collect();
        self.session_history_filtered.clear();
        self.model_filtered.clear();
        self.palette_selected = 0;
        self.sync_slash_overlay();
    }

    fn palette_commands(&self) -> Vec<crate::keybindings::PaletteCommand> {
        Action::grouped_palette_commands_for_overlay()
            .iter()
            .copied()
            .filter(|command| self.palette_command_available(command.id))
            .collect()
    }

    fn palette_command_available(&self, command_id: &str) -> bool {
        if command_id == "switch_model" {
            return !self.replay_mode;
        }

        if command_id == "cycle_variant" {
            return !self.replay_mode;
        }

        if self.startup_shell_visible() {
            matches!(
                command_id,
                "new_session" | "resume_session" | "replay_session" | "quit"
            )
        } else if matches!(command_id, "show_timestamps" | "hide_timestamps") {
            self.active_review_surface.is_none()
                && if command_id == "show_timestamps" {
                    !self.show_transcript_timestamps
                } else {
                    self.show_transcript_timestamps
                }
        } else if matches!(command_id, "show_thinking" | "hide_thinking") {
            self.active_review_surface.is_none()
                && if command_id == "show_thinking" {
                    !self.show_transcript_thinking
                } else {
                    self.show_transcript_thinking
                }
        } else if matches!(command_id, "show_tool_details" | "hide_tool_details") {
            self.active_review_surface.is_none()
                && if command_id == "show_tool_details" {
                    !self.show_tool_details
                } else {
                    self.show_tool_details
                }
        } else if matches!(
            command_id,
            "show_generic_tool_output" | "hide_generic_tool_output"
        ) {
            self.active_review_surface.is_none()
                && if command_id == "show_generic_tool_output" {
                    !self.show_generic_tool_output
                } else {
                    self.show_generic_tool_output
                }
        } else if matches!(
            command_id,
            "expand_selected_turn_results" | "collapse_selected_turn_results"
        ) {
            let expandable_ids = self.selected_activity_expandable_tool_ids();
            self.active_review_surface.is_none()
                && !expandable_ids.is_empty()
                && if command_id == "expand_selected_turn_results" {
                    expandable_ids
                        .iter()
                        .any(|tool_call_id| !self.expanded_tool_outputs.contains(tool_call_id))
                } else {
                    expandable_ids
                        .iter()
                        .any(|tool_call_id| self.expanded_tool_outputs.contains(tool_call_id))
                }
        } else if matches!(
            command_id,
            "stack_transcript_diffs" | "split_transcript_diffs"
        ) {
            self.active_review_surface.is_none()
                && if command_id == "stack_transcript_diffs" {
                    !self.stacked_transcript_diffs
                } else {
                    self.stacked_transcript_diffs
                }
        } else if command_id == "close_review_surface" {
            self.active_review_surface.is_some()
        } else if command_id == "open_event_log" {
            self.active_review_surface != Some(ReviewSurface::Events)
        } else {
            true
        }
    }

    pub(in crate::app) fn apply_new_session_launcher_selection(&mut self) {
        let lifecycle_exit = self.startup_mode
            || self.post_run_handoff_visible()
            || self.completed_session_shell_active();
        let prompt_buffer = self.prompt_buffer.clone();
        let prompt_cursor = self.prompt_cursor;
        set_pending_live_prompt_draft(Some(prompt_buffer.clone()));
        set_pending_live_launch_metadata(self.launch_metadata.clone());

        self.projection.reset();
        self.selected_event_index = 0;
        self.selected_activity_index = 0;
        self.follow_mode = true;
        self.details_scroll = 0;
        self.transcript_scroll = 0;
        self.status_banner = None;
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.permission_modal_permission_id = None;
        self.permission_modal_stage = PermissionModalStage::Decision;
        self.permission_modal_selection = PermissionModalSelection::AllowOnce;
        self.permission_modal_confirm_selection = PermissionConfirmSelection::Confirm;
        self.question_prompt_tab = 0;
        self.question_prompt_selection = 0;
        self.question_prompt_answers.clear();
        self.question_prompt_custom.clear();
        self.question_prompt_editing = false;
        self.prompt_history.clear();
        self.prompt_history_index = None;
        self.replay_mode = false;
        self.session_path = None;
        self.continued_post_run_handoff_active = false;
        self.continued_live_reopen_surface_active = false;
        self.active_tab = Tab::Run;
        self.live_details_drawer_open = false;
        self.continue_disabled_banner = None;

        self.prompt_buffer = prompt_buffer;
        self.prompt_cursor = prompt_cursor.min(self.prompt_buffer.chars().count());

        self.close_session_history();
        self.emit_ui_intent(UiIntent::NewSession);
        if lifecycle_exit {
            self.should_quit = true;
        }
    }

    pub(in crate::app) fn select_previous_startup_launcher_action(&mut self) {
        self.startup_launcher_action = self.startup_launcher_action.previous();
        self.continue_disabled_banner = None;
    }

    pub(in crate::app) fn select_next_startup_launcher_action(&mut self) {
        self.startup_launcher_action = self.startup_launcher_action.next();
        self.continue_disabled_banner = None;
    }

    pub(in crate::app) fn execute_startup_launcher_action(&mut self) {
        match self.startup_launcher_action {
            StartupLauncherAction::NewSession => self.apply_new_session_launcher_selection(),
            StartupLauncherAction::ReplaySession => {
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            StartupLauncherAction::ContinueSession => {
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
        }
    }

    pub(in crate::app) fn select_previous_post_run_handoff_action(&mut self) {
        let actions = self.post_run_handoff_actions();
        let current = self.selected_post_run_handoff_action();
        let current_index = actions
            .iter()
            .position(|action| *action == current)
            .unwrap_or(0);
        let previous_index = if current_index == 0 {
            actions.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.post_run_handoff_action = actions[previous_index];
    }

    pub(in crate::app) fn select_next_post_run_handoff_action(&mut self) {
        let actions = self.post_run_handoff_actions();
        let current = self.selected_post_run_handoff_action();
        let current_index = actions
            .iter()
            .position(|action| *action == current)
            .unwrap_or(0);
        let next_index = if current_index + 1 >= actions.len() {
            0
        } else {
            current_index + 1
        };
        self.post_run_handoff_action = actions[next_index];
    }

    pub(in crate::app) fn execute_post_run_handoff_action(&mut self) {
        match self.selected_post_run_handoff_action() {
            PostRunHandoffAction::ContinueSession => {
                if self.continued_post_run_handoff_active {
                    self.continued_post_run_handoff_active = false;
                    self.continued_live_reopen_surface_active = true;
                    self.active_tab = Tab::Run;
                    self.focus = Focus::Prompt;
                    return;
                }
                let Some((run_id, run_dir)) = self.post_run_reopen_target() else {
                    self.reset_post_run_handoff_selection();
                    return;
                };
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ContinueSession {
                    run_id: run_id.to_string(),
                    run_dir: run_dir.clone(),
                });
                self.should_quit = true;
            }
            PostRunHandoffAction::ReplayRun => {
                let Some((run_id, run_dir)) = self.post_run_reopen_target() else {
                    self.reset_post_run_handoff_selection();
                    return;
                };
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ReplaySession {
                    run_id: run_id.to_string(),
                    run_dir: run_dir.clone(),
                });
                self.should_quit = true;
            }
            PostRunHandoffAction::StartAnotherSession => {
                self.apply_new_session_launcher_selection();
            }
            PostRunHandoffAction::Quit => {
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
            }
        }
    }

    pub fn set_launch_metadata(&mut self, launch_metadata: LaunchMetadata) {
        let refresh_runtime_context = self.startup_mode
            || self.replay_mode
            || self.runtime_context_metadata.is_none()
            || (self.events.is_empty() && self.activities.is_empty());
        self.launch_metadata = launch_metadata.clone();
        if refresh_runtime_context {
            self.runtime_context_metadata = Some(launch_metadata);
        }
    }

    fn current_session_id(&self) -> Option<&str> {
        self.session_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(super) fn child_session_ids(&self) -> Vec<String> {
        let mut child_session_ids = BTreeSet::new();

        for activity in &self.activities {
            for tool_call in &activity.tool_calls {
                let child_session_id = tool_call
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        json_string_field(
                            tool_call.output_json.as_ref(),
                            &["child_session_id", "session_id"],
                        )
                    });

                if let Some(child_session_id) = child_session_id {
                    child_session_ids.insert(child_session_id);
                }
            }
        }

        child_session_ids.into_iter().collect()
    }

    pub(super) fn current_parent_session_id(&self) -> Option<String> {
        parent_session_id_from_events(&self.events)
    }

    fn build_launch_metadata_for_option(&self, selected_model: &ModelOption) -> LaunchMetadata {
        let mut launch_metadata = LaunchMetadata::from_model_option(selected_model)
            .with_available_models(self.launch_metadata.available_models().to_vec());
        if let Some(mode_label) = self.launch_metadata.mode_label().map(str::to_owned) {
            launch_metadata = launch_metadata.with_mode_label(mode_label);
        }
        launch_metadata
    }

    fn apply_selected_model_option(&mut self, selected_model: ModelOption, emit_intent: bool) {
        let launch_metadata = self.build_launch_metadata_for_option(&selected_model);
        self.launch_metadata = launch_metadata.clone();

        if emit_intent {
            set_pending_live_launch_metadata(launch_metadata.clone());
            self.emit_ui_intent(UiIntent::SwitchModel {
                profile: selected_model.profile,
                launch_metadata,
            });
        }
    }

    pub(super) fn cycle_variant(&mut self) {
        if self.replay_mode {
            return;
        }

        let profile_id = self.launch_metadata.profile().to_string();
        let Some(model_id) = self.launch_metadata.model().map(str::to_owned) else {
            return;
        };
        let provider_id = self.launch_metadata.provider().to_string();
        let mut variants = self
            .launch_metadata
            .available_models()
            .iter()
            .filter(|option| {
                option.profile == profile_id
                    && option.provider == provider_id
                    && option.model == model_id
            })
            .cloned()
            .collect::<Vec<_>>();

        let explicit_variants_exist = variants.iter().any(|option| option.variant().is_some());
        if explicit_variants_exist {
            variants.retain(|option| option.variant().is_some());
        }

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            if current_option.profile == profile_id
                && current_option.provider == provider_id
                && current_option.model == model_id
                && (!explicit_variants_exist || current_option.variant().is_some())
                && !variants.iter().any(|option| option == &current_option)
            {
                variants.push(current_option);
            }
        }

        variants.sort();
        variants.dedup();
        if variants.is_empty() {
            return;
        }

        let selected_model = match variants
            .iter()
            .position(|option| self.is_current_model_option(option))
        {
            Some(_) if variants.len() < 2 => return,
            Some(current_index) => {
                let next_index = (current_index + 1) % variants.len();
                variants[next_index].clone()
            }
            None => variants[0].clone(),
        };
        self.apply_selected_model_option(selected_model, !self.replay_mode);
    }

    fn current_session_snapshot(&self) -> Option<SessionNavigationSnapshot> {
        Some(SessionNavigationSnapshot {
            session_path: self.session_path.clone()?,
            events: self.events.clone(),
            launch_metadata: self.launch_metadata.clone(),
            child_session_ids: self.child_session_ids(),
        })
    }

    fn restore_session_snapshot(&mut self, snapshot: SessionNavigationSnapshot) {
        self.replay_mode = true;
        self.session_path = Some(snapshot.session_path);
        self.replace_events(snapshot.events);
        self.set_launch_metadata(snapshot.launch_metadata);
        self.active_review_surface = None;
        self.active_tab = Tab::Run;
        self.focus = Focus::Details;
        self.normalize_focus_for_active_surface();
    }

    fn session_path_for_id(&self, session_id: &str) -> Option<PathBuf> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }

        self.session_path
            .as_deref()
            .and_then(Path::parent)
            .map(|parent| parent.join(session_id))
    }

    fn live_switch_to_session(&mut self, session_id: String, session_path: PathBuf) {
        let resume_plan = inspect_resume_plan(&session_path);
        set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
        if resume_plan.is_resumable {
            self.emit_ui_intent(UiIntent::ContinueSession {
                run_id: session_id,
                run_dir: session_path,
            });
        } else {
            self.emit_ui_intent(UiIntent::ReplaySession {
                run_id: session_id,
                run_dir: session_path,
            });
        }
    }

    fn open_replay_session(&mut self, session_id: String, push_current: bool) {
        let Some(session_path) = self.session_path_for_id(&session_id) else {
            self.set_status_banner(Some(
                "session navigation unavailable: missing session path".to_string(),
            ));
            return;
        };

        let snapshot =
            match session_navigation_snapshot_from_path(&session_path, &self.launch_metadata) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    self.set_status_banner(Some(format!("session navigation failed: {err}")));
                    return;
                }
            };

        if push_current {
            if let Some(current_snapshot) = self.current_session_snapshot() {
                let already_pushed = self
                    .session_navigation_stack
                    .last()
                    .map(|existing| existing.session_path.as_path())
                    == Some(current_snapshot.session_path.as_path());
                if !already_pushed {
                    self.session_navigation_stack.push(current_snapshot);
                }
            }
        }

        self.restore_session_snapshot(snapshot);
    }

    fn sibling_child_session_target(&self, reverse: bool) -> Option<String> {
        let current_session_id = self.current_session_id()?;
        let siblings = if let Some(parent_snapshot) = self.session_navigation_stack.last() {
            parent_snapshot.child_session_ids.clone()
        } else {
            let parent_session_id = self.current_parent_session_id()?;
            let parent_session_path = self.session_path_for_id(&parent_session_id)?;
            session_navigation_snapshot_from_path(&parent_session_path, &self.launch_metadata)
                .ok()?
                .child_session_ids
        };

        sibling_session_id(&siblings, current_session_id, reverse)
    }

    pub(super) fn navigate_to_first_child_session(&mut self) {
        let Some(session_id) = self.child_session_ids().into_iter().next() else {
            return;
        };

        if self.replay_mode {
            self.open_replay_session(session_id, true);
            return;
        }

        if let Some(session_path) = self.session_path_for_id(&session_id) {
            self.live_switch_to_session(session_id, session_path);
        }
    }

    pub(super) fn navigate_to_child_sibling(&mut self, reverse: bool) {
        let target_session_id = self.sibling_child_session_target(reverse).or_else(|| {
            let child_session_ids = self.child_session_ids();
            if reverse {
                child_session_ids.into_iter().last()
            } else {
                child_session_ids.into_iter().next()
            }
        });
        let Some(target_session_id) = target_session_id else {
            return;
        };

        if self.replay_mode {
            self.open_replay_session(
                target_session_id,
                self.current_parent_session_id().is_none(),
            );
            return;
        }

        if let Some(session_path) = self.session_path_for_id(&target_session_id) {
            self.live_switch_to_session(target_session_id, session_path);
        }
    }

    pub(super) fn navigate_to_parent_session(&mut self) {
        let Some(parent_session_id) = self.current_parent_session_id() else {
            return;
        };

        if self.replay_mode {
            if let Some(parent_snapshot) = self.session_navigation_stack.pop() {
                self.restore_session_snapshot(parent_snapshot);
                return;
            }

            let Some(parent_session_path) = self.session_path_for_id(&parent_session_id) else {
                self.set_status_banner(Some(
                    "session navigation unavailable: missing parent session path".to_string(),
                ));
                return;
            };
            match session_navigation_snapshot_from_path(&parent_session_path, &self.launch_metadata)
            {
                Ok(snapshot) => self.restore_session_snapshot(snapshot),
                Err(err) => {
                    self.set_status_banner(Some(format!("session navigation failed: {err}")));
                }
            }
            return;
        }

        if let Some(parent_session_path) = self.session_path_for_id(&parent_session_id) {
            self.live_switch_to_session(parent_session_id, parent_session_path);
        }
    }

    pub(crate) fn is_current_model_option(&self, option: &ModelOption) -> bool {
        option.profile == self.active_profile()
            && option.provider == self.active_provider()
            && option.model == self.current_model_id()
            && option.variant() == self.current_model_variant()
    }

    fn rebuild_model_options(&mut self) {
        self.model_options = self.collect_model_options().into_iter().collect();
    }

    fn collect_model_options(&self) -> BTreeSet<ModelOption> {
        let mut options = BTreeSet::new();

        options.extend(self.launch_metadata.available_models().iter().cloned());

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            options.insert(current_option);
        }

        if options.is_empty() {
            for activity in &self.activities {
                if !activity.provider_id.trim().is_empty() && !activity.model_id.trim().is_empty() {
                    options.insert(ModelOption {
                        profile: self.launch_metadata.profile().to_string(),
                        provider: activity.provider_id.clone(),
                        provider_display_label: None,
                        provider_backend_label: None,
                        model: activity.model_id.clone(),
                        model_display_label: None,
                        variant: None,
                        variant_display_label: None,
                        display_label: None,
                        token_window_label: None,
                        context_window_tokens: None,
                        max_input_tokens: None,
                        max_output_tokens: None,
                        description: None,
                        profile_description: None,
                        reasoning_effort: None,
                        text_verbosity: None,
                        recommended_for: None,
                    });
                }
            }

            for entry in &self.session_history_entries {
                let Some(provider_model) = entry.catalog.provider_model.as_deref() else {
                    continue;
                };
                let Some((provider, model)) = provider_model.split_once('/') else {
                    continue;
                };
                options.insert(ModelOption {
                    profile: session_history_profile_label(entry).to_string(),
                    provider: provider.to_string(),
                    provider_display_label: None,
                    provider_backend_label: None,
                    model: model.to_string(),
                    model_display_label: None,
                    variant: None,
                    variant_display_label: None,
                    display_label: None,
                    token_window_label: None,
                    context_window_tokens: None,
                    max_input_tokens: None,
                    max_output_tokens: None,
                    description: None,
                    profile_description: None,
                    reasoning_effort: None,
                    text_verbosity: None,
                    recommended_for: None,
                });
            }
        }

        options
    }

    pub(super) fn update_model_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let mut filtered = self
            .model_options
            .iter()
            .enumerate()
            .filter(|(_, option)| option.matches(&input))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| {
            let left_option = &self.model_options[*left];
            let right_option = &self.model_options[*right];
            self.is_current_model_option(left_option)
                .cmp(&self.is_current_model_option(right_option))
                .reverse()
                .then_with(|| left_option.profile.cmp(&right_option.profile))
                .then_with(|| left_option.provider.cmp(&right_option.provider))
                .then_with(|| left_option.model.cmp(&right_option.model))
        });
        self.model_filtered = filtered;
        self.model_selected = 0;
    }

    pub(super) fn open_model_switcher(&mut self) {
        if !self.model_switcher_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = true;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.rebuild_model_options();
        self.update_model_filter();
        self.sync_slash_overlay();
    }

    pub(super) fn execute_selected_model(&mut self) {
        let Some(selected_index) = self.model_filtered.get(self.model_selected).copied() else {
            self.close_palette();
            return;
        };

        if self.replay_mode {
            self.close_palette();
            return;
        }

        let Some(selected_model) = self.model_options.get(selected_index).cloned() else {
            self.close_palette();
            return;
        };

        self.apply_selected_model_option(selected_model, true);
        self.close_palette();
    }

    pub fn set_session_history_entries(&mut self, entries: Vec<SessionHistoryEntry>) {
        self.session_history_entries = entries;
        self.update_session_history_filter();
        self.rebuild_model_options();
        self.session_history_selected = self
            .session_history_selected
            .min(self.session_history_filtered.len().saturating_sub(1));
    }

    pub fn selected_session_history_entry(&self) -> Option<&SessionHistoryEntry> {
        self.session_history_filtered
            .get(self.session_history_selected)
            .and_then(|index| self.session_history_entries.get(*index))
    }

    pub(super) fn handle_session_history_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_session_history();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_session_launcher_action();
                true
            }
            KeyCode::PageUp => {
                self.move_session_history_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.move_session_history_selection(10);
                true
            }
            KeyCode::Home => {
                self.session_history_selected = 0;
                true
            }
            KeyCode::End => {
                self.session_history_selected =
                    self.session_history_filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.move_session_history_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_session_history_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_session_history_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_session_history_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_session_history_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_session_history_selection(1);
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_session_history_filter);
                true
            }
            _ => false,
        }
    }

    fn move_session_history_selection(&mut self, delta: isize) {
        let len = self.session_history_filtered.len();
        if len == 0 {
            self.session_history_selected = 0;
            return;
        }

        if delta == -1 {
            self.session_history_selected = if self.session_history_selected == 0 {
                len - 1
            } else {
                self.session_history_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.session_history_selected = (self.session_history_selected + 1) % len;
            return;
        }

        let current = self.session_history_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        self.session_history_selected = usize::try_from(next).unwrap_or(0);
    }

    fn update_session_history_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let mut filtered = self
            .session_history_entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                session_history_entry_matches_action(entry, self.startup_launcher_action)
            })
            .filter(|(_, entry)| session_history_filter_matches(entry, &input))
            .map(|(index, entry)| {
                (
                    index,
                    session_history_action_sort_bucket(entry, self.startup_launcher_action),
                )
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|(left_index, left_bucket), (right_index, right_bucket)| {
            let left_entry = &self.session_history_entries[*left_index];
            let right_entry = &self.session_history_entries[*right_index];
            left_bucket
                .cmp(right_bucket)
                .then_with(|| {
                    right_entry
                        .catalog
                        .last_updated_at
                        .as_deref()
                        .unwrap_or("")
                        .cmp(left_entry.catalog.last_updated_at.as_deref().unwrap_or(""))
                })
                .then_with(|| {
                    session_history_run_name(left_entry).cmp(session_history_run_name(right_entry))
                })
                .then_with(|| left_entry.catalog.run_id.cmp(&right_entry.catalog.run_id))
        });
        self.session_history_filtered = filtered.into_iter().map(|(index, _)| index).collect();
        self.session_history_selected = 0;
    }

    pub(super) fn begin_session_history_picker(&mut self, action: StartupLauncherAction) {
        self.startup_launcher_action = action;
        self.continue_disabled_banner = None;
        self.palette_visible = true;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.update_session_history_filter();
        self.open_session_history();
    }

    fn open_session_history(&mut self) {
        if !self.session_history_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.palette_visible = true;
        self.session_history_selected = self
            .session_history_selected
            .min(self.session_history_filtered.len().saturating_sub(1));
        self.session_history_visible = true;
        self.sync_slash_overlay();
    }

    pub(super) fn close_session_history(&mut self) {
        self.close_palette();
    }

    fn execute_selected_session_launcher_action(&mut self) {
        if self.session_history_entries.is_empty() {
            if matches!(
                self.startup_launcher_action,
                StartupLauncherAction::ContinueSession
            ) {
                self.continue_disabled_banner =
                    Some("continue unavailable: no session history entries".to_string());
            } else {
                self.continue_disabled_banner =
                    Some("replay unavailable: no session history entries".to_string());
            }
            self.open_session_history();
            return;
        }

        if self.session_history_filtered.is_empty() {
            self.continue_disabled_banner =
                Some("no sessions match the current filter".to_string());
            self.open_session_history();
            return;
        }

        let Some(selected) = self.selected_session_history_entry() else {
            return;
        };
        let selected_run_id = selected.catalog.run_id.clone();
        let selected_run_dir = selected.run_dir.clone();
        let selected_resumable = selected.catalog.is_resumable;
        let selected_resume_disabled_reason = selected.catalog.resume_disabled_reason.clone();

        match self.startup_launcher_action {
            StartupLauncherAction::NewSession => {
                self.apply_new_session_launcher_selection();
            }
            StartupLauncherAction::ReplaySession => {
                self.continue_disabled_banner = None;
                self.replay_mode = true;
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ReplaySession {
                    run_id: selected_run_id,
                    run_dir: selected_run_dir,
                });
                if self.startup_mode {
                    self.should_quit = true;
                }
                self.close_session_history();
            }
            StartupLauncherAction::ContinueSession => {
                if !selected_resumable {
                    self.continue_disabled_banner = selected_resume_disabled_reason
                        .map(|reason| format!("continue unavailable: {reason}"))
                        .or_else(|| {
                            Some("continue unavailable for the selected session".to_string())
                        });
                    return;
                }

                self.continue_disabled_banner = None;
                self.replay_mode = false;
                set_pending_live_prompt_draft(Some(self.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ContinueSession {
                    run_id: selected_run_id,
                    run_dir: selected_run_dir,
                });
                if self.startup_mode {
                    self.should_quit = true;
                }
                self.close_session_history();
            }
        }
    }
}

fn humanize_profile_label(profile: &str) -> String {
    let words = profile
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        return profile.to_string();
    }
    words.join(" ")
}

fn sibling_session_id(
    session_ids: &[String],
    current_session_id: &str,
    reverse: bool,
) -> Option<String> {
    if session_ids.is_empty() {
        return None;
    }

    let current_index = session_ids
        .iter()
        .position(|session_id| session_id == current_session_id)?;
    let next_index = if reverse {
        current_index
            .checked_sub(1)
            .unwrap_or(session_ids.len().saturating_sub(1))
    } else {
        (current_index + 1) % session_ids.len()
    };
    session_ids.get(next_index).cloned()
}

fn lineage_parent_session_id_from_event(event: &EventEnvelopeV1) -> Option<String> {
    let parent_session_id = match &event.payload {
        EventV1::ToolCallRequested(payload) => payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_session_id.as_deref()),
        EventV1::ToolCallFinished(payload) => payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_session_id.as_deref()),
        EventV1::TaskCompleted(payload) => payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_session_id.as_deref()),
        _ => None,
    }?;

    let parent_session_id = parent_session_id.trim();
    (!parent_session_id.is_empty()).then(|| parent_session_id.to_string())
}

fn parent_session_id_from_events(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().find_map(lineage_parent_session_id_from_event)
}

fn load_session_events(session_path: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let events_path = session_path.join("events.jsonl");
    let body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;
    let mut events = Vec::new();
    for (line_number, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = serde_json::from_str(trimmed).map_err(|err| {
            format!(
                "failed to parse {} line {}: {err}",
                events_path.display(),
                line_number + 1
            )
        })?;
        events.push(event);
    }
    Ok(events)
}

fn infer_launch_metadata_from_events(
    events: &[EventEnvelopeV1],
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    let profile = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(payload) => Some(payload.profile.clone()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.profile().to_string());
    let (provider, model) = events
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

    let mut launch_metadata = LaunchMetadata::new(profile, provider, model)
        .with_available_models(fallback.available_models().to_vec());
    if let Some(mode_label) = fallback.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }
    launch_metadata
}

fn replay_launch_metadata_from_session(
    session_path: &Path,
    events: &[EventEnvelopeV1],
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    load_replay_run_metadata(session_path)
        .and_then(|metadata| {
            metadata
                .recorded_runtime_context
                .as_ref()
                .map(|context| launch_metadata_from_recorded_runtime_context(context, fallback))
        })
        .unwrap_or_else(|| infer_launch_metadata_from_events(events, fallback))
}

fn load_replay_run_metadata(session_path: &Path) -> Option<RunMetadata> {
    let meta_path = session_path.join("meta.json");
    let body = fs::read_to_string(meta_path).ok()?;
    serde_json::from_str(&body).ok()
}

fn launch_metadata_from_recorded_runtime_context(
    recorded_runtime_context: &harness_core::proj::RecordedRuntimeContext,
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    let mut launch_metadata = LaunchMetadata::from_model_option(&ModelOption {
        profile: recorded_runtime_context.profile.clone(),
        provider: recorded_runtime_context.provider.clone(),
        provider_display_label: recorded_runtime_context.provider_display_label.clone(),
        provider_backend_label: recorded_runtime_context.provider_backend_label.clone(),
        model: recorded_runtime_context.model.clone(),
        model_display_label: recorded_runtime_context.model_display_label.clone(),
        variant: recorded_runtime_context.variant.clone(),
        variant_display_label: recorded_runtime_context.variant_display_label.clone(),
        display_label: Some(recorded_runtime_context.display_label.clone())
            .filter(|value| !value.trim().is_empty()),
        token_window_label: recorded_runtime_context.token_window_label.clone(),
        context_window_tokens: recorded_runtime_context.context_window_tokens,
        max_input_tokens: recorded_runtime_context.max_input_tokens,
        max_output_tokens: recorded_runtime_context.max_output_tokens,
        description: recorded_runtime_context.description.clone(),
        profile_description: recorded_runtime_context.profile_description.clone(),
        reasoning_effort: recorded_runtime_context.reasoning_effort.clone(),
        text_verbosity: recorded_runtime_context.text_verbosity.clone(),
        recommended_for: recorded_runtime_context.recommended_for.clone(),
    })
    .with_available_models(fallback.available_models().to_vec());
    if let Some(mode_label) = fallback.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }
    launch_metadata
}

fn session_navigation_snapshot_from_path(
    session_path: &Path,
    fallback_launch_metadata: &LaunchMetadata,
) -> Result<SessionNavigationSnapshot, String> {
    let events = load_session_events(session_path)?;
    let launch_metadata =
        replay_launch_metadata_from_session(session_path, &events, fallback_launch_metadata);
    let replay = AppState::new_replay(session_path.to_path_buf(), events.clone());

    Ok(SessionNavigationSnapshot {
        session_path: session_path.to_path_buf(),
        events,
        launch_metadata,
        child_session_ids: replay.child_session_ids(),
    })
}
