//! Owner module for the `mcps` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("mcps", "toggles", &[]);
