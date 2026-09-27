use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    #[serde(alias = "defaultProfile")]
    pub default_profile: Option<String>,
    pub keybindings: BTreeMap<String, String>,
    #[serde(rename = "maxEventsInMemory", alias = "max_events_in_memory")]
    pub max_events_in_memory: usize,
    #[serde(
        rename = "maxTranscriptCharsInMemory",
        alias = "max_transcript_chars_in_memory"
    )]
    pub max_transcript_chars_in_memory: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: String,
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct BackgroundTaskSettings {
    #[serde(rename = "defaultConcurrency", alias = "default_concurrency")]
    pub default_concurrency: usize,
    #[serde(rename = "providerConcurrency", alias = "provider_concurrency")]
    pub provider_concurrency: usize,
    #[serde(rename = "modelConcurrency", alias = "model_concurrency")]
    pub model_concurrency: usize,
    #[serde(rename = "staleTimeoutMs", alias = "stale_timeout_ms")]
    pub stale_timeout_ms: u64,
    #[serde(
        rename = "messageStalenessTimeoutMs",
        alias = "message_staleness_timeout_ms"
    )]
    pub message_staleness_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeConfig {
    pub always_approve: bool,
    #[serde(alias = "backgroundTasks")]
    pub background_tasks: BackgroundTaskSettings,
    #[serde(alias = "sessionDir")]
    pub session_dir: PathBuf,
    pub permissions: RuntimePermissionsConfig,
    pub prompt: PromptRuntimeConfig,
    pub deterministic: DeterministicConfig,
    pub compaction: CompactionSettings,
    #[serde(alias = "providerRetry")]
    pub provider_retry: ProviderRetryRuntimeConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderRetryRuntimeConfig {
    #[serde(alias = "maxRetries")]
    pub max_retries: u32,
    #[serde(alias = "baseDelayMs")]
    pub base_delay_ms: u64,
    #[serde(alias = "maxDelayMs")]
    pub max_delay_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CompactionSettings {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_percent: Option<CompactionThresholdPercent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_tokens: Option<NonZeroU32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub model_thresholds: BTreeMap<String, CompactionThreshold>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_thresholds: BTreeMap<String, CompactionThreshold>,
    #[serde(alias = "reserveTokens")]
    pub reserve_tokens: u32,
    #[serde(alias = "keepRecentTokens")]
    pub keep_recent_tokens: u32,
    pub auto_retry_overflow: bool,
    #[serde(alias = "structuredSummaryContract")]
    pub structured_summary_contract: bool,
    pub estimated_token_triggers: bool,
    #[serde(alias = "fallbackInputTokens")]
    pub fallback_input_tokens: u32,
    pub split_oversized_turns: bool,
    pub suppress_auto_compaction: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimePermissionsConfig {
    pub ask_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PromptRuntimeConfig {
    pub wait_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    #[serde(alias = "sessionDir")]
    pub session_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DeterministicConfig {
    pub enabled: bool,
    pub seed: u64,
}

// Canonicalize before merging layers so alternate spellings still override one field.
pub(super) fn normalize_aliases(value: &mut serde_json::Value) -> Result<(), ConfigError> {
    for (path, aliases) in [
        (
            "/runtime",
            &[
                ("alwaysApprove", "always_approve"),
                ("backgroundTasks", "background_tasks"),
                ("sessionDir", "session_dir"),
                ("providerRetry", "provider_retry"),
            ][..],
        ),
        (
            "/runtime/background_tasks",
            &[
                ("defaultConcurrency", "default_concurrency"),
                ("providerConcurrency", "provider_concurrency"),
                ("modelConcurrency", "model_concurrency"),
                ("staleTimeoutMs", "stale_timeout_ms"),
                ("messageStalenessTimeoutMs", "message_staleness_timeout_ms"),
            ][..],
        ),
        (
            "/runtime/compaction",
            &[
                ("thresholdPercent", "threshold_percent"),
                ("thresholdTokens", "threshold_tokens"),
                ("modelThresholds", "model_thresholds"),
                ("agentThresholds", "agent_thresholds"),
                ("reserveTokens", "reserve_tokens"),
                ("keepRecentTokens", "keep_recent_tokens"),
                ("autoRetryOverflow", "auto_retry_overflow"),
                ("structuredSummaryContract", "structured_summary_contract"),
                ("estimatedTokenTriggers", "estimated_token_triggers"),
                ("fallbackInputTokens", "fallback_input_tokens"),
                ("splitOversizedTurns", "split_oversized_turns"),
                ("suppressAutoCompaction", "suppress_auto_compaction"),
            ][..],
        ),
        (
            "/runtime/provider_retry",
            &[
                ("maxRetries", "max_retries"),
                ("baseDelayMs", "base_delay_ms"),
                ("maxDelayMs", "max_delay_ms"),
            ][..],
        ),
        (
            "/runtime/permissions",
            &[("askTimeoutMs", "ask_timeout_ms")][..],
        ),
        (
            "/runtime/prompt",
            &[("waitTimeoutMs", "wait_timeout_ms")][..],
        ),
    ] {
        if let Some(fields) = value
            .pointer_mut(path)
            .and_then(serde_json::Value::as_object_mut)
        {
            for (old, new) in aliases {
                normalize::rename(fields, old, new)?;
            }
        }
    }
    Ok(())
}
