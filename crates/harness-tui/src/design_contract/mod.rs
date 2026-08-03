#![allow(
    clippy::mod_module_files,
    reason = "Task 8 requires a directory facade with mod.rs"
)]

mod generated;
mod states;
mod tokens;
mod validation;
mod viewports;

pub use generated::{DESIGN_CONTRACT_SCHEMA, DESIGN_TOKENS, VIEWPORTS};
pub use states::LifecycleState;
pub use tokens::{
    BorderRole, BorderToken, BorderTokens, ColorRole, ColorToken, DesignTokens, FocusRole,
    FocusStyle, FocusStyles, GlyphRole, GlyphRoles, GlyphToken, HierarchyLevel, HierarchyToken,
    HierarchyTokens, MotionKind, MotionReplacement, MotionToken, MotionTokens, PaletteColor,
    PaletteTokens, ReducedMotionSubstitution, ReducedMotionSubstitutions, SpacingTokens,
    StateColorBinding, StateColors, TextModifier,
};
pub use validation::{validate_no_adhoc_colors_or_geometry, DesignContractValidationError};
pub use viewports::{BreakpointBand, ResponsiveBreakpoints, ViewportBreakpoint, ViewportId};
