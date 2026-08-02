//! Owner module for the `view-plan` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("view-plan", "open_view_plan", &["view_plan"]);
