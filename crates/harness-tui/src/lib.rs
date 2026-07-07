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
pub mod render_test;
mod runtime;
mod session_events;
#[cfg(test)]
mod tests;
mod text;
pub mod theme;
mod time_format;
pub mod ui;

pub trait UnwrapOrAbort<T> {
    fn unwrap_or_abort(self) -> T;
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "replaces .expect() which also panics; abort() kills test processes"
)]
impl<T> UnwrapOrAbort<T> for Option<T> {
    fn unwrap_or_abort(self) -> T {
        match self {
            Some(v) => v,
            None => panic!("unwrap_or_abort on None"),
        }
    }
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "replaces .expect() which also panics; abort() kills test processes"
)]
impl<T, E> UnwrapOrAbort<T> for Result<T, E> {
    fn unwrap_or_abort(self) -> T {
        match self {
            Ok(v) => v,
            Err(_) => panic!("unwrap_or_abort on Err"),
        }
    }
}

mod view_model;

pub use app::{ReviewSurface, UiIntent};
pub use keybindings::{Action, KeyMap};
pub use layout::FrameLayoutPlan;
pub use runtime::{
    close_preserved_terminal_session, run_tui, run_tui_with_options,
    set_pending_replay_launch_metadata, LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions,
};
pub use theme::{LiveShellLayout, LiveShellTokens, ShellGeometry, ShellGeometryTarget, Theme};
