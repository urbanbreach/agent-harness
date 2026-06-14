use std::{collections::BTreeMap, path::PathBuf};

use super::{PermissionMode, ToolFailureMode};

pub const DEFAULT_REMOTE_SEARCH_ENDPOINT: &str = "https://mcp.exa.ai/mcp";
pub const DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_REMOTE_SEARCH_MAX_RETRIES: u32 = 1;
pub const DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS: u64 = 250;

pub(super) fn default_hashline_edit() -> bool {
    true
}

pub(super) fn default_logging_level() -> String {
    "info".to_string()
}

pub(super) fn default_max_events_in_memory() -> usize {
    25_000
}

pub(super) fn default_max_transcript_chars_in_memory() -> usize {
    200_000
}

pub(super) fn default_ui_variant_cycle_enabled() -> bool {
    true
}

pub(super) fn default_ui_child_session_navigation_enabled() -> bool {
    true
}

pub(super) fn default_background_task_default_concurrency() -> usize {
    4
}

pub(super) fn default_background_task_provider_concurrency() -> usize {
    4
}

pub(super) fn default_background_task_model_concurrency() -> usize {
    2
}

pub(super) fn default_background_task_stale_timeout_ms() -> u64 {
    30_000
}

pub(super) fn default_background_task_message_staleness_timeout_ms() -> u64 {
    10_000
}

pub(super) fn default_session_dir() -> PathBuf {
    PathBuf::from(".agent-harness/sessions")
}

pub(super) fn default_runtime_ask_timeout_ms() -> u64 {
    30_000
}

pub(super) fn default_prompt_wait_timeout_ms() -> u64 {
    30_000
}

pub(super) fn default_compaction_auto_retry_overflow() -> bool {
    true
}

pub(super) fn default_compaction_structured_summary_contract() -> bool {
    true
}

pub(super) fn default_compaction_estimated_token_triggers() -> bool {
    true
}

pub(super) fn default_compaction_fallback_input_tokens() -> u32 {
    32_768
}

pub(super) fn default_provider_retry_max_retries() -> u32 {
    2
}

pub(super) fn default_provider_retry_base_delay_ms() -> u64 {
    2_000
}

pub(super) fn default_provider_retry_max_delay_ms() -> u64 {
    30_000
}

pub(super) fn default_hook_timeout_ms() -> u64 {
    5_000
}

pub(super) fn default_skills_walk_to_git_root() -> bool {
    true
}

pub(super) fn default_skills_project_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from(".agent-harness/skills"),
        PathBuf::from(".harness/skills"),
    ]
}

pub(super) fn default_skills_global_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("~/.config/agent-harness/skills")]
}

pub(super) fn default_skills_permissions() -> BTreeMap<String, PermissionMode> {
    BTreeMap::from([
        ("*".to_string(), PermissionMode::Allow),
        ("experimental-*".to_string(), PermissionMode::Ask),
        ("internal-*".to_string(), PermissionMode::Deny),
    ])
}

pub(super) fn default_provider_timeout_ms() -> u64 {
    60_000
}

pub(super) fn default_runtime_tool_failure_mode() -> ToolFailureMode {
    ToolFailureMode::ContinueAsToolMessage
}

pub(super) fn default_remote_search_endpoint() -> String {
    DEFAULT_REMOTE_SEARCH_ENDPOINT.to_string()
}

pub(super) fn default_remote_search_timeout_secs() -> u64 {
    DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS
}

pub(super) fn default_remote_search_max_retries() -> u32 {
    DEFAULT_REMOTE_SEARCH_MAX_RETRIES
}

pub(super) fn default_remote_search_retry_backoff_ms() -> u64 {
    DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS
}

pub(super) fn default_mcp_timeout_secs() -> u64 {
    30
}

pub(super) fn default_mcp_enabled() -> bool {
    true
}
