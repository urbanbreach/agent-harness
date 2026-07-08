// allow: SIZE_OK — TUI app state (session projection + interaction)
use harness_core::agent::AgentModelRef;
use harness_core::config::{registered_profile_model_metadata, ResolvedProfileModelMetadata};
use serde_json::Value;

use crate::text::has_trimmed_content;

fn non_empty_option(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| has_trimmed_content(value))
}

fn non_empty_str(value: &str) -> Option<&str> {
    has_trimmed_content(value).then_some(value)
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
    thinking: Option<Value>,
    recommended_for: Option<String>,
    mode_label: Option<String>,
    available_models: Vec<ModelOption>,
    switchable_profiles: Vec<String>,
    mcp_resources: Vec<McpResourceOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpResourceOption {
    pub name: String,
    pub uri: String,
    pub mime: String,
    pub description: Option<String>,
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
            thinking: None,
            recommended_for: None,
            mode_label: None,
            available_models: Vec::new(),
            switchable_profiles: Vec::new(),
            mcp_resources: Vec::new(),
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
            thinking: option.thinking.clone(),
            recommended_for: option.recommended_for.clone(),
            mode_label: None,
            available_models: Vec::new(),
            switchable_profiles: Vec::new(),
            mcp_resources: Vec::new(),
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

    pub fn with_switchable_profiles(mut self, switchable_profiles: Vec<String>) -> Self {
        self.switchable_profiles = switchable_profiles;
        self
    }

    pub fn with_mcp_resources(mut self, mcp_resources: Vec<McpResourceOption>) -> Self {
        self.mcp_resources = mcp_resources;
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

    pub fn has_provider(&self) -> bool {
        self.provider.is_some()
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

    pub fn thinking(&self) -> Option<&Value> {
        self.thinking.as_ref().or_else(|| {
            self.matching_available_model()
                .and_then(|option| option.thinking.as_ref())
        })
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

    pub fn switchable_profiles(&self) -> &[String] {
        &self.switchable_profiles
    }

    pub fn mcp_resources(&self) -> &[McpResourceOption] {
        &self.mcp_resources
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
            thinking: self.thinking().cloned(),
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
        self.thinking = metadata.thinking.clone();
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
    pub thinking: Option<Value>,
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
            thinking: None,
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
