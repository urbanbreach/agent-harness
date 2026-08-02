//! Trust folder prompt view model helpers.
//!
//! Provides grant/deny labels, key hints, and state inspection for the
//! folder-trust prompt overlay. Used by `ui_lifecycle.rs` for rendering
//! and contract verification.
//!
//! This is a leaf module declared via `#[path]` in `ui_lifecycle.rs`.

use crate::app::AppState;

/// Trust prompt action labels shown in the overlay footer.
pub(crate) const ALLOW_LABEL: &str = "[y] Allow";
pub(crate) const DENY_LABEL: &str = "[n] Deny";
pub(crate) const CANCEL_LABEL: &str = "[Esc] Cancel";

/// Trust prompt title shown in the overlay header.
pub(crate) const TRUST_PROMPT_TITLE: &str = "Folder Trust";

/// Whether the trust folder prompt is currently visible.
pub(crate) fn trust_prompt_visible(app: &AppState) -> bool {
    app.trust_folder_prompt_visible
}

/// The trust prompt body text explaining the choice.
pub(crate) fn trust_prompt_body_lines() -> [&'static str; 3] {
    [
        "Repository-local executables require folder trust.",
        "Allow: permit repo-local binary execution",
        "Deny:  block repo-local binary execution",
    ]
}

/// The footer key hints for the trust prompt.
pub(crate) fn trust_prompt_footer_hints() -> [&'static str; 3] {
    [ALLOW_LABEL, DENY_LABEL, CANCEL_LABEL]
}
