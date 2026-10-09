use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelConfig {
    #[serde(rename = "name", alias = "display_name", alias = "displayName")]
    pub display_name: String,
    pub metadata: ModelMetadataConfig,
    pub limit: ModelLimitConfig,
    pub modalities: ModelModalitiesConfig,
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    pub variants: BTreeMap<String, ModelVariantConfig>,
    #[serde(skip)]
    #[schemars(skip)]
    pub limit_provenance: ModelLimitProvenance,
    /// models.dev release date used only for automatic model selection.
    #[serde(skip)]
    #[schemars(skip)]
    pub catalog_release_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelProfileConfig {
    pub model: String,
    pub variant: Option<String>,
    pub fallback: Vec<ModelProfileTargetConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelProfileTargetConfig {
    pub model: String,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelVariantConfig {
    #[serde(rename = "name", alias = "display_name", alias = "displayName")]
    pub display_name: Option<String>,
    pub metadata: ModelVariantMetadataConfig,
    pub limit: ModelLimitConfig,
    pub modalities: ModelModalitiesConfig,
    pub options: BTreeMap<String, serde_json::Value>,
    pub disabled: bool,
    #[serde(alias = "contextWindowTokens")]
    pub context_window_tokens: Option<u32>,
    #[serde(alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelLimitConfig {
    pub context: Option<u32>,
    pub input: Option<u32>,
    pub output: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelModalitiesConfig {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelMetadataConfig {
    pub family: Option<String>,
    #[serde(alias = "releaseStage")]
    pub release_stage: Option<ModelReleaseStage>,
    #[serde(alias = "contextWindowTokens")]
    pub context_window_tokens: Option<u32>,
    #[serde(alias = "supportsToolCalls")]
    pub supports_tool_calls: Option<bool>,
    #[serde(alias = "supportsReasoningSummaries")]
    pub supports_reasoning_summaries: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ModelReleaseStage {
    Stable,
    Preview,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelVariantMetadataConfig {
    pub description: Option<String>,
    #[serde(alias = "reasoningEffort")]
    pub reasoning_effort: Option<ModelVariantReasoningEffort>,
    #[serde(alias = "textVerbosity")]
    pub text_verbosity: Option<ModelVariantTextVerbosity>,
    #[serde(alias = "recommendedFor")]
    pub recommended_for: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Copy)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariantTextVerbosity {
    Low,
    Medium,
    High,
}

impl ModelVariantReasoningEffort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
            Self::Xhigh => "xhigh",
        }
    }
}
impl ModelVariantTextVerbosity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}
