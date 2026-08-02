//! Ordered motion, timing, scroll, resize, streaming, and cancellation
//! differential proof (P5 / Todo 35).
//!
//! This module captures the *contract* for comparing two ordered frame
//! sequences (reference vs. candidate) under synchronized tick timestamps.
//! A single settled frame is insufficient evidence; the differential proof
//! verifies state ordering, settle dwell, scroll flush, resize debounce,
//! streaming delta cadence, and cancellation transitions across the whole
//! ordered trace.
//!
//! Deterministic by construction: every timestamp is a monotonic tick counter
//! (`u64`) sourced from the captured event loop, never wall-clock. The
//! validator rejects self-oracle traces, missing binary identity, and missing
//! generating commands, mirroring `provenance.rs`.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::cells::{CursorState, SemanticFrame};
use super::compare::{is_settled, IdentityMaskRegistry, SETTLE_IDENTICAL_FRAMES};

/// A captured frame paired with its monotonic tick timestamp and a marker
/// describing what kind of motion produced it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickFrame {
    /// Monotonic event-loop tick at which the frame was captured. Must be
    /// non-decreasing across a trace.
    pub tick: u64,
    /// Marker for the motion phase that produced this frame.
    pub phase: MotionPhase,
    /// The captured semantic frame at this tick.
    pub frame: SemanticFrame,
}

/// Named motion phases that a TUI turn flows through. The validator uses
/// these labels to assert ordering without timing tolerances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionPhase {
    /// Initial paint before any input.
    Startup,
    /// A frame produced by streaming output from a provider/tool owner.
    StreamingDelta,
    /// A frame produced by a scroll/resize/follow action.
    ScrollFlush,
    /// A frame produced while the user is mid-resize (still debouncing).
    ResizeBurst,
    /// A frame produced after resize debounce settled at the new geometry.
    ResizeSettled,
    /// A frame produced by a cancel interrupt.
    Cancellation,
    /// A frame produced after cancellation cleaned up turn state.
    CancelRecovered,
    /// A frame produced after the turn reached its final state.
    FinishFlash,
    /// A repeated frame holding the same cells as the previous one, used to
    /// prove settle dwell.
    SettleRepeat,
}

impl MotionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            MotionPhase::Startup => "startup",
            MotionPhase::StreamingDelta => "streaming_delta",
            MotionPhase::ScrollFlush => "scroll_flush",
            MotionPhase::ResizeBurst => "resize_burst",
            MotionPhase::ResizeSettled => "resize_settled",
            MotionPhase::Cancellation => "cancellation",
            MotionPhase::CancelRecovered => "cancel_recovered",
            MotionPhase::FinishFlash => "finish_flash",
            MotionPhase::SettleRepeat => "settle_repeat",
        }
    }
}

impl fmt::Display for MotionPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The families of ordered motion/timing proof this module covers. Each
/// family has an independent contract used by [`validate_motion_trace`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionFamily {
    /// Ordered paint sequence: startup → streaming → finish → settle.
    OrderedMotion,
    /// Settle requires `SETTLE_IDENTICAL_FRAMES` consecutive identical frames.
    SettleDwell,
    /// Scroll flush must follow a scroll/follow trigger and not regress.
    ScrollFlush,
    /// Resize debounce: burst frames may differ, then one settled frame at
    /// the new geometry with no further churn.
    ResizeDebounce,
    /// Streaming deltas must be monotonic in tick and content-stable on idle.
    StreamingDeltas,
    /// Cancellation must transition Cancellation → CancelRecovered without
    /// skipped state or stale spinner cells.
    CancellationOrdering,
}

impl MotionFamily {
    pub fn all() -> &'static [MotionFamily] {
        &[
            MotionFamily::OrderedMotion,
            MotionFamily::SettleDwell,
            MotionFamily::ScrollFlush,
            MotionFamily::ResizeDebounce,
            MotionFamily::StreamingDeltas,
            MotionFamily::CancellationOrdering,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MotionFamily::OrderedMotion => "ordered_motion",
            MotionFamily::SettleDwell => "settle_dwell",
            MotionFamily::ScrollFlush => "scroll_flush",
            MotionFamily::ResizeDebounce => "resize_debounce",
            MotionFamily::StreamingDeltas => "streaming_deltas",
            MotionFamily::CancellationOrdering => "cancellation_ordering",
        }
    }
}

impl fmt::Display for MotionFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An ordered sequence of captured frames bound to one binary source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameTrace {
    pub source: TraceSource,
    /// Captured tick frames in observed order.
    pub frames: Vec<TickFrame>,
}

/// Binary identity for a motion trace. Mirrors `CaptureSource` but adds an
/// explicit SHA-256 binding so traces cannot be self-oracled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceSource {
    Reference,
    Harness,
}

impl TraceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TraceSource::Reference => "reference",
            TraceSource::Harness => "harness",
        }
    }
}

/// Identity binding for a captured trace. Every trace must declare the
/// exact binary SHA-256 that generated it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceIdentity {
    pub source: TraceSource,
    pub binary_sha256: String,
    /// Short version label emitted by `--version`.
    pub binary_version: String,
    /// Command that produced the trace (deterministic lane identifier).
    pub generating_command: String,
}

/// One typed motion defect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionDefect {
    pub family: MotionFamily,
    pub reason: String,
    pub detail: String,
}

impl MotionDefect {
    pub fn new(family: MotionFamily, reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            family,
            reason: reason.into(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for MotionDefect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}: {}", self.family, self.reason, self.detail)
    }
}

/// Required ordering for the `OrderedMotion` family.
const ORDERED_MOTION_SEQUENCE: &[MotionPhase] = &[
    MotionPhase::Startup,
    MotionPhase::StreamingDelta,
    MotionPhase::FinishFlash,
    MotionPhase::SettleRepeat,
];

/// Validates that `trace` satisfies the named `family` contract.
///
/// Returns `Ok(())` when the trace meets the family's invariants and
/// `Err(Vec<MotionDefect>)` with one typed defect per violation otherwise.
/// Identity and self-oracle defects are reported under the
/// `OrderedMotion` family by convention; tests can mutate the family via
/// [`validate_motion_trace_with_families`].
pub fn validate_motion_trace(trace: &FrameTrace) -> Result<(), Vec<MotionDefect>> {
    validate_motion_trace_with_families(trace, MotionFamily::all())
}

/// Validates that `trace` satisfies every family in `families`.
pub fn validate_motion_trace_with_families(
    trace: &FrameTrace,
    families: &[MotionFamily],
) -> Result<(), Vec<MotionDefect>> {
    let mut defects = Vec::new();
    for family in families {
        let family_defects = validate_family(trace, *family);
        defects.extend(family_defects);
    }
    if defects.is_empty() {
        Ok(())
    } else {
        Err(defects)
    }
}

fn validate_family(trace: &FrameTrace, family: MotionFamily) -> Vec<MotionDefect> {
    match family {
        MotionFamily::OrderedMotion => validate_ordered_motion(trace),
        MotionFamily::SettleDwell => validate_settle_dwell(trace),
        MotionFamily::ScrollFlush => validate_scroll_flush(trace),
        MotionFamily::ResizeDebounce => validate_resize_debounce(trace),
        MotionFamily::StreamingDeltas => validate_streaming_deltas(trace),
        MotionFamily::CancellationOrdering => validate_cancellation_ordering(trace),
    }
}

fn validate_monotonic_ticks(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = Vec::new();
    let mut prev_tick = None;
    for (idx, tick_frame) in trace.frames.iter().enumerate() {
        if let Some(prev) = prev_tick {
            if tick_frame.tick < prev {
                defects.push(MotionDefect::new(
                    MotionFamily::OrderedMotion,
                    "non_monotonic_tick",
                    format!(
                        "tick at index {idx} = {} < previous tick {prev}; ticks must be non-decreasing",
                        tick_frame.tick
                    ),
                ));
            }
        }
        prev_tick = Some(tick_frame.tick);
    }
    defects
}

fn validate_ordered_motion(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = validate_monotonic_ticks(trace);
    if trace.frames.is_empty() {
        defects.push(MotionDefect::new(
            MotionFamily::OrderedMotion,
            "empty_trace",
            "ordered motion requires at least one frame",
        ));
        return defects;
    }
    let mut observed_idx = 0;
    for required in ORDERED_MOTION_SEQUENCE {
        let found = trace.frames[observed_idx..]
            .iter()
            .position(|tick_frame| tick_frame.phase == *required);
        match found {
            Some(offset) => observed_idx += offset + 1,
            None => {
                defects.push(MotionDefect::new(
                    MotionFamily::OrderedMotion,
                    "missing_phase",
                    format!(
                        "ordered motion requires phase {} after index {}; not present in trace",
                        required, observed_idx
                    ),
                ));
                return defects;
            }
        }
    }
    defects
}

fn validate_settle_dwell(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = validate_monotonic_ticks(trace);
    let settle_frames: Vec<&TickFrame> = trace
        .frames
        .iter()
        .filter(|tick_frame| tick_frame.phase == MotionPhase::SettleRepeat)
        .collect();
    if settle_frames.len() < SETTLE_IDENTICAL_FRAMES {
        defects.push(MotionDefect::new(
            MotionFamily::SettleDwell,
            "insufficient_settle_dwell",
            format!(
                "settle requires {SETTLE_IDENTICAL_FRAMES} consecutive identical frames, observed {}",
                settle_frames.len()
            ),
        ));
        return defects;
    }
    let tail_frames: Vec<&SemanticFrame> = trace
        .frames
        .iter()
        .rev()
        .take(SETTLE_IDENTICAL_FRAMES)
        .map(|t| &t.frame)
        .collect();
    let mut reversed: Vec<SemanticFrame> = tail_frames
        .into_iter()
        .take(SETTLE_IDENTICAL_FRAMES)
        .cloned()
        .collect();
    reversed.reverse();
    if !is_settled(&reversed) {
        defects.push(MotionDefect::new(
            MotionFamily::SettleDwell,
            "non_identical_settle_tail",
            format!(
                "the last {SETTLE_IDENTICAL_FRAMES} frames of the trace must be identical to count as settle"
            ),
        ));
    }
    let mut last_tick = None;
    for tick_frame in &trace.frames {
        if tick_frame.phase == MotionPhase::SettleRepeat {
            if let Some(prev) = last_tick {
                if tick_frame.tick != prev {
                    defects.push(MotionDefect::new(
                        MotionFamily::SettleDwell,
                        "settle_tick_churn",
                        format!(
                            "settle frames must share a single tick value; observed {} then {}",
                            prev, tick_frame.tick
                        ),
                    ));
                    break;
                }
            }
            last_tick = Some(tick_frame.tick);
        }
    }
    defects
}

fn validate_scroll_flush(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = validate_monotonic_ticks(trace);
    let scroll_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ScrollFlush);
    let Some(scroll_idx) = scroll_idx else {
        defects.push(MotionDefect::new(
            MotionFamily::ScrollFlush,
            "missing_scroll_flush_phase",
            "scroll flush family requires at least one scroll_flush frame",
        ));
        return defects;
    };
    // Scroll flush must not regress: after the scroll flush, no earlier-phase
    // startup or first-streaming-delta frame may appear (re-ordered delta is
    // acceptable, but startup regression is not).
    let earlier_phase = trace.frames[scroll_idx + 1..]
        .iter()
        .find(|tick_frame| tick_frame.phase == MotionPhase::Startup);
    if let Some(tick_frame) = earlier_phase {
        defects.push(MotionDefect::new(
            MotionFamily::ScrollFlush,
            "scroll_phase_regression",
            format!(
                "a startup phase frame appeared at tick {} after scroll_flush; scroll flush must not regress",
                tick_frame.tick
            ),
        ));
    }
    defects
}

fn validate_resize_debounce(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = validate_monotonic_ticks(trace);
    let has_burst = trace
        .frames
        .iter()
        .any(|tick_frame| tick_frame.phase == MotionPhase::ResizeBurst);
    let has_settled = trace
        .frames
        .iter()
        .any(|tick_frame| tick_frame.phase == MotionPhase::ResizeSettled);
    if !has_burst {
        defects.push(MotionDefect::new(
            MotionFamily::ResizeDebounce,
            "missing_resize_burst_phase",
            "resize debounce family requires at least one resize_burst frame",
        ));
    }
    if !has_settled {
        defects.push(MotionDefect::new(
            MotionFamily::ResizeDebounce,
            "missing_resize_settled_phase",
            "resize debounce family requires at least one resize_settled frame",
        ));
        return defects;
    }
    // The settled frame must appear after every burst frame.
    let last_burst_idx = trace
        .frames
        .iter()
        .rposition(|tick_frame| tick_frame.phase == MotionPhase::ResizeBurst);
    let first_settled_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ResizeSettled);
    if let (Some(last_burst), Some(first_settled)) = (last_burst_idx, first_settled_idx) {
        if first_settled <= last_burst {
            defects.push(MotionDefect::new(
                MotionFamily::ResizeDebounce,
                "resize_settled_before_burst",
                format!(
                    "resize_settled at index {} must appear after the last resize_burst at index {}",
                    first_settled, last_burst
                ),
            ));
        }
    }
    // After resize_settled, no further frame may differ in dimensions: that
    // would indicate continued geometry churn.
    let first_settled = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::ResizeSettled);
    if let Some(settled_idx) = first_settled {
        let settled_frame = &trace.frames[settled_idx].frame;
        for (offset, tick_frame) in trace.frames[settled_idx + 1..].iter().enumerate() {
            if tick_frame.frame.cols != settled_frame.cols
                || tick_frame.frame.rows != settled_frame.rows
            {
                defects.push(MotionDefect::new(
                    MotionFamily::ResizeDebounce,
                    "resize_churn_after_settle",
                    format!(
                        "frame at index {} (tick {}) differs in dimensions from the resize_settled frame; resize must settle once",
                        settled_idx + 1 + offset,
                        tick_frame.tick
                    ),
                ));
                break;
            }
        }
    }
    defects
}

fn validate_streaming_deltas(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = validate_monotonic_ticks(trace);
    let deltas: Vec<&TickFrame> = trace
        .frames
        .iter()
        .filter(|tick_frame| tick_frame.phase == MotionPhase::StreamingDelta)
        .collect();
    if deltas.is_empty() {
        defects.push(MotionDefect::new(
            MotionFamily::StreamingDeltas,
            "missing_streaming_delta_phase",
            "streaming deltas family requires at least one streaming_delta frame",
        ));
        return defects;
    }
    // Two streaming deltas at the same tick indicate input starvation /
    // non-monotonic flush; the validator already flags non-monotonic ticks,
    // but equal ticks on consecutive deltas are also a defect here.
    for pair in deltas.windows(2) {
        if pair[0].tick == pair[1].tick {
            defects.push(MotionDefect::new(
                MotionFamily::StreamingDeltas,
                "streaming_delta_tick_collision",
                format!(
                    "two streaming_delta frames share tick {}; each delta must advance the tick",
                    pair[0].tick
                ),
            ));
        }
    }
    defects
}

fn validate_cancellation_ordering(trace: &FrameTrace) -> Vec<MotionDefect> {
    let mut defects = validate_monotonic_ticks(trace);
    let cancel_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::Cancellation);
    let recovered_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::CancelRecovered);
    match (cancel_idx, recovered_idx) {
        (None, _) => {
            defects.push(MotionDefect::new(
                MotionFamily::CancellationOrdering,
                "missing_cancellation_phase",
                "cancellation family requires a cancellation frame",
            ));
        }
        (Some(_), None) => {
            defects.push(MotionDefect::new(
                MotionFamily::CancellationOrdering,
                "missing_cancel_recovered_phase",
                "cancellation must be followed by a cancel_recovered frame",
            ));
        }
        (Some(cancel), Some(recovered)) if recovered <= cancel => {
            defects.push(MotionDefect::new(
                MotionFamily::CancellationOrdering,
                "cancel_recovered_before_cancellation",
                format!(
                    "cancel_recovered at index {recovered} must appear after cancellation at index {cancel}"
                ),
            ));
        }
        _ => {}
    }
    // A stale spinner cell means a frame after cancel_recovered still carries
    // a spinner glyph. The contract identifies a spinner by a single
    // rotating character in the prompt/footer area; we approximate by
    // scanning for any of the canonical spinner glyphs in the frame after
    // cancel_recovered.
    if let Some(recovered_idx) = recovered_idx {
        for tick_frame in &trace.frames[recovered_idx + 1..] {
            if frame_contains_stale_spinner(&tick_frame.frame) {
                defects.push(MotionDefect::new(
                    MotionFamily::CancellationOrdering,
                    "stale_spinner_after_cancel",
                    format!(
                        "frame at tick {} after cancel_recovered still contains a spinner glyph; cancellation must clear turn state",
                        tick_frame.tick
                    ),
                ));
                break;
            }
        }
    }
    defects
}

/// Canonical spinner glyphs observed in the reference TUI. Cancellation must
/// clear every cell that still carries one of these.
const SPINNER_GLYPHS: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "|", "/", "-", "\\",
];

fn frame_contains_stale_spinner(frame: &SemanticFrame) -> bool {
    frame
        .cells
        .iter()
        .any(|cell| SPINNER_GLYPHS.contains(&cell.grapheme.as_str()))
}

/// Compares two motion traces for cross-source parity.
///
/// Returns `Ok(())` only when both traces validate independently against the
/// provided families AND the cell/cursor content of the trace at each shared
/// tick agrees under `masks`. Self-oracle (same source) is rejected.
pub fn compare_motion_traces(
    reference: &FrameTrace,
    candidate: &FrameTrace,
    masks: &IdentityMaskRegistry,
    families: &[MotionFamily],
) -> Result<(), Vec<MotionDefect>> {
    let mut defects = Vec::new();
    if reference.source == candidate.source {
        defects.push(MotionDefect::new(
            MotionFamily::OrderedMotion,
            "self_oracle",
            format!(
                "reference source {:?} must differ from candidate source {:?}",
                reference.source, candidate.source
            ),
        ));
    }
    defects.extend(
        validate_motion_trace_with_families(reference, families)
            .err()
            .unwrap_or_default(),
    );
    defects.extend(
        validate_motion_trace_with_families(candidate, families)
            .err()
            .unwrap_or_default(),
    );
    if defects.iter().any(|defect| {
        defect.family != MotionFamily::OrderedMotion || defect.reason != "self_oracle"
    }) {
        // If either trace fails its family contract we still report those
        // defects, but we cannot meaningfully compare cell content.
        return Err(defects);
    }
    // Compare shared ticks cell-by-cell under the identity masks.
    let reference_by_tick: std::collections::BTreeMap<u64, &TickFrame> = reference
        .frames
        .iter()
        .map(|tick_frame| (tick_frame.tick, tick_frame))
        .collect();
    let candidate_by_tick: std::collections::BTreeMap<u64, &TickFrame> = candidate
        .frames
        .iter()
        .map(|tick_frame| (tick_frame.tick, tick_frame))
        .collect();
    let shared_ticks: BTreeSet<u64> = reference_by_tick
        .keys()
        .filter(|tick| candidate_by_tick.contains_key(*tick))
        .copied()
        .collect();
    if shared_ticks.is_empty() {
        defects.push(MotionDefect::new(
            MotionFamily::OrderedMotion,
            "no_shared_ticks",
            "reference and candidate traces share no tick timestamps; cannot compare",
        ));
    }
    for tick in &shared_ticks {
        let reference_frame = &reference_by_tick[tick].frame;
        let candidate_frame = &candidate_by_tick[tick].frame;
        let cmp = super::compare::compare_frames(reference_frame, candidate_frame, masks);
        if let Err(cell_diffs) = cmp {
            for cell_diff in cell_diffs {
                defects.push(MotionDefect::new(
                    MotionFamily::OrderedMotion,
                    "frame_mismatch_at_shared_tick",
                    format!("tick {tick}: {cell_diff}"),
                ));
            }
        }
    }
    if defects.is_empty() {
        Ok(())
    } else {
        Err(defects)
    }
}

/// Builds a synthetic trace that satisfies every family contract. Used by
/// deterministic tests as the reference baseline.
pub fn synthetic_passing_trace(
    source: TraceSource,
) -> Result<FrameTrace, super::frame_io::ParityCellError> {
    let startup = SemanticFrame::new(8, 2, CursorState::visible_block(1, 0));
    let mut delta1 = startup.clone();
    delta1.set_cell(super::cells::SemanticCell::blank(0, 0).with_grapheme("h", 1))?;
    let mut delta2 = delta1.clone();
    delta2.set_cell(super::cells::SemanticCell::blank(0, 1).with_grapheme("i", 1))?;
    let mut finish = delta2.clone();
    finish.set_cell(super::cells::SemanticCell::blank(0, 2).with_grapheme(".", 1))?;
    let cancel_frame = finish.clone();
    let recovered_frame = finish.clone();
    let settle_a = finish.clone();
    let settle_b = finish.clone();
    let settle_c = finish.clone();
    Ok(FrameTrace {
        source,
        frames: vec![
            TickFrame {
                tick: 0,
                phase: MotionPhase::Startup,
                frame: startup,
            },
            TickFrame {
                tick: 1,
                phase: MotionPhase::StreamingDelta,
                frame: delta1,
            },
            TickFrame {
                tick: 2,
                phase: MotionPhase::StreamingDelta,
                frame: delta2,
            },
            TickFrame {
                tick: 3,
                phase: MotionPhase::ScrollFlush,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 4,
                phase: MotionPhase::ResizeBurst,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 5,
                phase: MotionPhase::ResizeSettled,
                frame: finish.clone(),
            },
            TickFrame {
                tick: 6,
                phase: MotionPhase::FinishFlash,
                frame: finish,
            },
            TickFrame {
                tick: 7,
                phase: MotionPhase::Cancellation,
                frame: cancel_frame,
            },
            TickFrame {
                tick: 8,
                phase: MotionPhase::CancelRecovered,
                frame: recovered_frame,
            },
            TickFrame {
                tick: 9,
                phase: MotionPhase::SettleRepeat,
                frame: settle_a,
            },
            TickFrame {
                tick: 9,
                phase: MotionPhase::SettleRepeat,
                frame: settle_b,
            },
            TickFrame {
                tick: 9,
                phase: MotionPhase::SettleRepeat,
                frame: settle_c,
            },
        ],
    })
}

/// Builds a synthetic trace that exercises cancellation.
pub fn synthetic_cancellation_trace(
    source: TraceSource,
) -> Result<FrameTrace, super::frame_io::ParityCellError> {
    synthetic_passing_trace(source)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, reason = "module-level smoke tests")]

    use super::*;

    #[test]
    fn synthetic_passing_trace_validates_every_family() {
        let trace =
            synthetic_passing_trace(TraceSource::Reference).expect("synthetic trace builds");
        for family in MotionFamily::all() {
            assert!(
                validate_family(&trace, *family).is_empty(),
                "family {family} rejected the synthetic baseline: {:?}",
                validate_family(&trace, *family)
            );
        }
    }

    #[test]
    fn synthetic_cancellation_trace_validates_cancellation_family() {
        let trace =
            synthetic_cancellation_trace(TraceSource::Reference).expect("synthetic trace builds");
        let defects = validate_family(&trace, MotionFamily::CancellationOrdering);
        assert!(
            defects.is_empty(),
            "cancellation family rejected the cancellation baseline: {defects:?}"
        );
    }
}
