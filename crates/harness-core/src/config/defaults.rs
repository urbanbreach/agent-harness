use super::*;

macro_rules! defaults {
    ($($name:ident { $($field:ident: $value:expr),* $(,)? })*) => {$(
        impl Default for $name { fn default() -> Self { Self { $($field: $value),* } } }
    )*};
}

defaults! {
    HarnessConfig {
        schema: None, providers: BTreeMap::new(), disabled_providers: Vec::new(), enabled_providers: Vec::new(), model_profiles: BTreeMap::new(),
        agents: BTreeMap::new(), subagents: SubagentsConfig::default(), features: SubagentFeaturesConfig::default(),
        permissions: PermissionsConfig::default(), runtime: RuntimeConfig::default(), integrations: IntegrationsConfig::default(),
        hooks: HooksConfig::default(), skills: SkillsConfig::default(), lsp: LspConfig::default(), eval: EvalConfig::default(), background_task: BackgroundTaskSettings::default(),
        paths: PathsConfig::default(), deterministic: DeterministicConfig::default(), ui: UiConfig::default(), logging: LoggingConfig::default(),
        hashline_edit: true, formatter: FormatterConfig::default(), instruction_files: Vec::new(), small_model: None
    }
    UiConfig { default_profile: None, keybindings: BTreeMap::new(), max_events_in_memory: 25_000, max_transcript_chars_in_memory: 200_000 }
    LoggingConfig { level: "info".into(), file: None }
    BackgroundTaskSettings { default_concurrency: 4, provider_concurrency: 4, model_concurrency: 2, stale_timeout_ms: 30_000, message_staleness_timeout_ms: 10_000 }
    RuntimePermissionsConfig { ask_timeout_ms: 30_000 }
    PromptRuntimeConfig { wait_timeout_ms: 30_000 }
    PathsConfig { session_dir: ".agent-harness/sessions".into() }
    RuntimeConfig {
        yolo: false, background_tasks: BackgroundTaskSettings::default(), session_dir: PathsConfig::default().session_dir,
        permissions: RuntimePermissionsConfig::default(), prompt: PromptRuntimeConfig::default(), deterministic: DeterministicConfig::default(),
        compaction: CompactionSettings::default(), provider_retry: ProviderRetryRuntimeConfig::default(), behavior: BehaviorSettings::default()
    }
    BehaviorSettings {
        todo_continuation: TodoContinuationSettings::default(), loop_guard: LoopGuardSettings::default(),
        stream_guard: StreamGuardSettings::default(), directory_instructions: DirectoryInstructionSettings::default(),
        command_notifications: CommandNotificationSettings::default(), output_contract: OutputContractSettings::default()
    }
    TodoContinuationSettings { enabled: true, max_reminders: 3 }
    LoopGuardSettings {
        enabled: true, threshold: 4,
        exempt_tools: vec!["wait_commands_or_subagents".into(), "get_command_or_subagent_output".into(), "question".into()]
    }
    StreamGuardSettings { enabled: true, max_retries: 2 }
    DirectoryInstructionSettings { enabled: true, max_bytes: 32_768 }
    CommandNotificationSettings { enabled: true, wake_idle: true }
    OutputContractSettings { max_retries: 2 }
    ProviderRetryRuntimeConfig { max_retries: 2, base_delay_ms: 2_000, max_delay_ms: 30_000 }
    CompactionSettings {
        enabled: true, threshold_percent: None, threshold_tokens: None, model_thresholds: BTreeMap::new(), agent_thresholds: BTreeMap::new(),
        reserve_tokens: 16_384, keep_recent_tokens: 20_000, auto_retry_overflow: true, structured_summary_contract: true,
        estimated_token_triggers: true, fallback_input_tokens: 32_768, split_oversized_turns: false, suppress_auto_compaction: false
    }
    ProfileConfig {
        name: None, description: String::new(), system_prompt: None, model_ref: String::new(), model_ref_explicit: false, variant: None,
        temperature: None, top_p: None, mode: AgentMode::All, hidden: false, color: None, options: BTreeMap::new(), permissions: None,
        max_iters: None, tool_failure_mode: ToolFailureMode::ContinueAsToolMessage, tools: Vec::new()
    }
    PermissionDefaultsConfig {
        edit: PermissionMode::Allow, shell: PermissionMode::Allow, network: PermissionMode::Allow, question: Some(PermissionMode::Deny),
        task: Some(PermissionMode::Allow), eval: Some(PermissionMode::Ask), webfetch: Some(PermissionMode::Allow), websearch: Some(PermissionMode::Allow), codesearch: Some(PermissionMode::Allow),
        lsp: Some(PermissionMode::Allow), read: Some(PermissionMode::Allow), external_directory: Some(PermissionMode::Ask), doom_loop: Some(PermissionMode::Ask)
    }
    PermissionsConfig { defaults: PermissionDefaultsConfig::default(), fallback: None, rules: default_permission_rule_set_with_read_env(), shell_allowlist: ShellAllowlist::default() }
    SkillsConfig { project_roots: vec![".agent-harness/skills".into(), ".harness/skills".into()], global_roots: vec!["~/.config/agent-harness/skills".into()], urls: Vec::new(), disabled: Vec::new(), walk_to_git_root: true, permissions: BTreeMap::new() }
    LifecycleHookConfig { id: None, event: HookLifecycleEvent::ToolCallStarted, command: Vec::new(), cwd: None, timeout_ms: 5_000, critical: false, env: BTreeMap::new() }
    FormatterConfig { enabled: true, experimental_oxfmt: false, overrides: BTreeMap::new() }
    RemoteSearchConfig { endpoint: "https://mcp.exa.ai/mcp".into(), auth_token: None, require_auth: false, timeout_secs: 30, max_retries: 1, retry_backoff_ms: 250 }
    OpenAiCompatibleProviderConfig {
        name: None, auth_provider: None, base_url: "https://api.openai.com/v1".into(), api_key: String::new(), api_key_env: Vec::new(),
        timeout_ms: 60_000, api_mode: OpenAiApiMode::Auto, cache_retention: harness_providers::CacheRetention::Short,
        headers: BTreeMap::new(), options: OpenAiCompatibleProviderOptions::default(), models: BTreeMap::new()
    }
    AnthropicProviderConfig {
        name: None, base_url: "https://api.anthropic.com/v1".into(), api_key: String::new(), api_key_env: Vec::new(), timeout_ms: 60_000,
        headers: BTreeMap::new(), options: AnthropicProviderOptions::default(), models: BTreeMap::new()
    }
}

pub fn default_read_env_permission_rules() -> Vec<PermissionSelectorRule> {
    [
        ("*", PermissionMode::Allow),
        ("*.env", PermissionMode::Ask),
        ("*.env.*", PermissionMode::Ask),
        ("*.env.example", PermissionMode::Allow),
    ]
    .into_iter()
    .map(|(pattern, mode)| PermissionSelectorRule {
        selector: PermissionSelector::Glob(pattern.into()),
        mode,
    })
    .collect()
}
pub fn default_permission_rule_set_with_read_env() -> PermissionRuleSet {
    PermissionRuleSet {
        read: default_read_env_permission_rules(),
        ..Default::default()
    }
}
