//! Overlay leaf view.

/// Deterministic view state for an overlay surface.
///
/// No app-state or registry dependency — a plain `Copy` value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverlayLeafView {
    pub title: &'static str,
    pub visible: bool,
}

impl OverlayLeafView {
    pub const fn new(title: &'static str, visible: bool) -> Self {
        Self { title, visible }
    }
}
