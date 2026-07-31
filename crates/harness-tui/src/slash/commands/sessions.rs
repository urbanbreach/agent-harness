//! Owner module for the `sessions` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("sessions", "slash_sessions", &["resume", "continue"]);
