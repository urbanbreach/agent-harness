//! First-prompt composer focus view model helpers.
//!
//! Provides composer focus state inspection and type-to-dismiss behavior
//! verification for the startup first-prompt journey. Used by
//! `ui_lifecycle.rs` for rendering and contract verification.
//!
//! This is a leaf module declared via `#[path]` in `ui_lifecycle.rs`.

use crate::app::AppState;

/// Whether the welcome panel should be visible (prompt buffer is empty).
pub(crate) fn welcome_panel_visible(app: &AppState) -> bool {
    app.welcome_visible()
}

/// Whether the composer has focus on startup (always true in startup mode).
pub(crate) fn composer_has_focus(app: &AppState) -> bool {
    app.startup_shell_visible()
}

/// The typed text currently in the composer buffer.
pub(crate) fn composer_text(app: &AppState) -> &str {
    &app.composer.prompt_buffer
}

/// The cursor position (in chars, not bytes) in the composer buffer.
pub(crate) fn composer_cursor(app: &AppState) -> usize {
    app.composer.prompt_cursor
}

/// Whether typing has dismissed the welcome panel.
/// Returns true when the prompt buffer is non-empty (welcome was dismissed by typing).
pub(crate) fn welcome_dismissed_by_typing(app: &AppState) -> bool {
    app.welcome_dismissed()
}
