#![allow(
    clippy::module_name_repetitions,
    clippy::mod_module_files,
    reason = "the facade intentionally names the public shell geometry types"
)]

mod cursor_placement;
mod hit_map;
mod regions;
mod responsive;

pub use cursor_placement::{cursor_for, CursorPlacement};
pub use hit_map::{FocusTarget, HitMap, HitRegion, HitRegionState, HitTarget};
pub use regions::{
    identity_rectangles, IdentityCopy, IdentityRectangles, ShellRegions, ShellState,
    ALL_SHELL_STATES,
};
pub use responsive::{layout_for, layout_for_rect};
