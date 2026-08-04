//! Aggregator for all slash command leaf definitions.
//!
//! This module collects the leaf definitions from their owner modules. It does
//! not contain command-specific logic; it only collects.

pub mod agents;
pub mod auth;
pub mod clone;
pub mod compact;
pub mod connect;
pub mod copy;
pub mod dashboard;
pub mod exit;
pub mod export;
pub mod feedback;
pub mod follow;
pub mod fork;
pub mod help;
pub mod import;
pub mod mcps;
pub mod models;
pub mod new;
pub mod rename;
pub mod sessions;
pub mod settings;
pub mod shell;
pub mod thinking;
pub mod timestamps;
pub mod toggles;
pub mod tree;
pub mod view_plan;

use crate::slash::SlashCommandLeaf;

/// Return the canonical ordered list of slash command leaf definitions.
///
/// The order matches `command_registry::SLASH_COMMANDS` exactly so the
/// integrator can swap the source without changing observable behavior.
pub fn all_commands() -> &'static [SlashCommandLeaf] {
    &[
        new::LEAF,
        sessions::LEAF,
        fork::LEAF,
        tree::LEAF,
        clone::LEAF,
        models::LEAF,
        agents::LEAF,
        mcps::LEAF,
        toggles::LEAF,
        auth::LEAF,
        connect::LEAF,
        help::LEAF,
        shell::LEAF,
        follow::LEAF,
        compact::LEAF,
        exit::LEAF,
        rename::LEAF,
        copy::LEAF,
        export::LEAF,
        feedback::LEAF,
        timestamps::LEAF,
        thinking::LEAF,
        settings::LEAF,
        view_plan::LEAF,
        dashboard::LEAF,
        import::LEAF,
    ]
}
