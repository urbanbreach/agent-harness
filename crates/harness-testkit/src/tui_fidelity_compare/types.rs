pub use super::cells::{compare_cells, CellDiffRecord, CellSnapshot, FocusState, ZOrderEntry};
pub use super::error::ComparatorError;
pub use super::hashing::{
    hash_artifact_paths, hash_artifacts, verify_freshness, ArtifactHashes, ArtifactInputs,
    ArtifactPaths, StaleArtifact,
};
pub use super::motion::{
    compare_motion, MotionIssue, MotionTimingEvent, MotionTimingKind, MotionTrace,
    ACTION_BLINK_TICKS, CADENCE_HZ, CADENCE_MAX_GAP_MS, CADENCE_PERIOD_MS, FLUSH_TOLERANCE_MS,
    FRAME_TIMESTAMP_TOLERANCE_MS, GESTURE_BOUNDARY_MS, PASTE_FIRST_BYTE_MS, PASTE_SETTLED_MS,
    RESIZE_DEBOUNCE_MS, TITLE_HOLD_TICKS,
};
pub use super::pixels::{
    compare_png, compare_png_bytes, IdentityPixelSpan, PixelDiffRecord, PixelRect,
};
pub use super::secret_scan::{
    scan_artifacts, scan_directory, ArtifactRoots, SecretFinding, SecretScanResult,
};
pub use super::self_compare::reject_self_comparison;
pub use super::timing::{compare_timing, LatencyDistribution, TimingDefect, TimingTrace};

pub type CompareResult = Result<(), ComparatorError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareVerdict {
    pub cells: CompareResult,
    pub pixels: CompareResult,
    pub motion: CompareResult,
    pub timing: CompareResult,
    pub hashing: CompareResult,
    pub self_compare: CompareResult,
    pub secret_scan: CompareResult,
}

impl CompareVerdict {
    pub fn all_passed(&self) -> bool {
        self.cells.is_ok()
            && self.pixels.is_ok()
            && self.motion.is_ok()
            && self.timing.is_ok()
            && self.hashing.is_ok()
            && self.self_compare.is_ok()
            && self.secret_scan.is_ok()
    }
}
