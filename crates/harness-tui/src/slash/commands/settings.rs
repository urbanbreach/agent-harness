//! Owner module for the `settings` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("settings", "open_settings", &[]);
