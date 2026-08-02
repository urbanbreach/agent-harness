//! Owner module for the `timestamps` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("timestamps", "slash_timestamps", &["toggle-timestamps"]);
