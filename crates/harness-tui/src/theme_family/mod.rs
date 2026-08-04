//! Theme family facade for semantic roles and switching.
//!
//! Light/dark variants, capability-aware fallback, system-preference auto
//! mode, live preview/commit/cancel, and persisted choice round-trip through
//! the TUI config contract.

pub mod auto;
pub mod fallback;
pub mod family;
pub mod persist;
pub mod preview;
pub mod roles;

// Unified facade re-exports for ergonomic single-path access.
pub use auto::{AutoDetectError, AutoMode, AutoResolver, SystemPreference};
pub use fallback::{FallbackError, FallbackLadder, ResolvedColor};
pub use family::{FamilyColor, ThemeFamily};
pub use persist::{
    deserialize_choice, serialize_choice, PersistError, PersistedTheme, ThemeChoice,
};
pub use preview::{PreviewError, PreviewState, ThemePreview};
pub use roles::{BorderRole, ColorRole, FocusRole, GlyphRole, SemanticKind, SemanticRole};

/// Resolve a full palette snapshot for a family at a given capability level.
///
/// Convenience wrapper: map every `ColorRole` through `ThemeFamily::resolve`
/// then degrade each truecolor value through `FallbackLadder::resolve` for the
/// requested capability, returning one `ResolvedColor` per role in `ColorRole::ALL`
/// order.
pub fn resolve_palette(
    family: ThemeFamily,
    level: crate::theme::ColorLevel,
) -> Vec<(ColorRole, ResolvedColor)> {
    ColorRole::ALL
        .iter()
        .map(|&role| {
            let color = family.resolve(role);
            let resolved = FallbackLadder::resolve(color.rgb(), level);
            (role, resolved)
        })
        .collect()
}
