use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model_resolution::ModelResolution;

use super::{ModelLimitProvenance, ResolvedModelLimits};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    #[serde(rename = "name", alias = "display_name", alias = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub metadata: ModelMetadataConfig,
    #[serde(default)]
    pub limit: ModelLimitConfig,
    #[serde(default)]
    pub modalities: ModelModalitiesConfig,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub variants: BTreeMap<String, ModelVariantConfig>,
    #[serde(skip)]
    #[schemars(skip)]
    pub limit_provenance: ModelLimitProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileConfig {
    pub model: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub fallback: Vec<ModelProfileTargetConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileTargetConfig {
    pub model: String,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelVariantConfig {
    #[serde(
        rename = "name",
        default,
        alias = "display_name",
        alias = "displayName"
    )]
    pub display_name: Option<String>,
    #[serde(default)]
    pub metadata: ModelVariantMetadataConfig,
    #[serde(default)]
    pub limit: ModelLimitConfig,
    #[serde(default)]
    pub modalities: ModelModalitiesConfig,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, alias = "contextWindowTokens")]
    pub context_window_tokens: Option<u32>,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelLimitConfig {
    #[serde(default)]
    pub context: Option<u32>,
    #[serde(default)]
    pub input: Option<u32>,
    #[serde(default)]
    pub output: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelModalitiesConfig {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfileModelMetadata {
    pub profile: String,
    pub profile_description: Option<String>,
    pub provider: String,
    pub provider_display_label: String,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: String,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
    pub display_label: String,
    pub token_window_label: Option<String>,
    pub limits: ResolvedModelLimits,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub resolution: ModelResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelCatalogEntry {
    pub provider: String,
    pub provider_display_label: String,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: String,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
    pub display_label: String,
    pub token_window_label: Option<String>,
    pub limits: ResolvedModelLimits,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub supports_reasoning_summaries: bool,
    pub resolution: ModelResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelTarget {
    pub model_ref: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub limits: ResolvedModelLimits,
    pub resolution: ModelResolution,
    pub catalog_entry: Option<Box<ResolvedModelCatalogEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelSelection {
    pub selector: String,
    pub profile: Option<String>,
    pub primary: ResolvedModelTarget,
    pub fallback: Vec<ResolvedModelTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelProfileCatalogEntry {
    pub name: String,
    pub primary: ResolvedModelTarget,
    pub fallback: Vec<ResolvedModelTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadataConfig {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default, alias = "releaseStage")]
    pub release_stage: Option<ModelReleaseStage>,
    #[serde(default, alias = "contextWindowTokens")]
    pub context_window_tokens: Option<u32>,
    #[serde(default, alias = "supportsToolCalls")]
    pub supports_tool_calls: Option<bool>,
    #[serde(default, alias = "supportsReasoningSummaries")]
    pub supports_reasoning_summaries: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelReleaseStage {
    Stable,
    Preview,
    Deprecated,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelVariantMetadataConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "reasoningEffort")]
    pub reasoning_effort: Option<ModelVariantReasoningEffort>,
    #[serde(default, alias = "textVerbosity")]
    pub text_verbosity: Option<ModelVariantTextVerbosity>,
    #[serde(default, alias = "recommendedFor")]
    pub recommended_for: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariantReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Max,
    Xhigh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariantTextVerbosity {
    Low,
    Medium,
    High,
}
