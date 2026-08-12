//! TUI reference-parity infrastructure (semantic cells L2 / A-CELLS).
//!
//! Capture and exact-compare only. No SSIM, no production golden wiring.

pub mod artifact_schema;
pub mod catalog;
pub mod cells;
pub mod compare;
pub mod frame_io;
pub mod identity;
pub mod motion;
pub mod provenance;
pub mod status;
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
pub use identity::{frame_line, IdentityKind, IdentityReplacement, IdentitySubstitution};
pub use motion::{
    compare_motion_traces, compare_ordered_motion_traces, validate_motion_trace,
    validate_motion_trace_with_families, FrameTrace, MotionDefect, MotionFamily, MotionPhase,
    TickFrame, TraceIdentity, TraceSource,
};
pub use provenance::{
    compare_frames_with_provenance, validate_capture_provenance, validate_no_self_comparison,
    CaptureProvenance, CaptureSource, ProvenanceContext, ProvenanceError, Viewport,
};
pub use status::ProofDimension;
pub use vt100_adapter::semantic_frame_from_vt100_screen;
