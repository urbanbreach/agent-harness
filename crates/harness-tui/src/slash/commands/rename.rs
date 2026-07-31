//! Owner module for the `rename` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("rename", "slash_rename", &[]);
