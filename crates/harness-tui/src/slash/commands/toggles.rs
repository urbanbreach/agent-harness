//! Owner module for the `toggles` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("toggles", "toggles", &[]);
