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
    /// Empty selects `<data>/sessions/<canonical-project-key>`; relative overrides use the workspace directory.
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
    pub mid_turn_enabled: bool,
    // Unknown entries are dropped, the first duplicate wins and a non-array means the default.
    #[serde(deserialize_with = "numbers::method_order")]
    pub method_order: Vec<CompactionMethod>,
    // A percentage above 0 and at most 100, as a number or an `N%` string. `-1` means unset.
    #[serde(deserialize_with = "numbers::threshold_percent")]
    pub threshold_percent: Option<CompactionThresholdPercent>,
    #[serde(deserialize_with = "numbers::threshold_tokens")]
    pub threshold_tokens: Option<NonZeroU32>,
    #[serde(deserialize_with = "selectors::model_thresholds")]
    pub model_thresholds: BTreeMap<String, CompactionThreshold>,
    pub model_thresholds_enabled: bool,
    #[serde(deserialize_with = "selectors::agent_thresholds")]
    pub agent_thresholds: BTreeMap<String, CompactionThreshold>,
    #[serde(deserialize_with = "numbers::reserve_tokens")]
    pub reserve_tokens: Option<u32>,
    #[serde(deserialize_with = "numbers::token_count")]
    pub keep_recent_tokens: u32,
    pub auto_continue: bool,
    pub async_enabled: bool,
    pub handoff_save_to_disk: bool,
    pub remote_streaming_v2_enabled: bool,
    // Provider-native compaction as a whole. Only a literal `false` disables it.
    pub remote_enabled: bool,
    #[serde(deserialize_with = "numbers::endpoint")]
    pub remote_endpoint: Option<String>,
    // Finite numbers are floored, then clamped into the token range, so negatives become 0.
    #[serde(deserialize_with = "numbers::clamped_u32")]
    pub v2_retained_message_budget: u32,
    pub idle_enabled: bool,
    // Finite numbers are floored, then clamped into the token range, so negatives become 0.
    #[serde(deserialize_with = "numbers::clamped_u32")]
    pub idle_threshold_tokens: u32,
    // Finite numbers are floored, then clamped to zero or more seconds.
    #[serde(deserialize_with = "numbers::clamped_u64")]
    pub idle_timeout_seconds: u64,
    pub supersede_reads: bool,
    pub drop_useless: bool,
    pub context_promotion: bool,
    pub promotion_targets: BTreeMap<String, String>,
    pub summary_model: Option<String>,
    // Summary model per `provider/model` selector, preferred over `summary_model`.
    #[serde(deserialize_with = "selectors::summary_models")]
    pub summary_models: BTreeMap<String, String>,
    pub auto_retry_overflow: bool,
    pub estimated_token_triggers: bool,
    #[serde(deserialize_with = "numbers::token_count")]
    pub fallback_input_tokens: u32,
    // Window that percentage thresholds scale from instead of the model's own window.
    #[serde(deserialize_with = "numbers::base_window_tokens")]
    pub base_window_tokens: Option<u32>,
    // Provider-native compaction overrides per `provider/model` selector.
    #[serde(deserialize_with = "selectors::remote_models")]
    pub remote_models: BTreeMap<String, RemoteModelSettings>,
    pub retry: CompactionRetrySettings,
    pub notes_windows: bool,
    pub archive: ArchiveSettings,
    pub branch_summary: BranchSummarySettings,
}

// Provider-native compaction settings for one model selector. Unset fields defer to the
// provider's own rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteModelSettings {
    pub enabled: Option<bool>,
    pub api: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub v2_streaming_enabled: Option<bool>,
    pub v2_endpoint: Option<String>,
}

// Automatic retry of a failed compaction request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CompactionRetrySettings {
    pub enabled: bool,
    pub max_retries: u32,
    pub base_delay_ms: u64,
}

// Which system prompt text archive frames carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveSystemPrompt {
    None,
    AgentsMd,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ArchiveSettings {
    pub shape: String,
    pub system_prompt: ArchiveSystemPrompt,
    pub tool_results: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BranchSummarySettings {
    pub enabled: bool,
    #[serde(deserialize_with = "numbers::token_count")]
    pub reserve_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimePermissionsConfig {
    pub ask_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PromptRuntimeConfig {
    /// Optional cap on how long `harness run`/`prompt` wait for the turn. Unset waits until it ends.
    pub wait_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    /// Empty selects `<data>/sessions/<canonical-project-key>`; relative overrides use the workspace directory.
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
                ("estimatedTokenTriggers", "estimated_token_triggers"),
                ("fallbackInputTokens", "fallback_input_tokens"),
                ("midTurnEnabled", "mid_turn_enabled"),
                ("methodOrder", "method_order"),
                ("modelThresholdsEnabled", "model_thresholds_enabled"),
                ("autoContinue", "auto_continue"),
                ("asyncEnabled", "async_enabled"),
                ("handoffSaveToDisk", "handoff_save_to_disk"),
                ("remoteStreamingV2Enabled", "remote_streaming_v2_enabled"),
                ("remoteEndpoint", "remote_endpoint"),
                ("v2RetainedMessageBudget", "v2_retained_message_budget"),
                ("idleEnabled", "idle_enabled"),
                ("idleThresholdTokens", "idle_threshold_tokens"),
                ("idleTimeoutSeconds", "idle_timeout_seconds"),
                ("supersedeReads", "supersede_reads"),
                ("dropUseless", "drop_useless"),
                ("contextPromotion", "context_promotion"),
                ("promotionTargets", "promotion_targets"),
                ("summaryModel", "summary_model"),
                ("branchSummary", "branch_summary"),
                ("remoteEnabled", "remote_enabled"),
                ("baseWindowTokens", "base_window_tokens"),
                ("remoteModels", "remote_models"),
                ("summaryModels", "summary_models"),
                ("notesWindows", "notes_windows"),
            ][..],
        ),
        (
            "/runtime/compaction/branch_summary",
            &[("reserveTokens", "reserve_tokens")][..],
        ),
        (
            "/runtime/compaction/retry",
            &[
                ("maxRetries", "max_retries"),
                ("baseDelayMs", "base_delay_ms"),
            ][..],
        ),
        (
            "/runtime/compaction/archive",
            &[
                ("systemPrompt", "system_prompt"),
                ("toolResults", "tool_results"),
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
    // Model selectors are dynamic keys, so their entries are canonicalized one by one.
    if let Some(models) = value
        .pointer_mut("/runtime/compaction/remote_models")
        .and_then(serde_json::Value::as_object_mut)
    {
        for model in models.values_mut() {
            if let Some(fields) = model.as_object_mut() {
                normalize::rename(fields, "v2StreamingEnabled", "v2_streaming_enabled")?;
                normalize::rename(fields, "v2Endpoint", "v2_endpoint")?;
            }
        }
    }
    Ok(())
}
