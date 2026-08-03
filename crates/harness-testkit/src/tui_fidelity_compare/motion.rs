use crate::parity::motion::{
    compare_motion_traces, validate_motion_trace, FrameTrace, MotionFamily,
};

use super::error::ComparatorError;

pub const FLUSH_TOLERANCE_MS: u64 = 16;
pub const GESTURE_BOUNDARY_MS: u64 = 80;
pub const CADENCE_HZ: u64 = 30;
pub const TITLE_HOLD_TICKS: u64 = 8;
pub const ACTION_BLINK_TICKS: u64 = 15;
pub const RESIZE_DEBOUNCE_MS: u64 = 16;
pub const PASTE_FIRST_BYTE_MS: u64 = 2;
pub const PASTE_SETTLED_MS: u64 = 10;
pub const FRAME_TIMESTAMP_TOLERANCE_MS: u64 = 16;
pub const CADENCE_PERIOD_MS: u64 = 1_000 / CADENCE_HZ;
pub const CADENCE_MAX_GAP_MS: u64 = CADENCE_PERIOD_MS * 2 + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionTimingKind {
    Flush,
    GestureBoundary,
    CadenceGap,
    TitleHold,
    ActionBlink,
    ResizeDebounce,
    PasteFirstByte,
    PasteSettled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MotionTimingEvent {
    pub kind: MotionTimingKind,
    pub value: u64,
}

impl MotionTimingEvent {
    pub const fn new(kind: MotionTimingKind, value: u64) -> Self {
        Self { kind, value }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionTrace {
    pub frame_trace: FrameTrace,
    pub timestamps_ms: Vec<u64>,
    pub checkpoints: Vec<String>,
    pub timing_events: Vec<MotionTimingEvent>,
}

impl MotionTrace {
    pub fn from_frame_trace(frame_trace: FrameTrace, timestamps_ms: Vec<u64>) -> Self {
        let mut checkpoints = Vec::new();
        for frame in &frame_trace.frames {
            let checkpoint = match frame.phase.as_str() {
                "startup" => "rest",
                "streaming_delta" => "mid",
                "settle_repeat" => "settled",
                _ => continue,
            };
            if !checkpoints.iter().any(|item| item == checkpoint) {
                checkpoints.push(checkpoint.to_owned());
            }
        }
        Self {
            frame_trace,
            timestamps_ms,
            checkpoints,
            timing_events: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionIssue {
    pub side: String,
    pub reason: String,
    pub detail: String,
}

pub fn compare_motion(
    reference: &MotionTrace,
    candidate: &MotionTrace,
) -> Result<(), ComparatorError> {
    let mut defects = Vec::new();
    if reference.frame_trace.source == candidate.frame_trace.source {
        defects.push(MotionIssue {
            side: "pair".to_owned(),
            reason: "self_source".to_owned(),
            detail: "reference and candidate must have different trace sources".to_owned(),
        });
    }
    if let Err(source_defects) = compare_motion_traces(
        &reference.frame_trace,
        &candidate.frame_trace,
        &Default::default(),
        MotionFamily::all(),
    ) {
        defects.extend(source_defects.into_iter().map(|defect| MotionIssue {
            side: "pair".to_owned(),
            reason: defect.reason,
            detail: defect.detail,
        }));
    }
    validate_trace("reference", reference, &mut defects);
    validate_trace("candidate", candidate, &mut defects);
    if reference.timestamps_ms.len() == candidate.timestamps_ms.len() {
        for (index, (left, right)) in reference
            .timestamps_ms
            .iter()
            .zip(&candidate.timestamps_ms)
            .enumerate()
        {
            if left.abs_diff(*right) > FRAME_TIMESTAMP_TOLERANCE_MS {
                defects.push(MotionIssue {
                    side: "pair".to_owned(),
                    reason: "timestamp_drift".to_owned(),
                    detail: format!("frame {index}: {left}ms versus {right}ms"),
                });
            }
        }
    }
    if defects.is_empty() {
        Ok(())
    } else {
        let defects_len = defects.len();
        Err(ComparatorError::Motion {
            defects,
            defects_len,
        })
    }
}

fn validate_trace(side: &str, trace: &MotionTrace, defects: &mut Vec<MotionIssue>) {
    for required in ["rest", "mid", "settled"] {
        if !trace.checkpoints.iter().any(|item| item == required) {
            defects.push(MotionIssue {
                side: side.to_owned(),
                reason: "missing_checkpoint".to_owned(),
                detail: required.to_owned(),
            });
        }
    }
    if trace.timestamps_ms.len() != trace.frame_trace.frames.len() {
        defects.push(MotionIssue {
            side: side.to_owned(),
            reason: "timestamp_count".to_owned(),
            detail: format!(
                "{} timestamps for {} frames",
                trace.timestamps_ms.len(),
                trace.frame_trace.frames.len()
            ),
        });
    }
    if let Err(source_defects) = validate_motion_trace(&trace.frame_trace) {
        defects.extend(source_defects.into_iter().map(|defect| MotionIssue {
            side: side.to_owned(),
            reason: defect.reason,
            detail: defect.detail,
        }));
    }
    for event in &trace.timing_events {
        if let Some(detail) = invalid_timing(event) {
            defects.push(MotionIssue {
                side: side.to_owned(),
                reason: "timing_tolerance".to_owned(),
                detail,
            });
        }
    }
}

fn invalid_timing(event: &MotionTimingEvent) -> Option<String> {
    let valid = match event.kind {
        MotionTimingKind::Flush => event.value <= FLUSH_TOLERANCE_MS,
        MotionTimingKind::GestureBoundary => event.value <= GESTURE_BOUNDARY_MS,
        MotionTimingKind::CadenceGap => event.value <= CADENCE_MAX_GAP_MS,
        MotionTimingKind::TitleHold => event.value == TITLE_HOLD_TICKS,
        MotionTimingKind::ActionBlink => event.value == ACTION_BLINK_TICKS,
        MotionTimingKind::ResizeDebounce => event.value <= RESIZE_DEBOUNCE_MS,
        MotionTimingKind::PasteFirstByte => event.value <= PASTE_FIRST_BYTE_MS,
        MotionTimingKind::PasteSettled => event.value <= PASTE_SETTLED_MS,
    };
    (!valid).then_some(format!("{event:?} exceeds its named contract"))
}
