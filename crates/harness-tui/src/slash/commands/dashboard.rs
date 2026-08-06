//! Owner module for the `dashboard` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("dashboard", "open_status_dialog", &["status"]);
