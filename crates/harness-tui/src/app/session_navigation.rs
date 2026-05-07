use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::agent::AgentModelRef;
use harness_core::config::{registered_profile_model_metadata, ResolvedProfileModelMetadata};
use harness_core::event::{first_lineage_parent_session_id, EventEnvelopeV1, EventV1};
use harness_core::proj::{
    inspect_resume_plan, load_run_metadata, SessionCatalogEntry, SessionModeSource,
};
use harness_core::session_lineage::{latest_clone_stable_prefix, StableSessionPrefix};
use serde_json::Value;

use super::{
    json_string_field, set_pending_live_launch_metadata, set_pending_live_prompt_draft,
    ActivityEntry, AppState, Focus, PermissionConfirmSelection, PermissionModalSelection,
    PermissionModalStage, PostRunHandoffAction, ReviewSurface, StartupLauncherAction,
    SubagentSessionInfo, Tab, ToolCallEntry, UiIntent, SLASH_COMMANDS,
};
use crate::keybindings::Action;
use crate::text::{has_trimmed_content, non_empty_trimmed};
use crate::time_format::iso_timestamp_minute;

const SLASH_COMMAND_RESULT_LIMIT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineageSlashCommand {
    Fork,
    Tree,
    Clone,
}

impl LineageSlashCommand {
    fn status_banner(self, blocked_reason: Option<&'static str>) -> &'static str {
        match (self, blocked_reason) {
            (Self::Fork, Some("replay")) => "session fork blocked: replay mode is read-only",
            (Self::Clone, Some("replay")) => "session clone blocked: replay mode is read-only",
            (Self::Fork, Some("active")) => {
                "Harness session fork blocked: live session has active work"
            }
            (Self::Clone, Some("active")) => {
                "Harness session clone blocked: live session has active work"
            }
            (Self::Fork, _) => "session fork is prepared; creation is not available yet",
            (Self::Tree, _) => "session tree is prepared; browser is not available yet",
            (Self::Clone, _) => "Harness session clone blocked: no stable prefix is available",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SessionNavigationSnapshot {
    pub(super) session_path: PathBuf,
    pub(super) events: Vec<EventEnvelopeV1>,
    pub(super) launch_metadata: LaunchMetadata,
    pub(super) child_session_ids: Vec<String>,
    pub(super) replay_mode: bool,
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
        .filter(|value| has_trimmed_content(value))
    {
        parts.push(format!("parent {parent_session_id}"));
    }

    if parts.is_empty() {
        "root session".to_string()
    } else {
        parts.join(" · ")
    }
}

fn non_empty_option(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| has_trimmed_content(value))
}

fn non_empty_str(value: &str) -> Option<&str> {
    has_trimmed_content(value).then_some(value)
}

fn activity_provider_model(activity: &ActivityEntry) -> Option<(&str, &str)> {
    Some((
        non_empty_str(&activity.provider_id)?,
        non_empty_str(&activity.model_id)?,
    ))
}

macro_rules! model_option_label_accessors {
    ($($name:ident),+ $(,)?) => {
        $(
            pub fn $name(&self) -> Option<&str> {
                non_empty_option(&self.$name)
            }
        )+
    };
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
    if let Some(minute) = iso_timestamp_minute(timestamp) {
        format!("updated {}", minute.replace('T', " "))
    } else {
        let trimmed = timestamp.trim();
        if trimmed.is_empty() {
            "updated <unavailable>".to_string()
        } else {
            format!("updated {trimmed}")
        }
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

fn harness_lineage_parent_run_id(run_dir: &Path) -> Option<String> {
    let body = fs::read_to_string(run_dir.join("meta.json")).ok()?;
    let metadata: Value = serde_json::from_str(&body).ok()?;
    metadata
        .get("harness_lineage")
        .and_then(|lineage| lineage.get("parent_run_id"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
        .map(str::to_string)
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
        let model = model.filter(|value| non_empty_str(value).is_some());
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
        non_empty_option(&self.profile).unwrap_or("default")
    }

    pub(super) fn configured_profile(&self) -> Option<&str> {
        non_empty_option(&self.profile)
    }

    pub fn provider(&self) -> &str {
        non_empty_option(&self.provider).unwrap_or("local")
    }

    pub fn profile_description(&self) -> Option<&str> {
        non_empty_option(&self.profile_description)
    }

    pub fn provider_display_label(&self) -> Option<&str> {
        self.fallback_model_option_label(
            &self.provider_display_label,
            ModelOption::provider_display_label,
        )
    }

    pub fn provider_backend_label(&self) -> Option<&str> {
        self.fallback_model_option_label(
            &self.provider_backend_label,
            ModelOption::provider_backend_label,
        )
    }

    pub fn model_display_label(&self) -> Option<&str> {
        self.fallback_model_option_label(
            &self.model_display_label,
            ModelOption::model_display_label,
        )
    }

    pub fn model(&self) -> Option<&str> {
        non_empty_option(&self.model)
    }

    pub fn variant(&self) -> Option<&str> {
        self.fallback_model_option_label(&self.variant, ModelOption::variant)
    }

    pub fn variant_display_label(&self) -> Option<&str> {
        self.fallback_model_option_label(
            &self.variant_display_label,
            ModelOption::variant_display_label,
        )
    }

    pub fn display_label(&self) -> Option<&str> {
        self.fallback_model_option_label(&self.display_label, ModelOption::display_label)
    }

    pub fn token_window_label(&self) -> Option<&str> {
        self.fallback_model_option_label(&self.token_window_label, ModelOption::token_window_label)
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
        self.fallback_model_option_label(&self.description, ModelOption::description)
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.fallback_model_option_label(&self.reasoning_effort, ModelOption::reasoning_effort)
    }

    pub fn text_verbosity(&self) -> Option<&str> {
        self.fallback_model_option_label(&self.text_verbosity, ModelOption::text_verbosity)
    }

    pub fn recommended_for(&self) -> Option<&str> {
        self.fallback_model_option_label(&self.recommended_for, ModelOption::recommended_for)
    }

    pub fn mode_label(&self) -> Option<&str> {
        non_empty_option(&self.mode_label)
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
        let variant = non_empty_option(&self.variant);

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

    fn fallback_model_option_label<'a>(
        &'a self,
        local: &'a Option<String>,
        available: for<'model> fn(&'model ModelOption) -> Option<&'model str>,
    ) -> Option<&'a str> {
        non_empty_option(local).or_else(|| self.matching_available_model().and_then(available))
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

    pub(crate) fn selector_title(&self) -> &str {
        self.display_label()
            .or_else(|| self.model_display_label())
            .unwrap_or(self.model.as_str())
    }

    pub(crate) fn selector_category(&self) -> &str {
        self.provider_display_label()
            .unwrap_or(self.provider.as_str())
    }

    pub fn variant(&self) -> Option<&str> {
        non_empty_option(&self.variant)
    }

    model_option_label_accessors!(
        provider_display_label,
        provider_backend_label,
        model_display_label,
        variant_display_label,
        display_label,
        token_window_label,
        description,
        profile_description,
        reasoning_effort,
        text_verbosity,
        recommended_for,
    );

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

fn model_variant_cycle_cmp(left: &ModelOption, right: &ModelOption) -> std::cmp::Ordering {
    left.provider
        .cmp(&right.provider)
        .then_with(|| left.model.cmp(&right.model))
        .then_with(|| variant_cycle_rank(left).cmp(&variant_cycle_rank(right)))
        .then_with(|| left.variant.cmp(&right.variant))
        .then_with(|| left.profile.cmp(&right.profile))
}

fn model_variant_cycle_none_option(seed: &ModelOption) -> ModelOption {
    let mut option = seed.clone();
    let base_label = option.model_display_label.clone().or_else(|| {
        option
            .display_label
            .as_deref()
            .and_then(|label| label.split_once(" · ").map(|(base, _)| base.trim()))
            .filter(|base| !base.is_empty())
            .map(str::to_string)
    });
    option.variant = None;
    option.variant_display_label = None;
    if option.model_display_label.is_none() {
        option.model_display_label = base_label.clone();
    }
    option.display_label = base_label;
    option.token_window_label = None;
    option.max_input_tokens = None;
    option.max_output_tokens = None;
    option.description = None;
    option.reasoning_effort = None;
    option.text_verbosity = None;
    option.recommended_for = None;
    option
}

fn model_variant_cycle_option_matches_current(
    option: &ModelOption,
    profile_id: &str,
    provider_id: &str,
    model_id: &str,
    variant: Option<&str>,
) -> bool {
    option.profile == profile_id
        && option.provider == provider_id
        && option.model == model_id
        && option.variant() == variant
}

fn variant_cycle_rank(option: &ModelOption) -> u8 {
    option
        .reasoning_effort()
        .or_else(|| option.variant())
        .map(reasoning_variant_rank)
        .unwrap_or(0)
}

fn reasoning_variant_rank(label: &str) -> u8 {
    match label.trim().to_ascii_lowercase().as_str() {
        "none" => 1,
        "minimal" => 2,
        "low" => 3,
        "medium" => 4,
        "high" => 5,
        "xhigh" | "x-high" | "extra-high" => 6,
        "max" => 7,
        _ => u8::MAX,
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

fn model_selector_fuzzy_score(option: &ModelOption, needle: &str) -> Option<usize> {
    let title_score = fuzzy_subsequence_score(&option.selector_title().to_lowercase(), needle)
        .map(|score| score.saturating_mul(2));
    let category_score =
        fuzzy_subsequence_score(&option.selector_category().to_lowercase(), needle)
            .map(|score| score.saturating_mul(2).saturating_add(1));
    match (title_score, category_score) {
        (Some(title), Some(category)) => Some(title.min(category)),
        (Some(title), None) => Some(title),
        (None, Some(category)) => Some(category),
        (None, None) => None,
    }
}

fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if let Some(index) = haystack.find(needle) {
        return Some(index);
    }

    let mut needle_chars = needle.chars();
    let mut current = needle_chars.next()?;
    let mut matched = 0usize;
    let mut gap_score = 0usize;
    for (position, candidate) in haystack.chars().enumerate() {
        if candidate != current {
            gap_score = gap_score.saturating_add(1);
            continue;
        }
        matched = matched.saturating_add(1);
        gap_score = gap_score.saturating_add(position.saturating_sub(matched.saturating_sub(1)));
        match needle_chars.next() {
            Some(next) => current = next,
            None => {
                return Some(gap_score.saturating_add(haystack.len().saturating_sub(needle.len())));
            }
        }
    }
    None
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
        let run_id = self.run_id().and_then(non_empty_str)?;
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
                .and_then(|activity| non_empty_str(&activity.provider_id))
                .filter(|value| !Self::launch_value_is_unknown(value))
                .unwrap_or("local")
        }
    }

    fn current_model_id(&self) -> &str {
        self.launch_metadata
            .model()
            .or_else(|| {
                self.activities
                    .back()
                    .and_then(|activity| non_empty_str(&activity.model_id))
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
            .or_else(|| {
                self.launch_metadata
                    .mode_label()
                    .filter(|label| !label.eq_ignore_ascii_case("live"))
            })
            .filter(|value| !Self::launch_value_is_unknown(value))
    }

    pub(crate) fn current_context_window_tokens(&self) -> Option<u32> {
        self.launch_metadata.context_window_tokens().or_else(|| {
            self.activities
                .back()
                .and_then(|activity| {
                    non_empty_str(&activity.model_id).map(|model_id| {
                        LaunchMetadata::new(
                            self.active_profile(),
                            self.active_provider(),
                            Some(model_id.to_string()),
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
            Some(backend) if !provider_label_includes_backend(provider, backend) => {
                format!("{provider} ({backend})")
            }
            None => provider.to_string(),
            Some(_) => provider.to_string(),
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
        if self.lineage_browser_visible {
            return self.handle_lineage_browser_key(key);
        }

        if self.fork_selector_visible {
            return self.handle_fork_selector_key(key);
        }

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
        if self.prompt_cursor == 0 || self.prompt_buffer.chars().any(char::is_whitespace) {
            return None;
        }

        let cursor_byte = self.prompt_cursor_byte_index();
        let query = self.prompt_buffer[..cursor_byte].strip_prefix('/')?;
        (!query.chars().any(char::is_whitespace)).then_some(query)
    }

    pub(in crate::app) fn clear_slash_menu(&mut self) {
        self.slash_visible = false;
        self.slash_filtered.clear();
        self.slash_selected = 0;
    }

    pub(in crate::app) fn slash_overlay_should_render(&self) -> bool {
        self.slash_visible
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
        let mut filtered = SLASH_COMMANDS
            .iter()
            .filter(|(command, _)| self.slash_command_available(command))
            .filter_map(|(command, description)| {
                slash_command_match_rank(command, description, &slash_query)
                    .map(|rank| (rank, (*command).to_string()))
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        self.slash_filtered = filtered
            .into_iter()
            .take(SLASH_COMMAND_RESULT_LIMIT)
            .map(|(_, command)| command)
            .collect();
        self.slash_selected = 0;
    }

    pub(in crate::app) fn typed_slash_command(&self) -> Option<&'static str> {
        self.prompt_buffer
            .trim()
            .strip_prefix('/')
            .and_then(|command| {
                SLASH_COMMANDS.iter().find_map(|(name, _)| {
                    ((*name == command || slash_command_aliases(name).contains(&command))
                        && self.slash_command_available(name))
                    .then_some(*name)
                })
            })
    }

    pub(crate) fn slash_command_column_width(&self) -> usize {
        SLASH_COMMANDS
            .iter()
            .filter(|(command, _)| self.slash_command_available(command))
            .map(|(command, _)| slash_command_display_width(command))
            .max()
            .unwrap_or(0)
            .saturating_add(2)
    }

    fn slash_command_available(&self, command: &str) -> bool {
        match command {
            "new" | "status" | "exit" => true,
            "resume" | "replay" => !self.replay_mode,
            "fork" => !self.startup_mode && !self.replay_mode,
            "clone" => !self.startup_mode && self.lineage_write_blocked_reason().is_none(),
            "tree" => !self.startup_mode,
            "model" => self.model_switcher_supported(),
            "events" => !self.startup_mode,
            "shell" => self.active_review_surface.is_some(),
            "follow" => !self.replay_mode && !self.startup_mode,
            "compact" => self.compact_session_supported,
            _ => false,
        }
    }

    fn model_switcher_supported(&self) -> bool {
        !self.replay_mode
            && (!self.launch_metadata.available_models().is_empty()
                || self.launch_metadata.model().is_some()
                || self
                    .activities
                    .iter()
                    .any(|activity| activity_provider_model(activity).is_some()))
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
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
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

    pub fn execute_slash_command(&mut self, command: &str, preserved_draft: Option<String>) {
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
            "status" => {
                self.restore_slash_draft(preserved_draft);
                self.status_dialog_visible = true;
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
            "compact" => {
                self.restore_slash_draft(preserved_draft);
                self.emit_ui_intent(UiIntent::CompactSession);
            }
            "fork" => self
                .execute_passive_lineage_slash_command(preserved_draft, LineageSlashCommand::Fork),
            "tree" => self
                .execute_passive_lineage_slash_command(preserved_draft, LineageSlashCommand::Tree),
            "clone" => self
                .execute_passive_lineage_slash_command(preserved_draft, LineageSlashCommand::Clone),
            "exit" => self.execute_action(Action::Quit),
            _ => {}
        }
    }

    fn execute_passive_lineage_slash_command(
        &mut self,
        preserved_draft: Option<String>,
        command: LineageSlashCommand,
    ) {
        self.restore_slash_draft(preserved_draft);
        let blocked_reason = match command {
            LineageSlashCommand::Fork if self.replay_mode => Some("replay"),
            LineageSlashCommand::Fork => None,
            LineageSlashCommand::Clone => self.lineage_write_blocked_reason(),
            LineageSlashCommand::Tree => None,
        };
        if blocked_reason.is_none() {
            match command {
                LineageSlashCommand::Fork => {
                    self.open_fork_selector();
                    return;
                }
                LineageSlashCommand::Tree => {
                    self.open_lineage_browser();
                    return;
                }
                LineageSlashCommand::Clone => {
                    self.execute_clone_from_latest_stable_prefix();
                    return;
                }
            }
        }
        self.set_status_banner(Some(command.status_banner(blocked_reason).to_string()));
    }

    pub(in crate::app) fn source_run_dir_for_lineage_write(&self) -> Result<PathBuf, String> {
        self.session_path
            .clone()
            .ok_or_else(|| "Harness session write blocked: no live session path".to_string())
    }

    pub(in crate::app) fn emit_fork_session_intent(
        &mut self,
        stable_prefix: StableSessionPrefix,
        prompt_text: String,
    ) -> Result<(), String> {
        let source_run_dir = self.source_run_dir_for_lineage_write()?;
        self.emit_ui_intent(UiIntent::ForkSession {
            source_run_dir,
            events: self.events.clone(),
            stable_prefix,
            prompt_text,
        });
        Ok(())
    }

    fn execute_clone_from_latest_stable_prefix(&mut self) {
        let stable_prefix = match latest_clone_stable_prefix(&self.events) {
            Ok(prefix) if prefix.event_count > 0 => prefix,
            Ok(_) => {
                self.set_status_banner(Some(
                    "Harness session clone blocked: no stable events are available".to_string(),
                ));
                return;
            }
            Err(err) => {
                self.set_status_banner(Some(format!("Harness session clone blocked: {err}")));
                return;
            }
        };

        match self.source_run_dir_for_lineage_write() {
            Ok(source_run_dir) => {
                self.emit_ui_intent(UiIntent::CloneSession {
                    source_run_dir,
                    events: self.events.clone(),
                    stable_prefix,
                });
            }
            Err(err) => self.set_status_banner(Some(err)),
        }
    }

    fn lineage_write_blocked_reason(&self) -> Option<&'static str> {
        if self.replay_mode {
            Some("replay")
        } else if self.active_turn_in_progress() {
            Some("active")
        } else {
            None
        }
    }

    pub(in crate::app) fn apply_selected_slash_completion(&mut self) {
        let Some(command) = self.slash_filtered.get(self.slash_selected).cloned() else {
            return;
        };
        self.execute_slash_command(&command, self.slash_draft_snapshot.clone());
    }

    pub(in crate::app) fn handle_model_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_model();
                true
            }
            KeyCode::PageUp => {
                self.move_model_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.move_model_selection(10);
                true
            }
            KeyCode::Home => {
                self.model_selected = 0;
                true
            }
            KeyCode::End => {
                self.model_selected = self.model_filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.move_model_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_model_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_model_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_model_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_model_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_model_selection(1);
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_model_filter);
                true
            }
            _ => false,
        }
    }

    fn move_model_selection(&mut self, delta: isize) {
        let len = self.model_filtered.len();
        if len == 0 {
            self.model_selected = 0;
            return;
        }

        if delta == -1 {
            self.model_selected = if self.model_selected == 0 {
                len - 1
            } else {
                self.model_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.model_selected = (self.model_selected + 1) % len;
            return;
        }

        let current = self.model_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        self.model_selected = usize::try_from(next).unwrap_or(0);
    }

    pub(in crate::app) fn handle_slash_key(&mut self, key: &KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.restore_slash_draft(self.slash_draft_snapshot.clone());
                true
            }
            (KeyCode::Enter, _) | (KeyCode::Tab, _) => {
                self.apply_selected_slash_completion();
                true
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_slash_selection(-1);
                true
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_slash_selection(1);
                true
            }
            _ => false,
        }
    }

    fn move_slash_selection(&mut self, delta: isize) {
        let len = self.slash_filtered.len();
        if len == 0 {
            self.slash_selected = 0;
            return;
        }

        if delta == -1 {
            self.slash_selected = if self.slash_selected == 0 {
                len - 1
            } else {
                self.slash_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.slash_selected = (self.slash_selected + 1) % len;
            return;
        }

        let current = self.slash_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        self.slash_selected = usize::try_from(next).unwrap_or(0);
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
            "toggle_terminal_panel" => self.execute_action(Action::ToggleTerminalPanel),
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
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
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
            return self.model_switcher_supported();
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
        } else if command_id == "toggle_terminal_panel" {
            !self.startup_shell_visible()
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

    pub(in crate::app) fn current_session_id(&self) -> Option<&str> {
        self.session_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .and_then(non_empty_trimmed)
    }

    pub(crate) fn current_subagent_session_info(&self) -> Option<SubagentSessionInfo> {
        let current_session_id = self.current_session_id()?;
        let parent_snapshot = self.session_navigation_stack.last();
        let parent_session_id = parent_snapshot
            .and_then(|snapshot| session_id_from_path(&snapshot.session_path))
            .or_else(|| self.current_parent_session_id());

        parent_session_id.as_ref()?;

        let sibling_ids = parent_snapshot
            .map(|snapshot| snapshot.child_session_ids.clone())
            .or_else(|| {
                parent_session_id
                    .as_deref()
                    .and_then(|session_id| self.session_path_for_id(session_id))
                    .and_then(|path| {
                        session_navigation_snapshot_from_path(&path, &self.launch_metadata).ok()
                    })
                    .map(|snapshot| snapshot.child_session_ids)
            })
            .unwrap_or_default();
        let total = sibling_ids.len().max(1);
        let index = sibling_ids
            .iter()
            .position(|session_id| session_id == current_session_id)
            .map(|idx| idx + 1)
            .unwrap_or(1);

        let task = parent_snapshot
            .and_then(|snapshot| child_task_info_from_events(&snapshot.events, current_session_id))
            .or_else(|| {
                parent_session_id
                    .as_deref()
                    .and_then(|session_id| self.session_path_for_id(session_id))
                    .and_then(|path| {
                        session_navigation_snapshot_from_path(&path, &self.launch_metadata).ok()
                    })
                    .and_then(|snapshot| {
                        child_task_info_from_events(&snapshot.events, current_session_id)
                    })
            });
        let child_agent = child_agent_info_from_events(&self.events, current_session_id);
        let label = task
            .as_ref()
            .and_then(|task| task.label.as_deref())
            .or_else(|| {
                child_agent
                    .as_ref()
                    .and_then(|agent| agent.label.as_deref())
            })
            .map(humanize_profile_label)
            .unwrap_or_else(|| "Subagent".to_string());
        let title = task
            .as_ref()
            .and_then(|task| task.description.clone())
            .or_else(|| {
                child_agent
                    .as_ref()
                    .and_then(|agent| agent.description.clone())
            })
            .filter(|description| has_trimmed_content(description))
            .unwrap_or_else(|| current_session_id.to_string());
        let parent_label = parent_session_id
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| "Parent".to_string());

        Some(SubagentSessionInfo {
            label,
            title,
            parent_label,
            index,
            total,
            usage: subagent_usage_label(self),
        })
    }

    pub(super) fn child_session_ids(&self) -> Vec<String> {
        let mut child_session_ids = BTreeSet::new();
        let delegated_child_request_ids = self.delegated_child_request_ids();

        for activity in &self.activities {
            if delegated_child_request_ids.contains(activity.request_id.as_str()) {
                continue;
            }
            for tool_call in &activity.tool_calls {
                if let Some(child_session_id) =
                    Self::task_tool_child_session_id_from_entry(tool_call)
                {
                    child_session_ids.insert(child_session_id);
                }
            }
        }

        child_session_ids.into_iter().collect()
    }

    fn delegated_child_request_ids(&self) -> BTreeSet<String> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter_map(Self::task_tool_child_request_id)
            .collect()
    }

    fn task_tool_child_request_id(tool_call: &ToolCallEntry) -> Option<String> {
        if !Self::tool_call_is_task_spawn(tool_call) {
            return None;
        }

        tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref())
            .and_then(non_empty_trimmed)
            .map(str::to_string)
            .or_else(|| {
                json_string_field(
                    tool_call.output_json.as_ref(),
                    &["child_request_id", "request_id"],
                )
            })
    }

    fn task_tool_child_session_id_from_entry(tool_call: &ToolCallEntry) -> Option<String> {
        if !Self::tool_call_is_task_spawn(tool_call) {
            return None;
        }

        tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
            .and_then(non_empty_trimmed)
            .map(str::to_string)
            .or_else(|| {
                json_string_field(
                    tool_call.output_json.as_ref(),
                    &["child_session_id", "session_id"],
                )
            })
    }

    fn tool_call_is_task_spawn(tool_call: &ToolCallEntry) -> bool {
        matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
            || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
    }

    pub(super) fn current_parent_session_id(&self) -> Option<String> {
        self.session_path
            .as_deref()
            .and_then(harness_lineage_parent_run_id)
            .or_else(|| first_lineage_parent_session_id(&self.events).map(str::to_string))
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
        let base_variant_exists = variants.iter().any(|option| option.variant().is_none());
        if explicit_variants_exist && !base_variant_exists {
            let none_seed = self
                .launch_metadata
                .to_model_option()
                .or_else(|| variants.first().cloned());
            if let Some(none_seed) = none_seed {
                variants.push(model_variant_cycle_none_option(&none_seed));
            }
        }

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            let current_variant = current_option.variant();
            if current_option.profile == profile_id
                && current_option.provider == provider_id
                && current_option.model == model_id
                && !variants.iter().any(|option| {
                    model_variant_cycle_option_matches_current(
                        option,
                        &profile_id,
                        &provider_id,
                        &model_id,
                        current_variant,
                    )
                })
            {
                variants.push(current_option);
            }
        }

        variants.sort_by(model_variant_cycle_cmp);
        variants.dedup();
        if variants.is_empty() {
            return;
        }

        let selected_model = match variants.iter().position(|option| {
            model_variant_cycle_option_matches_current(
                option,
                &profile_id,
                &provider_id,
                &model_id,
                self.current_model_variant(),
            )
        }) {
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
            replay_mode: self.replay_mode,
        })
    }

    fn restore_session_snapshot(&mut self, snapshot: SessionNavigationSnapshot) {
        self.replay_mode = snapshot.replay_mode;
        self.session_path = Some(snapshot.session_path);
        self.replace_events(snapshot.events);
        self.set_launch_metadata(snapshot.launch_metadata);
        self.active_review_surface = None;
        self.active_tab = Tab::Run;
        self.focus = Focus::Details;
        self.normalize_focus_for_active_surface();
    }

    fn session_path_for_id(&self, session_id: &str) -> Option<PathBuf> {
        let session_id = safe_session_id_path_component(session_id)?;

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

        if !session_path.is_dir() {
            self.open_inline_child_session(session_id, session_path, push_current);
            return;
        }

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

    fn open_inline_child_session(
        &mut self,
        session_id: String,
        session_path: PathBuf,
        push_current: bool,
    ) {
        let Some(snapshot) = self.inline_child_session_snapshot(&session_id, session_path) else {
            self.set_status_banner(Some(format!("subagent session unavailable: {session_id}")));
            return;
        };

        if push_current {
            if let Some(current_snapshot) = self.current_session_snapshot() {
                self.session_navigation_stack.push(current_snapshot);
            }
        }

        self.restore_session_snapshot(snapshot);
    }

    fn inline_child_session_snapshot(
        &self,
        session_id: &str,
        session_path: PathBuf,
    ) -> Option<SessionNavigationSnapshot> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }

        let child_request_ids = self
            .activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter_map(|tool_call| {
                let child_session = tool_call
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.as_deref())
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                    .or_else(|| {
                        json_string_field(
                            tool_call.output_json.as_ref(),
                            &["child_session_id", "session_id", "task_id"],
                        )
                    })?;
                (child_session == session_id).then(|| {
                    tool_call
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.child_request_id.as_deref())
                        .and_then(non_empty_trimmed)
                        .map(str::to_string)
                        .or_else(|| {
                            json_string_field(
                                tool_call.output_json.as_ref(),
                                &["child_request_id", "request_id"],
                            )
                        })
                })
            })
            .flatten()
            .collect::<BTreeSet<_>>();

        let events = self
            .events
            .iter()
            .filter(|event| {
                matches!(&event.payload, EventV1::RunStarted(_))
                    || event.actor.agent_id.as_deref() == Some(session_id)
                    || event
                        .correlation_id
                        .as_deref()
                        .is_some_and(|request_id| child_request_ids.contains(request_id))
                    || matches!(
                        &event.payload,
                        EventV1::AgentSpawned(payload) if payload.agent_id == session_id
                    )
            })
            .cloned()
            .collect::<Vec<_>>();

        (!events.is_empty()).then(|| SessionNavigationSnapshot {
            session_path,
            events,
            launch_metadata: self.launch_metadata.clone(),
            child_session_ids: Vec::new(),
            replay_mode: true,
        })
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

        self.navigate_to_child_session_id(session_id);
    }

    pub(super) fn navigate_to_child_session_id(&mut self, session_id: String) {
        if self.replay_mode {
            self.open_replay_session(session_id, true);
            return;
        }

        if let Some(session_path) = self.session_path_for_id(&session_id) {
            if session_path.is_dir() {
                self.live_switch_to_session(session_id, session_path);
            } else {
                self.open_inline_child_session(session_id, session_path, true);
            }
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
            if session_path.is_dir() {
                self.live_switch_to_session(target_session_id, session_path);
            } else {
                self.open_inline_child_session(target_session_id, session_path, true);
            }
        }
    }

    pub(super) fn navigate_to_parent_session(&mut self) {
        if self.replay_mode {
            if let Some(parent_snapshot) = self.session_navigation_stack.pop() {
                self.restore_session_snapshot(parent_snapshot);
                return;
            }
        }

        let Some(parent_session_id) = self.current_parent_session_id() else {
            return;
        };

        if self.replay_mode {
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
            && option
                .variant()
                .is_none_or(|variant| Some(variant) == self.current_model_variant())
    }

    pub(crate) fn model_switcher_visual_row_count(&self) -> usize {
        if self.model_filtered.is_empty() {
            return 1;
        }
        if !self.palette_input.trim().is_empty() {
            return self.model_filtered.len();
        }

        let mut rows = 0usize;
        let mut groups = 0usize;
        let mut previous_category: Option<&str> = None;
        for option_index in &self.model_filtered {
            let Some(option) = self.model_options.get(*option_index) else {
                continue;
            };
            let category = option.selector_category();
            if previous_category != Some(category) {
                rows = rows.saturating_add(if groups == 0 { 1 } else { 2 });
                groups = groups.saturating_add(1);
                previous_category = Some(category);
            }
            rows = rows.saturating_add(1);
        }
        rows.max(1)
    }

    fn rebuild_model_options(&mut self) {
        self.model_options = self.collect_model_options().into_iter().collect();
    }

    fn collect_model_options(&self) -> BTreeSet<ModelOption> {
        let mut options = BTreeSet::new();

        options.extend(
            self.launch_metadata
                .available_models()
                .iter()
                .map(|option| self.model_selector_option_for_active_profile(option)),
        );

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            options.insert(self.model_selector_option_for_active_profile(&current_option));
        }

        if options.is_empty() {
            for activity in &self.activities {
                if let Some((provider, model)) = activity_provider_model(activity) {
                    let option = ModelOption {
                        profile: self.launch_metadata.profile().to_string(),
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
                    };
                    options.insert(self.model_selector_option_for_active_profile(&option));
                }
            }

            for entry in &self.session_history_entries {
                let Some(provider_model) = entry.catalog.provider_model.as_deref() else {
                    continue;
                };
                let Some((provider, model)) = provider_model.split_once('/') else {
                    continue;
                };
                let option = ModelOption {
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
                };
                options.insert(self.model_selector_option_for_active_profile(&option));
            }
        }

        options
    }

    fn model_option_for_active_profile(&self, option: &ModelOption) -> ModelOption {
        let mut option = option.clone();
        option.profile = self.active_profile().to_string();
        option.profile_description = self
            .launch_metadata
            .profile_description()
            .map(str::to_string);
        option
    }

    fn model_selector_option_for_active_profile(&self, option: &ModelOption) -> ModelOption {
        let mut option = self.model_option_for_active_profile(option);
        option.variant = None;
        option.variant_display_label = None;
        option.display_label = option.model_display_label.clone();
        option.reasoning_effort = None;
        option.text_verbosity = None;
        option.recommended_for = None;
        option
    }

    pub(super) fn update_model_filter(&mut self) {
        let input = self.palette_input.trim().to_lowercase();
        if input.is_empty() {
            let mut filtered = (0..self.model_options.len()).collect::<Vec<_>>();
            filtered.sort_by(|left, right| {
                let left_option = &self.model_options[*left];
                let right_option = &self.model_options[*right];
                left_option
                    .selector_category()
                    .cmp(right_option.selector_category())
                    .then_with(|| {
                        left_option
                            .selector_title()
                            .cmp(right_option.selector_title())
                    })
                    .then_with(|| left_option.model.cmp(&right_option.model))
                    .then_with(|| left_option.variant.cmp(&right_option.variant))
                    .then_with(|| left_option.profile.cmp(&right_option.profile))
            });
            self.model_selected = filtered
                .iter()
                .position(|index| self.is_current_model_option(&self.model_options[*index]))
                .unwrap_or(0);
            self.model_filtered = filtered;
            return;
        }

        let mut filtered = self
            .model_options
            .iter()
            .enumerate()
            .filter_map(|(index, option)| {
                model_selector_fuzzy_score(option, &input).map(|score| (index, score))
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|(left, left_score), (right, right_score)| {
            let left_option = &self.model_options[*left];
            let right_option = &self.model_options[*right];
            left_score
                .cmp(right_score)
                .then_with(|| {
                    left_option
                        .selector_category()
                        .cmp(right_option.selector_category())
                })
                .then_with(|| {
                    left_option
                        .selector_title()
                        .cmp(right_option.selector_title())
                })
                .then_with(|| left_option.model.cmp(&right_option.model))
        });
        self.model_filtered = filtered.into_iter().map(|(index, _)| index).collect();
        self.model_selected = 0;
    }

    pub(super) fn open_model_switcher(&mut self) {
        if !self.model_switcher_supported() {
            self.model_switcher_visible = false;
            return;
        }
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
        if self.lineage_browser_visible {
            let current_run_id = self.current_session_id().map(str::to_string);
            let entries = self
                .session_history_entries
                .iter()
                .map(|entry| entry.catalog.clone())
                .collect::<Vec<_>>();
            self.lineage_browser
                .rebuild(entries, current_run_id, &self.palette_input);
        }
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

fn slash_command_match_rank(command: &str, description: &str, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }

    let display = format!("/{command}");
    let command = command.to_lowercase();
    let description = description.to_lowercase();
    let aliases = slash_command_aliases(command.as_str());

    if command == query || display == query || aliases.contains(&query) {
        return Some((0, 0));
    }

    if command.starts_with(query)
        || display.starts_with(query)
        || aliases.iter().any(|alias| alias.starts_with(query))
    {
        return Some((0, command.len().saturating_sub(query.len())));
    }

    if let Some(index) = command.find(query).or_else(|| display.find(query)) {
        return Some((1, index));
    }

    if let Some(index) = aliases.iter().find_map(|alias| alias.find(query)) {
        return Some((1, index));
    }

    if let Some(score) = slash_subsequence_score(&command, query)
        .or_else(|| slash_subsequence_score(&display, query))
        .or_else(|| {
            aliases
                .iter()
                .filter_map(|alias| slash_subsequence_score(alias, query))
                .min()
        })
    {
        return Some((2, score));
    }

    description.find(query).map(|index| (3, index))
}

fn slash_command_aliases(command: &str) -> &'static [&'static str] {
    match command {
        "new" => &["new-session", "session"],
        "resume" => &["continue"],
        "model" => &["models"],
        "status" => &["system-status"],
        "events" => &["event-log"],
        "shell" => &["session-shell"],
        "compact" => &["summarize", "summary"],
        "exit" => &["quit", "q"],
        _ => &[],
    }
}

fn slash_command_display_width(command: &str) -> usize {
    command.chars().count().saturating_add(1)
}

fn slash_subsequence_score(haystack: &str, needle: &str) -> Option<usize> {
    let mut total_gap = 0usize;
    let mut last_index = 0usize;

    for ch in needle.chars() {
        let next = haystack[last_index..].find(ch)?;
        total_gap = total_gap.saturating_add(next);
        last_index = last_index
            .saturating_add(next)
            .saturating_add(ch.len_utf8());
    }

    Some(total_gap)
}

#[derive(Debug, Clone)]
struct ChildTaskInfo {
    label: Option<String>,
    description: Option<String>,
}

fn session_id_from_path(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

fn safe_session_id_path_component(session_id: &str) -> Option<&str> {
    let session_id = session_id.trim();
    if session_id.is_empty()
        || session_id.contains(['/', '\\'])
        || session_id.chars().any(char::is_control)
    {
        return None;
    }

    let mut components = Path::new(session_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if component.to_str() == Some(session_id) => {
            Some(session_id)
        }
        _ => None,
    }
}

fn child_task_info_from_events(
    events: &[EventEnvelopeV1],
    current_session_id: &str,
) -> Option<ChildTaskInfo> {
    events.iter().rev().find_map(|event| {
        let EventV1::ToolCallRequested(tool_call) = &event.payload else {
            return None;
        };
        let lineage_session = tool_call
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref())
            .and_then(non_empty_trimmed);
        let args = serde_json::from_str::<Value>(&tool_call.args_summary).ok();
        let output_session = args.as_ref().and_then(|value| {
            json_string_field(Some(value), &["child_session_id", "session_id", "task_id"])
        });
        if lineage_session != Some(current_session_id)
            && output_session.as_deref() != Some(current_session_id)
        {
            return None;
        }

        Some(ChildTaskInfo {
            label: args.as_ref().and_then(|value| {
                json_string_field(Some(value), &["subagent_type", "profile", "profile_name"])
            }),
            description: args
                .as_ref()
                .and_then(|value| json_string_field(Some(value), &["description", "task"])),
        })
    })
}

fn child_agent_info_from_events(
    events: &[EventEnvelopeV1],
    current_session_id: &str,
) -> Option<ChildTaskInfo> {
    events.iter().find_map(|event| {
        let EventV1::AgentSpawned(agent) = &event.payload else {
            return None;
        };
        (agent.agent_id == current_session_id).then(|| ChildTaskInfo {
            label: Some(agent.profile.clone()),
            description: None,
        })
    })
}

fn subagent_usage_label(app: &AppState) -> Option<String> {
    let total_tokens = app
        .activities
        .iter()
        .filter_map(|activity| activity.usage)
        .map(|usage| u64::from(usage.total_tokens))
        .sum::<u64>();
    if total_tokens == 0 {
        return None;
    }
    let token_label = compact_usage_count(total_tokens);
    let percent = app.current_context_window_tokens().and_then(|limit| {
        (limit > 0).then(|| format!("{}%", (total_tokens * 100 / u64::from(limit)).min(999)))
    });
    Some(match percent {
        Some(percent) => format!("{token_label} ({percent})"),
        None => token_label,
    })
}

fn compact_usage_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
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

fn provider_label_includes_backend(provider: &str, backend: &str) -> bool {
    let provider = provider.trim();
    let backend = backend.trim();
    if provider.eq_ignore_ascii_case(backend) {
        return true;
    }

    provider
        .strip_suffix(')')
        .and_then(|label| label.rsplit_once('(').map(|(_, suffix)| suffix.trim()))
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(backend))
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
    load_run_metadata(session_path)
        .and_then(|metadata| {
            metadata
                .recorded_runtime_context
                .as_ref()
                .map(|context| launch_metadata_from_recorded_runtime_context(context, fallback))
        })
        .unwrap_or_else(|| infer_launch_metadata_from_events(events, fallback))
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
            .filter(|value| non_empty_str(value).is_some()),
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
    let events = crate::event_log::load_session_events(session_path)?;
    let launch_metadata =
        replay_launch_metadata_from_session(session_path, &events, fallback_launch_metadata);
    let replay = AppState::new_replay(session_path.to_path_buf(), events.clone());

    Ok(SessionNavigationSnapshot {
        session_path: session_path.to_path_buf(),
        events,
        launch_metadata,
        child_session_ids: replay.child_session_ids(),
        replay_mode: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(
        kind: harness_core::event::ActorKind,
        agent_id: &str,
    ) -> harness_core::event::EventActor {
        harness_core::event::EventActor::new(kind, Some(agent_id.to_string()))
    }

    fn event(
        seq: u64,
        correlation_id: Option<&str>,
        actor: harness_core::event::EventActor,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_subagent_nav_{seq:04}"),
            seq,
            run_id: "parent_run".to_string(),
            mono_ms: seq * 100,
            ts: Some(format!("2026-03-22T14:36:{seq:02}Z")),
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    #[test]
    fn subagent_session_info_matches_reference_footer_contract() {
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-subagent-parent/parent_run"));
        app.ingest_event(event(
            1,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::User, "interactive-user"),
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent".to_string(),
                text: "Audit transcript parity".to_string(),
            }),
        ));
        app.ingest_event(event(
            2,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::Worker, "agent_parent"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_parent".to_string(),
                provider_id: "default".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Audit transcript parity".to_string(),
                request_digest: "digest-parent".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(event(
            3,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::System, "coordinator"),
            EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task".to_string(),
                tool_id: "task".to_string(),
                args_summary:
                    r#"{"description":"map chat renderers","subagent_type":"sisyphus-junior"}"#
                        .to_string(),
                args_digest: "digest-task-call".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        parent_session_id: Some("parent_run".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ));
        app.ingest_event(event(
            4,
            Some("req_child"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker"),
            EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_worker".to_string(),
                profile: "sisyphus-junior".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ));
        app.ingest_event(event(
            5,
            Some("req_child"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_child".to_string(),
                provider_id: "default".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "map chat renderers".to_string(),
                request_digest: "digest-child".to_string(),
                metadata: None,
            }),
        ));

        app.navigate_to_child_session_id("agent_worker".to_string());

        let info = app
            .current_subagent_session_info()
            .expect("child session should expose subagent footer info");
        assert_eq!(info.label, "Sisyphus Junior");
        assert_eq!(info.title, "map chat renderers");
        assert_eq!(info.parent_label, "parent_run");
        assert_eq!((info.index, info.total), (1, 1));
        assert!(
            app.replay_mode,
            "inline child sessions should stay read-only"
        );
    }

    #[test]
    fn subagent_session_info_uses_spawned_profile_when_task_args_omit_label() {
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-subagent-parent/parent_run"));
        app.ingest_event(event(
            1,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::System, "coordinator"),
            EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task".to_string(),
                tool_id: "task".to_string(),
                args_summary: r#"{"description":"map chat renderers"}"#.to_string(),
                args_digest: "digest-task-call".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        parent_session_id: Some("parent_run".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ));
        app.ingest_event(event(
            2,
            Some("req_child"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker"),
            EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_worker".to_string(),
                profile: "sisyphus-junior".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ));

        app.navigate_to_child_session_id("agent_worker".to_string());

        let info = app
            .current_subagent_session_info()
            .expect("child session should merge task description with spawned profile");
        assert_eq!(info.label, "Sisyphus Junior");
        assert_eq!(info.title, "map chat renderers");
    }

    #[test]
    fn session_path_for_id_rejects_unsafe_event_derived_ids() {
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-sessions/parent_run"));

        assert_eq!(
            app.session_path_for_id("child_run"),
            Some(PathBuf::from("/tmp/harness-sessions/child_run"))
        );
        for unsafe_id in [
            "",
            ".",
            "..",
            "../secrets",
            "/tmp/secrets",
            "child/run",
            "child\\run",
            "child\nrun",
        ] {
            assert_eq!(
                app.session_path_for_id(unsafe_id),
                None,
                "unsafe session id should be rejected: {unsafe_id:?}"
            );
        }
    }
}
