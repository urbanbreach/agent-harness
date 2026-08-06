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
#[expect(
    clippy::mod_module_files,
    reason = "task 45 requires capability_matrix/mod.rs as the public facade"
)]
pub mod capability_matrix;
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
#[expect(
    clippy::mod_module_files,
    reason = "task 44 requires contextual_tips/mod.rs as the public facade"
)]
pub mod contextual_tips;
#[expect(
    clippy::mod_module_files,
    reason = "task 30 requires dashboard/mod.rs as the public facade"
)]
pub mod dashboard;
#[expect(
    clippy::mod_module_files,
    reason = "task 35 requires dashboard_controls/mod.rs as the public facade"
)]
pub mod dashboard_controls;
#[expect(
    clippy::mod_module_files,
    reason = "task 34 requires dashboard_details/mod.rs as the public facade"
)]
pub mod dashboard_details;
#[expect(
    clippy::mod_module_files,
    reason = "task 33 requires dashboard_dispatch/mod.rs as the public facade"
)]
pub mod dashboard_dispatch;
#[expect(
    clippy::mod_module_files,
    reason = "task 36 requires dashboard_integration/mod.rs as the public facade"
)]
pub mod dashboard_integration;
#[expect(
    clippy::mod_module_files,
    reason = "task 32 requires dashboard_peek/mod.rs as the public facade"
)]
pub mod dashboard_peek;
#[expect(
    clippy::mod_module_files,
    reason = "task 31 requires dashboard_roster/mod.rs as the public facade"
)]
pub mod dashboard_roster;
pub mod design_contract;
pub mod event;
#[expect(
    clippy::mod_module_files,
    reason = "task 48 requires fidelity_config/mod.rs as the public facade"
)]
pub mod fidelity_config;
pub mod gestures;
pub mod ghost_suggestions;
#[expect(
    clippy::mod_module_files,
    reason = "task 39 requires inline_image/mod.rs as the public facade"
)]
pub mod inline_image;
pub mod input;
pub mod keybindings;
pub mod layout;
pub mod leaf_actions;
pub mod leaf_views;
#[cfg(test)]
mod lib_tests;
#[expect(
    clippy::mod_module_files,
    reason = "task 46 requires lifecycle_choreography/mod.rs as the public facade"
)]
pub mod lifecycle_choreography;
#[expect(
    clippy::mod_module_files,
    reason = "task 41 requires mermaid_worker/mod.rs as the public facade"
)]
pub mod mermaid_worker;
pub mod mouse;
pub mod overlay;
#[expect(
    clippy::mod_module_files,
    reason = "task 47 requires perf_budgets/mod.rs as the public facade"
)]
pub mod perf_budgets;
pub mod prompt_queue_actions;
pub mod render_test;
pub mod responsive;
mod runtime;
pub(crate) mod runtime_integration;
pub mod scheduling;
mod session_events;
pub mod shell_geometry;
pub mod slash;
pub mod terminal;
#[expect(
    clippy::mod_module_files,
    reason = "task 43 requires terminal_notifications/mod.rs as the public facade"
)]
pub mod terminal_notifications;
#[expect(
    clippy::mod_module_files,
    reason = "task 42 requires terminal_title/mod.rs as the public facade"
)]
pub mod terminal_title;
#[cfg(test)]
mod tests;
mod text;
pub mod theme;
#[expect(
    clippy::mod_module_files,
    reason = "task 37 requires theme_family/mod.rs as the public facade"
)]
pub mod theme_family;
pub mod theme_leaf;
#[expect(
    clippy::mod_module_files,
    reason = "theme_system preserves the richer token-driven theme contract"
)]
pub mod theme_system;
mod time_format;
#[expect(
    clippy::mod_module_files,
    reason = "task 25 requires transcript_block_viewer/mod.rs as the public facade"
)]
pub mod transcript_block_viewer;
#[expect(
    clippy::mod_module_files,
    reason = "task 24 requires transcript_blocks/mod.rs as the public facade"
)]
pub mod transcript_blocks;
#[expect(
    clippy::mod_module_files,
    reason = "task 22 requires transcript_identity/mod.rs as the public facade"
)]
pub mod transcript_identity;
#[expect(
    clippy::mod_module_files,
    reason = "task 29 requires transcript_integration/mod.rs as the public facade"
)]
pub mod transcript_integration;
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
#[expect(
    clippy::mod_module_files,
    reason = "task 40 requires video_viewer/mod.rs as the public facade"
)]
pub mod video_viewer;
#[expect(
    clippy::mod_module_files,
    reason = "task 38 requires welcome_surface/mod.rs as the public facade"
)]
pub mod welcome_surface;

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
