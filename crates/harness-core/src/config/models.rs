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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Long-context prices, in models.dev's shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<ModelCostTier>,
    /// Peak hours bill these prices; other hours multiply them by `off_peak_multiplier`.
    #[serde(default, alias = "timeBased", skip_serializing_if = "Option::is_none")]
    pub time_based: Option<ModelCostSchedule>,
}

/// Peak hours in UTC and the price multiplier for every other hour. A request's start time
/// decides its whole price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelCostSchedule {
    #[serde(alias = "offPeakMultiplier")]
    #[schemars(range(min = 0))]
    pub off_peak_multiplier: f64,
    #[serde(alias = "peakWindows")]
    pub peak_windows: Vec<ModelCostPeakWindow>,
}

/// UTC minutes of the day, start inclusive and end exclusive, on each listed weekday
/// (0 = Sunday).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelCostPeakWindow {
    #[schemars(inner(range(max = 6)), length(min = 1))]
    pub weekdays: Vec<u8>,
    #[serde(alias = "startMinute")]
    #[schemars(range(max = 1439))]
    pub start_minute: u16,
    #[serde(alias = "endMinute")]
    #[schemars(range(min = 1, max = 1440))]
    pub end_minute: u16,
}

/// Prices for a whole request whose prompt, cached tokens included, exceeds `tier.size`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelCostTier {
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
    pub tier: ModelCostTierBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelCostTierBound {
    #[serde(rename = "type")]
    pub kind: ModelCostTierKind,
    /// Prompt tokens a request must exceed to bill at this tier.
    pub size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelCostTierKind {
    Context,
}

/// Token prices in millionths of a USD per million tokens, i.e. picodollars per token,
/// so session totals add up exactly. Cache tokens without a price bill at the input rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ModelCostRates", into = "ModelCostRates")]
pub struct ModelCost {
    base: TokenPrices,
    /// Prompt sizes a request must exceed, ascending, with the prices it then bills at.
    tiers: Vec<(u32, TokenPrices)>,
    schedule: Option<PeakSchedule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenPrices {
    input: u32,
    output: u32,
    cache_read: Option<u32>,
    cache_write: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PeakSchedule {
    /// Millionths of the peak price billed off-peak.
    off_peak: u32,
    windows: Vec<ModelCostPeakWindow>,
}

impl PeakSchedule {
    fn is_peak(&self, unix_ms: i64) -> bool {
        const DAY_MS: i64 = 86_400_000;
        // 1970-01-01 was a Thursday.
        let weekday = (unix_ms.div_euclid(DAY_MS) + 4).rem_euclid(7);
        let minute = unix_ms.rem_euclid(DAY_MS) / 60_000;
        self.windows.iter().any(|window| {
            window.weekdays.iter().any(|day| i64::from(*day) == weekday)
                && (i64::from(window.start_minute)..i64::from(window.end_minute)).contains(&minute)
        })
    }
}

impl ModelCost {
    /// Picodollars (1e-12 USD) billed for one request that started at `started_at_ms`, in
    /// Unix milliseconds. A prompt over a tier's size bills the whole request, output
    /// included, at that tier's prices. One-hour cache writes bill at twice the input price.
    /// Off-peak requests bill the schedule's share. `None` when the price depends on an
    /// unknown start time.
    pub fn picodollars(&self, request: &BilledTokens, started_at_ms: Option<i64>) -> Option<u128> {
        let prompt = u64::from(request.input)
            + u64::from(request.cache_read)
            + u64::from(request.cache_write);
        let prices = self
            .tiers
            .iter()
            .rev()
            .find(|(size, _)| prompt > u64::from(*size))
            .map_or(&self.base, |(_, prices)| prices);
        let cache_rate = |rate: Option<u32>| u128::from(rate.unwrap_or(prices.input));
        let one_hour = request.cache_write_1h.min(request.cache_write);
        let cost = u128::from(request.input) * u128::from(prices.input)
            + u128::from(request.output) * u128::from(prices.output)
            + u128::from(request.cache_read) * cache_rate(prices.cache_read)
            + u128::from(request.cache_write - one_hour) * cache_rate(prices.cache_write)
            + u128::from(one_hour) * 2 * u128::from(prices.input);
        let Some(schedule) = &self.schedule else {
            return Some(cost);
        };
        Some(if schedule.is_peak(started_at_ms?) {
            cost
        } else {
            cost * u128::from(schedule.off_peak) / 1_000_000
        })
    }
}

impl TryFrom<ModelCostRates> for ModelCost {
    type Error = String;

    fn try_from(rates: ModelCostRates) -> Result<Self, String> {
        let mut tiers = rates
            .tiers
            .iter()
            .map(|tier| {
                let prices =
                    token_prices(tier.input, tier.output, tier.cache_read, tier.cache_write)?;
                Ok((tier.tier.size, prices))
            })
            .collect::<Result<Vec<_>, String>>()?;
        tiers.sort_unstable_by_key(|(size, _)| *size);
        let schedule = rates
            .time_based
            .map(|schedule| {
                let valid = |w: &ModelCostPeakWindow| {
                    !w.weekdays.is_empty()
                        && w.weekdays.iter().all(|day| *day <= 6)
                        && w.start_minute < w.end_minute
                        && w.end_minute <= 1440
                };
                if !schedule.peak_windows.iter().all(valid) {
                    return Err("peak windows need weekdays 0 (Sunday) to 6 and minutes \
                        0 <= startMinute < endMinute <= 1440"
                        .to_owned());
                }
                Ok(PeakSchedule {
                    off_peak: micros_per_million(schedule.off_peak_multiplier).ok_or_else(
                        || "off-peak multiplier must be between 0 and 4294".to_owned(),
                    )?,
                    windows: schedule.peak_windows,
                })
            })
            .transpose()?;
        Ok(Self {
            base: token_prices(
                rates.input,
                rates.output,
                rates.cache_read,
                rates.cache_write,
            )?,
            tiers,
            schedule,
        })
    }
}

fn token_prices(
    input: f64,
    output: f64,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
) -> Result<TokenPrices, String> {
    let micros = |usd: f64| {
        micros_per_million(usd).ok_or_else(|| {
            format!("model cost {usd} must be between 0 and 4294 USD per million tokens")
        })
    };
    Ok(TokenPrices {
        input: micros(input)?,
        output: micros(output)?,
        cache_read: cache_read.map(micros).transpose()?,
        cache_write: cache_write.map(micros).transpose()?,
    })
}

impl From<ModelCost> for ModelCostRates {
    fn from(cost: ModelCost) -> Self {
        let usd = |micros: u32| f64::from(micros) / 1e6;
        let TokenPrices {
            input,
            output,
            cache_read,
            cache_write,
        } = cost.base;
        Self {
            input: usd(input),
            output: usd(output),
            cache_read: cache_read.map(usd),
            cache_write: cache_write.map(usd),
            tiers: cost
                .tiers
                .into_iter()
                .map(|(size, prices)| ModelCostTier {
                    input: usd(prices.input),
                    output: usd(prices.output),
                    cache_read: prices.cache_read.map(usd),
                    cache_write: prices.cache_write.map(usd),
                    tier: ModelCostTierBound {
                        kind: ModelCostTierKind::Context,
                        size,
                    },
                })
                .collect(),
            time_based: cost.schedule.map(|schedule| ModelCostSchedule {
                off_peak_multiplier: usd(schedule.off_peak),
                peak_windows: schedule.windows,
            }),
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

/// Disjoint billed token counts of one request: uncached prompt, output, cache reads and
/// cache writes, of which `cache_write_1h` were kept for an hour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BilledTokens {
    pub input: u32,
    pub output: u32,
    pub cache_read: u32,
    pub cache_write: u32,
    pub cache_write_1h: u32,
}

impl BilledTokens {
    /// `usage.prompt_tokens` already includes cache reads and writes.
    pub fn new(
        usage: &harness_providers::CompletionUsage,
        cache_read: u32,
        cache_write: u32,
        cache_write_1h: u32,
    ) -> Self {
        Self {
            input: usage
                .prompt_tokens
                .saturating_sub(cache_read.saturating_add(cache_write)),
            output: usage.completion_tokens,
            cache_read,
            cache_write,
            cache_write_1h,
        }
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
