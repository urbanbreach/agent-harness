//! Ratatui shell for startup, live, and replay workflows.
//!
//! Keep TUI orchestration here, route state derivation through `app`, and keep
//! layout/theme contracts centralized in their dedicated modules rather than in
//! ad hoc render helpers.

pub mod animation_evidence;
pub mod app;
#[expect(
    clippy::mod_module_files,
    reason = "task 18 requires attachment_lifecycle/mod.rs as the public facade"
)]
pub mod attachment_lifecycle;
mod clipboard;
pub mod clipboard_leaf;
#[expect(
    clippy::mod_module_files,
    reason = "completion_controller is a focused public facade"
)]
pub mod completion_controller;
#[expect(
    clippy::mod_module_files,
    reason = "task 14 requires composer_atoms/mod.rs as the public facade"
)]
pub mod composer_atoms;
#[expect(
    clippy::mod_module_files,
    reason = "task 15 requires composer_editing/mod.rs as the public facade"
)]
pub mod composer_editing;
pub mod composer_integration;
pub mod design_contract;
pub mod event;
pub mod gestures;
pub mod ghost_suggestions;
pub mod input;
pub mod keybindings;
pub mod layout;
pub mod leaf_actions;
pub mod leaf_views;
#[cfg(test)]
mod lib_tests;
pub mod mouse;
pub mod overlay;
pub mod prompt_queue_actions;
pub mod render_test;
pub mod responsive;
mod runtime;
pub mod scheduling;
mod session_events;
pub mod shell_geometry;
pub mod slash;
pub mod terminal;
#[cfg(test)]
mod tests;
mod text;
pub mod theme;
pub mod theme_leaf;
mod time_format;
#[expect(
    clippy::mod_module_files,
    reason = "task 24 requires transcript_blocks/mod.rs as the public facade"
)]
pub mod transcript_blocks;
#[expect(
    clippy::mod_module_files,
    reason = "task 25 requires transcript_block_viewer/mod.rs as the public facade"
)]
pub mod transcript_block_viewer;
#[expect(
    clippy::mod_module_files,
    reason = "task 22 requires transcript_identity/mod.rs as the public facade"
)]
pub mod transcript_identity;
#[expect(
    clippy::mod_module_files,
    reason = "task 26 requires transcript_pager/mod.rs as the public facade"
)]
pub mod transcript_pager;
#[expect(
    clippy::mod_module_files,
    reason = "task 28 requires transcript_scroll/mod.rs as the public facade"
)]
pub mod transcript_scroll;
pub mod transcript_selection;
pub mod transcript_timeline;
pub mod ui;

pub use harness_core::UnwrapOrAbort;

mod view_model;

pub use app::notifications;
pub use app::terminal_diagnostics;
pub use app::theme_preview;
pub use app::tips;
pub use app::{ReviewSurface, UiIntent};
pub use keybindings::{Action, KeyMap};
pub use layout::FrameLayoutPlan;
pub use runtime::{
    close_preserved_terminal_session, run_tui, run_tui_with_options,
    set_pending_replay_launch_metadata, LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions,
};
pub use theme::{LiveShellLayout, LiveShellTokens, ShellGeometry, ShellGeometryTarget, Theme};
