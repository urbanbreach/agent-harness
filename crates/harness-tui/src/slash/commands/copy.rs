//! Owner module for the `copy` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("copy", "slash_copy", &[]);
