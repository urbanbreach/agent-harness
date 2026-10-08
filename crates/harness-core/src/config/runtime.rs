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
    pub yolo: bool,
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
    /// Coordinator-side guards that keep agents working, out of loops, and informed.
    pub behavior: BehaviorSettings,
}

/// Runtime guidance the coordinator adds to an agent's context while it works.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviorSettings {
    pub todo_continuation: TodoContinuationSettings,
    pub loop_guard: LoopGuardSettings,
    pub stream_guard: StreamGuardSettings,
    pub directory_instructions: DirectoryInstructionSettings,
    pub command_notifications: CommandNotificationSettings,
    pub output_contract: OutputContractSettings,
}

impl BehaviorSettings {
    /// Every guard disabled, keeping the configured limits.
    pub fn off() -> Self {
        let mut settings = Self::default();
        settings.todo_continuation.enabled = false;
        settings.loop_guard.enabled = false;
        settings.stream_guard.enabled = false;
        settings.directory_instructions.enabled = false;
        settings.command_notifications.enabled = false;
        settings.command_notifications.wake_idle = false;
        settings.output_contract.max_retries = 0;
        settings
    }
}

/// Remind an agent that ends its turn while its todo list still has open items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TodoContinuationSettings {
    pub enabled: bool,
    /// Reminders per turn; a reminder is only repeated after the agent made tool calls.
    pub max_reminders: u32,
}

/// Detect an agent repeating the same tool calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct LoopGuardSettings {
    pub enabled: bool,
    /// Consecutive identical calls (or repeated call cycles) that trigger a reminder.
    /// Twice this many stops the turn. Must be between 2 and 100 while enabled.
    #[schemars(range(min = 2, max = 100))]
    pub threshold: u32,
    /// Tools whose repeated identical calls are expected, such as waits.
    pub exempt_tools: Vec<String>,
}

/// Stop a streamed response that degenerates into repetition and retry with a correction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct StreamGuardSettings {
    pub enabled: bool,
    /// Corrected retries per turn before the turn fails.
    pub max_retries: u32,
}

/// Add AGENTS.md files from directories the agent reads or edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DirectoryInstructionSettings {
    pub enabled: bool,
    /// Bytes of one instruction file added to context; longer files are truncated.
    pub max_bytes: u32,
}

/// Tell an agent when a background shell command it started finishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CommandNotificationSettings {
    pub enabled: bool,
    /// Start a turn for an idle agent when one of its background commands finishes.
    pub wake_idle: bool,
}

/// Hold a subagent to the output schema its caller requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct OutputContractSettings {
    /// Corrective reminders before a non-conforming result is returned as invalid.
    pub max_retries: u32,
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
