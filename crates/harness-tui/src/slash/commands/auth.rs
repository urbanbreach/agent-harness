//! Owner module for the `auth` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf = SlashCommandLeaf::new("auth", "auth", &["login"]);
