//! Owner module for the `new` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("new", "slash_new", &["clear"]);
