//! Welcome panel view model helpers.
//!
//! Provides identity, version, changelog, local notices, and action rows
//! for the startup welcome panel. Used by `ui_lifecycle.rs` to render the
//! bordered welcome panel and compact welcome body.
//!
//! This is a leaf module declared via `#[path]` in `ui_lifecycle.rs`.

use crate::app::AppState;

/// Welcome panel identity: title + version.
pub(crate) struct WelcomeIdentity {
    pub title: &'static str,
    pub version: &'static str,
}

/// A local notice derived from the app's environment state.
pub(crate) struct LocalNotice {
    pub text: String,
}

/// Derive the welcome identity (title + version) from the theme and crate version.
pub(crate) fn welcome_identity(title: &'static str) -> WelcomeIdentity {
    WelcomeIdentity {
        title,
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// Derive local notices from the app state.
///
/// Returns notices for environment-specific issues like missing provider,
/// missing auth, or other startup warnings. When there are no notices,
/// returns an empty vector.
pub(crate) fn local_notices(app: &AppState) -> Vec<LocalNotice> {
    let mut notices = Vec::new();

    if let Some(banner) = &app.status_banner {
        if !banner.is_empty() {
            notices.push(LocalNotice {
                text: banner.clone(),
            });
        }
    }

    notices
}

/// Whether the welcome panel should show a local notices section.
pub(crate) fn has_local_notices(app: &AppState) -> bool {
    !local_notices(app).is_empty()
}

/// The canonical changelog bullets.
pub(crate) fn changelog_bullets() -> [&'static str; 3] {
    [
        "Subagent spawning is dramatically faster when you have many sessio…",
        "/session-info now lets you click any row to copy its value, with h…",
        "TUI rendering now automatically matches high-refresh displays (120…",
    ]
}
