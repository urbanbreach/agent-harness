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
    /// Token prices for the TUI cost counter; catalog models carry models.dev prices.
    pub cost: Option<ModelCost>,
    #[serde(skip)]
    #[schemars(skip)]
    pub limit_provenance: ModelLimitProvenance,
    /// models.dev release date used only for automatic model selection.
    #[serde(skip)]
    #[schemars(skip)]
    pub catalog_release_date: Option<String>,
}

/// Token prices in USD per million tokens, the unit config files and models.dev use.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelCostRates {
    #[schemars(range(min = 0))]
    pub input: f64,
    #[schemars(range(min = 0))]
    pub output: f64,
    #[serde(default, alias = "cacheRead", skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0))]
    pub cache_read: Option<f64>,
    #[serde(default, alias = "cacheWrite", skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0))]
    pub cache_write: Option<f64>,
}

/// Token prices in millionths of a USD per million tokens, i.e. picodollars per token,
/// so session totals add up exactly. Cache tokens without a price bill at the input rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ModelCostRates", into = "ModelCostRates")]
pub struct ModelCost {
    input: u32,
    output: u32,
    cache_read: Option<u32>,
    cache_write: Option<u32>,
}

impl ModelCost {
    /// Picodollars (1e-12 USD) billed for `tokens`.
    pub fn picodollars(&self, tokens: &BilledTokens) -> u128 {
        let cache_rate = |rate: Option<u32>| u128::from(rate.unwrap_or(self.input));
        u128::from(tokens.input) * u128::from(self.input)
            + u128::from(tokens.output) * u128::from(self.output)
            + u128::from(tokens.cache_read) * cache_rate(self.cache_read)
            + u128::from(tokens.cache_write) * cache_rate(self.cache_write)
    }
}

impl TryFrom<ModelCostRates> for ModelCost {
    type Error = String;

    fn try_from(rates: ModelCostRates) -> Result<Self, String> {
        let micros = |usd: f64| {
            micros_per_million(usd).ok_or_else(|| {
                format!("model cost {usd} must be between 0 and 4294 USD per million tokens")
            })
        };
        Ok(Self {
            input: micros(rates.input)?,
            output: micros(rates.output)?,
            cache_read: rates.cache_read.map(micros).transpose()?,
            cache_write: rates.cache_write.map(micros).transpose()?,
        })
    }
}

impl From<ModelCost> for ModelCostRates {
    fn from(cost: ModelCost) -> Self {
        let usd = |micros: u32| f64::from(micros) / 1e6;
        Self {
            input: usd(cost.input),
            output: usd(cost.output),
            cache_read: cost.cache_read.map(usd),
            cache_write: cost.cache_write.map(usd),
        }
    }
}

impl JsonSchema for ModelCost {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ModelCost".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ModelCostRates::json_schema(generator)
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "range checked before the cast"
)]
fn micros_per_million(usd: f64) -> Option<u32> {
    let micros = (usd * 1e6).round();
    (0.0..=f64::from(u32::MAX))
        .contains(&micros)
        .then_some(micros as u32)
}

/// Disjoint billed token counts: uncached prompt, output, cache reads and cache writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BilledTokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl BilledTokens {
    /// Adds one request; `usage.prompt_tokens` already includes cache reads and writes.
    pub fn add(
        &mut self,
        usage: &harness_providers::CompletionUsage,
        cache_read: u32,
        cache_write: u32,
    ) {
        let cached = cache_read.saturating_add(cache_write);
        self.input += u64::from(usage.prompt_tokens.saturating_sub(cached));
        self.output += u64::from(usage.completion_tokens);
        self.cache_read += u64::from(cache_read);
        self.cache_write += u64::from(cache_write);
    }
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
