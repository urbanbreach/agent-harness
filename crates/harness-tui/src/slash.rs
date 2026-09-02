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
    pub takes_args: bool,
    pub args_required: bool,
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
            takes_args: false,
            args_required: false,
        }
    }

    pub const fn with_args(mut self, args_required: bool) -> Self {
        self.takes_args = true;
        self.args_required = args_required;
        self
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
        if self.args_required && !self.takes_args {
            return Err(SlashCommandLeafError::ArgsRequiredWithoutArgs);
        }
        Ok(())
    }
}

/// Validation error for a [`SlashCommandLeaf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandLeafError {
    EmptyId,
    EmptyMetadataId,
    ArgsRequiredWithoutArgs,
}

#[cfg(test)]
mod tests {
    use super::{SlashCommandLeaf, SlashCommandLeafError};

    #[test]
    fn default_leaf_has_no_args() {
        // Given a leaf created with the existing constructor.
        let leaf = SlashCommandLeaf::new("help", "help", &[]);

        // When its argument metadata is inspected.
        // Then it defaults to accepting no arguments.
        assert!(!leaf.takes_args);
        assert!(!leaf.args_required);
    }

    #[test]
    fn optional_args_are_metadata() {
        // Given a leaf configured with optional arguments.
        let leaf = SlashCommandLeaf::new("model", "model", &[]).with_args(false);

        // When its argument metadata is inspected.
        // Then it accepts arguments without requiring them.
        assert!(leaf.takes_args);
        assert!(!leaf.args_required);
        assert_eq!(leaf.validate(), Ok(()));
    }

    #[test]
    fn required_args_are_metadata() {
        // Given a leaf configured with required arguments.
        let leaf = SlashCommandLeaf::new("resume", "resume", &[]).with_args(true);

        // When it is validated.
        // Then the required argument invariant is valid.
        assert!(leaf.takes_args);
        assert!(leaf.args_required);
        assert_eq!(leaf.validate(), Ok(()));
    }

    #[test]
    fn required_args_without_argument_support_are_invalid() {
        // Given a leaf whose metadata requires arguments without accepting them.
        let leaf = SlashCommandLeaf {
            args_required: true,
            ..SlashCommandLeaf::new("help", "help", &[])
        };

        // When it is validated.
        let result = leaf.validate();

        // Then validation returns the typed invariant error.
        assert_eq!(result, Err(SlashCommandLeafError::ArgsRequiredWithoutArgs));
    }
}
