//! Owner module for the `follow` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("follow", "toggle_follow", &[]);
