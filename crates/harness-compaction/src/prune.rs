//! Pure, reversible tool-output replacement plans.
//!
//! Planners read [`Item`](crate::Item)s and return the edits a coordinator should apply
//! together with the tokens those edits save. They never mutate, read clocks or perform I/O.
use crate::item::Item;
use harness_providers::AssistantToolCall;

mod age;
mod collect;
mod facts;
mod incremental;
mod lookback;
mod matcher;

pub use age::plan_age_prune;
pub use facts::{read_complete, tool_facts, ResultFacts};
pub use incremental::plan_incremental_prune;
pub use lookback::lookback_floor;
pub use matcher::{call_index, protects, read_path, skill_protection, ToolMatcher};
pub(crate) use matcher::{counts_toward_suffix, eligible, read_target, Calls};

pub const SUPERSEDED_NOTICE: &str = "[Superseded by a newer read of this file]";
pub const USELESS_NOTICE: &str = "[Uneventful result elided]";
/// Results smaller than this never age out; replacing them saves nothing.
pub const MIN_PRUNE_TOKENS: u32 = 50;
/// Default age pruning: tool output newer than this many tokens is kept.
pub const DEFAULT_PROTECT_TOKENS: u32 = 40_000;
/// Default age pruning: skip the whole pass unless it saves at least this many tokens.
pub const DEFAULT_MINIMUM_SAVINGS: u32 = 20_000;
/// Incremental pruning leaves the newest messages within this many tokens untouched.
pub const WARM_SUFFIX_TOKENS: u32 = 8_000;
/// Generic idle gap after which a provider cache is assumed cold.
pub const IDLE_FLUSH_MS: u64 = 30 * 60 * 1000;
/// Idle gap a coordinator session uses before flushing regardless of warm suffixes.
pub const SESSION_IDLE_FLUSH_MS: u64 = 90 * 60 * 1000;

pub fn aged_notice(tokens: u32) -> String {
    format!("[Output truncated - {tokens} tokens]")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneEdit {
    pub index: usize,
    pub replacement: String,
    pub kind: PruneKind,
    /// Tokens this replacement saves.
    pub savings: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneKind {
    Superseded,
    Useless,
    Aged,
}

/// What a pruning pass would do: edits in chronological order and the tokens they save.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrunePlan {
    pub edits: Vec<PruneEdit>,
    pub savings: u32,
}

/// Derives the supersession identity of a result from its paired call.
pub type SupersedeKey<'a> = dyn Fn(&Item<'_>, &AssistantToolCall) -> Option<String> + 'a;
/// Whether a successful result stands for its whole key (a bare read of a whole file).
pub type SupersedeComplete<'a> = dyn Fn(&Item<'_>) -> bool + 'a;

/// Reads of the same file supersede one another; see [`crate::files::read_supersede_key`].
pub fn read_key(item: &Item<'_>, call: &AssistantToolCall) -> Option<String> {
    if item.message.name.as_deref() != Some("read") || call.function_name != "read" {
        return None;
    }
    crate::files::read_supersede_key(&call.arguments_json)
}

/// A read is complete when its tool reported the whole file untruncated, or an image.
pub fn read_is_complete(item: &Item<'_>) -> bool {
    item.tool.read_complete == Some(true)
}

pub struct AgePruneConfig<'a> {
    pub protect_tokens: u32,
    pub minimum_savings: u32,
    /// Replace useless results regardless of age.
    pub prune_useless: bool,
    /// Stop scanning once the newer whole-message suffix exceeds this many tokens.
    pub cache_warm_suffix: Option<u32>,
    pub cache_lookback: Option<u32>,
    pub supersede_key: Option<&'a SupersedeKey<'a>>,
    /// Completeness proof for successful results; absent means every success is complete.
    pub supersede_complete: Option<&'a SupersedeComplete<'a>>,
}

impl Default for AgePruneConfig<'_> {
    fn default() -> Self {
        Self {
            protect_tokens: DEFAULT_PROTECT_TOKENS,
            minimum_savings: DEFAULT_MINIMUM_SAVINGS,
            prune_useless: true,
            cache_warm_suffix: None,
            cache_lookback: None,
            supersede_key: None,
            supersede_complete: None,
        }
    }
}

pub struct IncrementalPruneConfig<'a> {
    pub prune_useless: bool,
    pub suffix_token_limit: u32,
    pub idle_flush_ms: u64,
    /// Used only to test the idle gap; stamps come from the caller.
    pub now_ms: u64,
    pub cache_lookback: Option<u32>,
    pub supersede_key: Option<&'a SupersedeKey<'a>>,
    pub supersede_complete: Option<&'a SupersedeComplete<'a>>,
}

impl Default for IncrementalPruneConfig<'_> {
    fn default() -> Self {
        Self {
            prune_useless: false,
            suffix_token_limit: WARM_SUFFIX_TOKENS,
            idle_flush_ms: IDLE_FLUSH_MS,
            now_ms: 0,
            cache_lookback: None,
            supersede_key: None,
            supersede_complete: None,
        }
    }
}

#[cfg(test)]
mod tests;
