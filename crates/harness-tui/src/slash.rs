//! Slash command leaf definitions extracted from `command_registry.rs`.
//!
//! Each command has its own owner module under `commands/`. The aggregator
//! in `commands/mod.rs` collects them into the canonical ordered list of 26.
//! This module defines the shared `SlashCommandLeaf` value type; it does not
//! contain command-specific logic.

pub mod commands;

pub use commands::all_commands;

/// A single slash command leaf definition.
///
/// Mirrors `harness_tui::keybindings::SlashCommand` but lives in its own
/// module tree so later TUI shards can depend on the leaf contract without
/// pulling in the full keybinding registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandLeaf {
    pub id: &'static str,
    pub metadata_id: &'static str,
    pub aliases: &'static [&'static str],
}

impl SlashCommandLeaf {
    pub const fn new(
        id: &'static str,
        metadata_id: &'static str,
        aliases: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            metadata_id,
            aliases,
        }
    }

    /// Validate that this leaf has non-empty id and metadata_id.
    ///
    /// Empty definitions are rejected so the aggregator can never silently
    /// include a placeholder entry.
    pub fn validate(&self) -> Result<(), SlashCommandLeafError> {
        if self.id.is_empty() {
            return Err(SlashCommandLeafError::EmptyId);
        }
        if self.metadata_id.is_empty() {
            return Err(SlashCommandLeafError::EmptyMetadataId);
        }
        Ok(())
    }
}

/// Validation error for a [`SlashCommandLeaf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandLeafError {
    EmptyId,
    EmptyMetadataId,
}
