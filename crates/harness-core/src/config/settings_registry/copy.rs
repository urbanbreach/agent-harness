/// Curated label and description for a registry row, in sentence case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingCopy {
    pub label: &'static str,
    pub description: &'static str,
}

/// Curated copy for the compaction rows that the generic id-derived label would not explain.
pub fn setting_copy(id: &str) -> Option<SettingCopy> {
    let (label, description) = match id {
        "runtime.compaction.notes_windows" => (
            "Notes-backed context windows (experimental)",
            "Keep persistent notes and searchable raw history across context windows.",
        ),
        "runtime.compaction.remote_enabled" => (
            "Remote compaction",
            "Let providers that support it compact the conversation natively.",
        ),
        "runtime.compaction.remote_models" => (
            "Remote compaction by model",
            "Per-model remote compaction overrides, keyed by provider/model selector.",
        ),
        "runtime.compaction.base_window_tokens" => (
            "Base window tokens",
            "Scale percentage thresholds from this window size instead of the model's own window.",
        ),
        "runtime.compaction.summary_models" => (
            "Summary models",
            "Model that writes summaries, keyed by the active provider/model selector.",
        ),
        "runtime.compaction.retry.enabled" => (
            "Automatic retry",
            "Retry a failed compaction request before giving up.",
        ),
        "runtime.compaction.retry.max_retries" => (
            "Compaction retries",
            "Most retries after a failed compaction request.",
        ),
        "runtime.compaction.retry.base_delay_ms" => (
            "Compaction retry delay",
            "Base delay in milliseconds between compaction retries.",
        ),
        "runtime.compaction.archive.system_prompt" => (
            "System prompt imaging",
            "Render no system prompt text, only AGENTS.md rules, or all of it into archive images.",
        ),
        "runtime.compaction.archive.tool_results" => (
            "Tool result imaging",
            "Render large tool results into archive images.",
        ),
        "collapse_compacted_history" => (
            "Collapse compacted history",
            "Hide transcript entries that a compaction summary replaced.",
        ),
        _ => return None,
    };
    Some(SettingCopy { label, description })
}
