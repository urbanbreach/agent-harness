//! TUI reference-parity infrastructure (semantic cells L2 / A-CELLS).
//!
//! Capture and exact-compare only. No SSIM, no production golden wiring.

pub mod cells;
pub mod compare;
pub mod frame_io;
pub mod vt100_adapter;

pub use cells::{
    CellModifiers, CursorShape, CursorState, ResolvedRgb, SemanticCell, SemanticFrame, DEFAULT_BG,
    DEFAULT_FG, SEMANTIC_FRAME_SCHEMA_VERSION,
};
pub use compare::{
    compare_frames, is_settled, CellDiff, CompareResult, IdentityFieldMask, IdentityMaskRegistry,
    StableFrameTracker, SETTLE_IDENTICAL_FRAMES,
};
pub use frame_io::ParityCellError;
pub use vt100_adapter::semantic_frame_from_vt100_screen;
