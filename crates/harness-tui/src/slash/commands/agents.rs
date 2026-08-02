//! Owner module for the `agents` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("agents", "switch_model", &[]);
