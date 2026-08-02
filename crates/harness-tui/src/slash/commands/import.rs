//! Owner module for the `import` slash command leaf.

use crate::slash::SlashCommandLeaf;

pub const LEAF: SlashCommandLeaf =
    SlashCommandLeaf::new("import", "slash_import", &["import-session"]);
