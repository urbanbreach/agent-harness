//! Fixed-tick animation evidence capture for A-ANIMATION infrastructure.
//!
//! Captures deterministic terminal-cell frame sequences by advancing
//! [`AppState`] animation ticks (and optional [`FakeClock`]) without wall-clock
//! time. This is foundation only — not an A-ANIMATION parity pass.

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use harness_core::clock::{Clock, FakeClock};
use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::render_test::render_to_string;
use crate::ui;

/// Schema id written into every sequence artifact.
pub const ANIMATION_FRAME_SEQUENCE_SCHEMA: &str = "animation-frame-sequence-v1";

/// Default FakeClock step (ms) recorded per animation tick when not overridden.
pub const DEFAULT_TICK_MS: u64 = 1_000 / 30;

/// Capture plan for a fixed-tick animation sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedTickPlan {
    /// Stable surface id (e.g. `streaming-wait-spinner`).
    pub surface_id: String,
    pub width: u16,
    pub height: u16,
    /// Number of frames to capture (including the initial pre-advance frame).
    pub frame_count: usize,
    /// Monotonic ms advanced on the FakeClock after each captured frame except the last.
    pub tick_ms: u64,
}

impl FixedTickPlan {
    pub fn new(surface_id: impl Into<String>, width: u16, height: u16, frame_count: usize) -> Self {
        Self {
            surface_id: surface_id.into(),
            width,
            height,
            frame_count,
            tick_ms: DEFAULT_TICK_MS,
        }
    }

    pub fn with_tick_ms(mut self, tick_ms: u64) -> Self {
        self.tick_ms = tick_ms;
        self
    }
}

/// One captured terminal-cell frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationFrame {
    pub index: usize,
    pub mono_ms: u64,
    pub animation_phase: usize,
    /// Full terminal cell grid as text (row-major, `\n`-joined).
    pub cells: String,
}

/// Deterministic fixed-tick frame sequence artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationFrameSequence {
    pub schema_version: String,
    pub surface_id: String,
    pub width: u16,
    pub height: u16,
    pub tick_ms: u64,
    pub frames: Vec<AnimationFrame>,
}

/// Fail-closed capture / determinism errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimationEvidenceError {
    EmptyPlan,
    NondeterministicFrame { index: usize, detail: String },
    NondeterministicSequence { detail: String },
    Io(String),
    Serialize(String),
}

impl std::fmt::Display for AnimationEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPlan => write!(f, "fixed-tick plan requires frame_count >= 1"),
            Self::NondeterministicFrame { index, detail } => {
                write!(f, "nondeterministic frame {index}: {detail}")
            }
            Self::NondeterministicSequence { detail } => {
                write!(f, "nondeterministic sequence: {detail}")
            }
            Self::Io(msg) => write!(f, "animation evidence io: {msg}"),
            Self::Serialize(msg) => write!(f, "animation evidence serialize: {msg}"),
        }
    }
}

impl std::error::Error for AnimationEvidenceError {}

impl From<io::Error> for AnimationEvidenceError {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AnimationEvidenceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialize(value.to_string())
    }
}

/// Render the app into a terminal-cell string for the plan viewport.
pub fn render_app_cells(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

/// Capture one frame, fail-closed if a second paint of the same state differs.
pub fn capture_stable_frame(
    app: &AppState,
    width: u16,
    height: u16,
    index: usize,
    mono_ms: u64,
) -> Result<AnimationFrame, AnimationEvidenceError> {
    let first = render_app_cells(app, width, height);
    let second = render_app_cells(app, width, height);
    if first != second {
        return Err(AnimationEvidenceError::NondeterministicFrame {
            index,
            detail: "double-paint of the same AppState produced different cell grids".to_string(),
        });
    }
    Ok(AnimationFrame {
        index,
        mono_ms,
        animation_phase: app.animation_phase_for_evidence(),
        cells: first,
    })
}

/// Advance FakeClock and AppState animation phase by one fixed tick.
pub fn advance_fixed_tick(app: &mut AppState, clock: &FakeClock, tick_ms: u64) {
    clock.advance(tick_ms);
    app.advance_animation_tick_by_for_evidence(Duration::from_millis(tick_ms));
}

/// Capture a fixed-tick frame sequence.
///
/// Frame 0 is the current state. Each subsequent frame advances FakeClock by
/// `plan.tick_ms` and AppState animation phase by one tick, then paints.
pub fn capture_fixed_tick_sequence(
    app: &mut AppState,
    clock: &FakeClock,
    plan: &FixedTickPlan,
) -> Result<AnimationFrameSequence, AnimationEvidenceError> {
    if plan.frame_count == 0 {
        return Err(AnimationEvidenceError::EmptyPlan);
    }

    app.freeze_now_for_animation_evidence();
    let mut frames = Vec::with_capacity(plan.frame_count);
    for index in 0..plan.frame_count {
        if index > 0 {
            advance_fixed_tick(app, clock, plan.tick_ms);
        }
        frames.push(capture_stable_frame(
            app,
            plan.width,
            plan.height,
            index,
            clock.mono_ms(),
        )?);
    }

    Ok(AnimationFrameSequence {
        schema_version: ANIMATION_FRAME_SEQUENCE_SCHEMA.to_string(),
        surface_id: plan.surface_id.clone(),
        width: plan.width,
        height: plan.height,
        tick_ms: plan.tick_ms,
        frames,
    })
}

/// Fail closed when two sequences (same plan, independently prepared apps) differ.
pub fn assert_sequences_equal(
    left: &AnimationFrameSequence,
    right: &AnimationFrameSequence,
) -> Result<(), AnimationEvidenceError> {
    if left == right {
        return Ok(());
    }
    if left.frames.len() != right.frames.len() {
        return Err(AnimationEvidenceError::NondeterministicSequence {
            detail: format!(
                "frame count {} != {}",
                left.frames.len(),
                right.frames.len()
            ),
        });
    }
    for (left_frame, right_frame) in left.frames.iter().zip(right.frames.iter()) {
        if left_frame != right_frame {
            return Err(AnimationEvidenceError::NondeterministicSequence {
                detail: format!(
                    "frame {} differs (phase {} vs {}, mono_ms {} vs {})",
                    left_frame.index,
                    left_frame.animation_phase,
                    right_frame.animation_phase,
                    left_frame.mono_ms,
                    right_frame.mono_ms
                ),
            });
        }
    }
    Err(AnimationEvidenceError::NondeterministicSequence {
        detail: "sequence metadata differs".to_string(),
    })
}

/// Write a sequence artifact as pretty JSON under `path`.
pub fn write_sequence_artifact(
    path: &Path,
    sequence: &AnimationFrameSequence,
) -> Result<(), AnimationEvidenceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(sequence)?;
    fs::write(path, body)?;
    Ok(())
}

/// Load a sequence artifact written by [`write_sequence_artifact`].
pub fn read_sequence_artifact(
    path: &Path,
) -> Result<AnimationFrameSequence, AnimationEvidenceError> {
    let body = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&body)?)
}

/// Collect spinner glyph characters present in a frame cell grid (braille spinner set).
pub fn spinner_glyphs_in_cells(cells: &str) -> Vec<char> {
    const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
    cells.chars().filter(|ch| SPINNER.contains(ch)).collect()
}
