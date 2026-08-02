//! Owner module for the `exit` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("exit", "quit", &["quit", "q"]);
