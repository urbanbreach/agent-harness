//! Ratatui shell for startup, live, and replay workflows.
//!
//! Keep TUI orchestration here, route state derivation through `app`, and keep
//! layout/theme contracts centralized in their dedicated modules rather than in
//! ad hoc render helpers.

pub mod app;
mod clipboard;
pub mod event;
pub mod keybindings;
pub mod layout;
#[cfg(test)]
mod lib_tests;
pub mod overlay;
mod runtime;
#[cfg(test)]
mod tests;
pub mod theme;
pub mod ui;
mod view_model;

pub use app::{ReviewSurface, UiIntent};
pub use keybindings::{Action, KeyMap};
pub use layout::FrameLayoutPlan;
pub use runtime::{
    close_preserved_terminal_session, load_events_from_run_dir, run_tui, run_tui_with_options,
    set_pending_replay_launch_metadata, LiveUpdate, TuiMode, TuiOptions,
};
pub use theme::{LiveShellLayout, LiveShellTokens, ShellGeometry, ShellGeometryTarget, Theme};
