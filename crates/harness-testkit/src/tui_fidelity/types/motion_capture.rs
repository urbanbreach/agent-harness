use serde::{Deserialize, Serialize};

use crate::parity::{MotionFamily, MotionPhase};

use super::CheckpointName;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionCaptureContract {
    pub families: Vec<MotionFamily>,
    pub markers: Vec<MotionMarker>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionMarker {
    pub phase: MotionPhase,
    pub boundary: MotionBoundary,
    pub observation: MotionObservationRule,
    pub repeat_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionBoundary {
    BeforeAction { ordinal: usize },
    AfterAction { ordinal: usize },
    Checkpoint { name: CheckpointName },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionObservationRule {
    FirstChanged,
    EachChanged,
    LastChangedBeforeStable,
    StableRepeat,
}
