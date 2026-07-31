//! Owner module for the `clone` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("clone", "slash_clone", &[]);
