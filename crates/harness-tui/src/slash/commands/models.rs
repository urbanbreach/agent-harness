//! Owner module for the `models` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("models", "switch_model", &["mo"]);
