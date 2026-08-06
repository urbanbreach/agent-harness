#![expect(
    clippy::mod_module_files,
    reason = "Task 37 requires the conventional theme_family directory module root"
)]

mod bindings;
mod focus;
mod glyphs;
mod palette;
mod resolution;

pub mod auto;
pub mod fallback;
pub mod family;
pub mod persist;
pub mod preview;
pub mod roles;

pub use auto::{SystemAppearance, ThemeChoice, ThemeEnvironment, detect_system_appearance};
pub use fallback::{ColorLevel, FALLBACK_LADDER, ResolvedTheme};
pub use family::ThemeFamily;
pub use persist::{TUI_THEME_KEY, ThemeConfigError, load_theme_choice, store_theme_choice};
pub use preview::{ThemePreviewState, ThemePreviewStatus};
pub use roles::{
    BorderPalette, BorderRole, DiffColors, FocusPalette, FocusRole, FocusStyle, GlyphPalette,
    GlyphRole, LifecycleColors, LifecyclePalette, LifecycleState, MediaColors, Palette,
    PaletteRole, PermissionColors, SelectionColors, SemanticThemeColors, ToolColors,
};
