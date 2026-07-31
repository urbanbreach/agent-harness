//! Owner module for the `compact` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("compact", "slash_compact", &["summarize"]);
