//! Owner module for the `fork` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("fork", "slash_fork", &[]);
