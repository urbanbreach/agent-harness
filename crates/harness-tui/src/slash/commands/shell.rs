//! Owner module for the `shell` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("shell", "close_review_surface", &["session-shell"]);
