//! Curated labels, descriptions and groups for settings rows.
//!
//! Rows without an entry fall back to the registry copy and then to a label derived from the
//! setting id.

use harness_core::config::{setting_copy, SettingSurface};

/// Where a row belongs in the settings overlay. Order is presentation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SettingGroup {
    General,
    ContextCompaction,
    ContextExperimental,
    AppearanceDisplay,
}

impl SettingGroup {
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::ContextCompaction => "Context / Compaction",
            Self::ContextExperimental => "Context / Experimental",
            Self::AppearanceDisplay => "Appearance / Display",
        }
    }
}

const COMPACTION: &str = "runtime.compaction.";

/// Curated `(label, description)` for the compaction rows and the display flag.
fn curated(id: &str) -> Option<(&'static str, &'static str)> {
    let suffix = id.strip_prefix(COMPACTION);
    let copy = match (id, suffix) {
        ("collapse_compacted_history", _) => (
            "Collapse Compacted History",
            "Show only the retained tail after a compaction. Turn off to keep the full chronological transcript.",
        ),
        (_, Some("enabled")) => (
            "Auto-Compact",
            "Compact the conversation automatically when it nears the context limit.",
        ),
        (_, Some("notes_windows")) => (
            "Notes-backed context windows (experimental)",
            "Keep persistent notes and searchable raw history across context windows.",
        ),
        (_, Some("mid_turn_enabled")) => (
            "Mid-Turn Compaction",
            "Allow compaction between tool calls inside a single turn.",
        ),
        (_, Some("method_order")) => (
            "Compaction Method Order",
            "Order in which compaction methods are tried. A method that fails or is unavailable hands over to the next.",
        ),
        (_, Some("threshold_percent")) => (
            "Compaction Threshold",
            "Share of the model window that starts compaction. Default lets the runtime pick.",
        ),
        (_, Some("threshold_tokens")) => (
            "Compaction Token Limit",
            "Token count that starts compaction. A positive value overrides the percentage threshold.",
        ),
        (_, Some("handoff_save_to_disk")) => (
            "Save Handoff Docs",
            "Also write each automatic handoff document to disk.",
        ),
        (_, Some("remote_enabled")) => (
            "Remote Compaction",
            "Let providers that support it compact the conversation natively.",
        ),
        (_, Some("remote_streaming_v2_enabled")) => (
            "Remote Compaction V2",
            "Use the streaming protocol for provider-native compaction.",
        ),
        (_, Some("async_enabled")) => (
            "Async Compaction",
            "Summarize in the background while the context nears the threshold.",
        ),
        (_, Some("idle_enabled")) => (
            "Idle Compaction",
            "Compact in the background after the session has sat idle.",
        ),
        (_, Some("idle_threshold_tokens")) => (
            "Idle Compaction Threshold",
            "Smallest context size that idle compaction acts on.",
        ),
        (_, Some("idle_timeout_seconds")) => (
            "Idle Compaction Delay",
            "How long the session must stay idle before idle compaction runs.",
        ),
        (_, Some("supersede_reads")) => (
            "Supersede Stale Reads",
            "Drop earlier file reads that a later read or edit of the same file replaced.",
        ),
        (_, Some("drop_useless")) => (
            "Elide Uneventful Results",
            "Remove tool results that carried no information from compacted history.",
        ),
        (_, Some("archive.system_prompt")) => (
            "Archive System Prompt",
            "System prompt text rendered into archive images: none, only AGENTS.md rules, or all of it.",
        ),
        (_, Some("archive.tool_results")) => (
            "Archive Tool Results",
            "Render large tool results into archive images.",
        ),
        (_, Some("archive.shape")) => (
            "Archive Shape",
            "Cell geometry and ink of archive images. Auto picks a shape for the active model.",
        ),
        _ => return None,
    };
    Some(copy)
}

pub(crate) fn curated_label(id: &str) -> Option<String> {
    curated(id)
        .map(|(label, _)| label.to_string())
        .or_else(|| setting_copy(id).map(|copy| copy.label.to_string()))
}

pub(crate) fn description(id: &str) -> Option<&'static str> {
    curated(id)
        .map(|(_, description)| description)
        .or_else(|| setting_copy(id).map(|copy| copy.description))
}

pub(crate) fn group(id: &str, surface: SettingSurface) -> SettingGroup {
    if surface == SettingSurface::Tui {
        return SettingGroup::AppearanceDisplay;
    }
    match id.strip_prefix(COMPACTION) {
        Some(suffix) if suffix == "notes_windows" || suffix.starts_with("archive.") => {
            SettingGroup::ContextExperimental
        }
        Some(_) => SettingGroup::ContextCompaction,
        None => SettingGroup::General,
    }
}
