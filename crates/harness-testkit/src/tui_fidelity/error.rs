use super::types::{AdapterKind, CheckpointName, SemanticState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioError {
    Deserialize(String),
    Serialize(String),
    InvalidSchemaVersion { expected: String, observed: String },
    EmptyScenarioId,
    InvalidScenarioId,
    NoAdapters,
    DuplicateAdapter(AdapterKind),
    AdapterNotSelected(AdapterKind),
    NoActions,
    InvalidAction(ActionError),
    InvalidTiming(TimingError),
    InvalidGeometry(GeometryError),
    InvalidCheckpoint(CheckpointError),
    InvalidMotionCapture(MotionCaptureError),
    InvalidSubstitution(SubstitutionError),
    InvalidExitCode(ExitCodeError),
    InvalidCleanup(CleanupError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MotionCaptureError {
    NoFamilies,
    NoMarkers,
    DuplicateFamily,
    BoundaryOutOfRange { marker: usize, ordinal: usize },
    MarkerOutOfOrder { marker: usize },
    IncompatiblePhase { marker: usize },
    InvalidRepeatCount { marker: usize },
    MissingTerminalSettle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    EmptyPaste,
    EmptyText,
    ZeroInterByteDelay,
    EmptyTerminalReply,
    ControlText,
    NullCharacter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimingError {
    Zero {
        index: usize,
    },
    OutOfOrder {
        index: usize,
        previous: u64,
        current: u64,
    },
    CheckpointBeforeActions {
        checkpoint: CheckpointName,
    },
    CheckpointOutOfOrder {
        checkpoint: CheckpointName,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeometrySubject {
    ScenarioViewport,
    Action(usize),
    Checkpoint(CheckpointName),
    Substitution(CheckpointName),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeometryError {
    EmptyViewport { subject: GeometrySubject },
    OutOfBounds { subject: GeometrySubject },
    ViewportMismatch { subject: GeometrySubject },
    DragHasNoDistance { action: usize },
    WheelAmountZero { action: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    Count {
        observed: usize,
    },
    Missing(CheckpointName),
    Duplicate(CheckpointName),
    OutOfOrder {
        expected: CheckpointName,
        observed: CheckpointName,
    },
    StateMismatch {
        checkpoint: CheckpointName,
        expected: SemanticState,
        observed: SemanticState,
    },
    EmptyCaptureId(CheckpointName),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubstitutionError {
    EmptyText,
    ControlText,
    MissingReferenceProvenance,
    MissingCandidateProvenance,
    ControlReferenceProvenance,
    ControlCandidateProvenance,
    FieldKindMismatch,
    SameText,
    NonCanonicalPlaceholder,
    RectangleGeometry,
    BroadRegion,
    PaddingMismatch,
    StyleMismatch,
    WrappingMismatch,
    DuplicateField { checkpoint: CheckpointName },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitCodeError {
    Negative(i32),
    TooLarge(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupError {
    WorkspaceNotRestored,
    EvidenceNotPreserved,
    NoTemporaryPaths,
    InvalidPath,
    DuplicatePath,
}
