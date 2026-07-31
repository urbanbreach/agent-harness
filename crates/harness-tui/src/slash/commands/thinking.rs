//! Owner module for the `thinking` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("thinking", "slash_thinking", &["toggle-thinking"]);
