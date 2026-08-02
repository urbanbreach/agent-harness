//! Owner module for the `help` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("help", "help", &[]);
