use std::fmt;

use super::error::{
    ActionError, CheckpointError, CleanupError, ExitCodeError, GeometryError, GeometrySubject,
    MotionCaptureError, ScenarioError, SubstitutionError, TimingError,
};

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deserialize(message) => write!(f, "scenario deserialization failed: {message}"),
            Self::Serialize(message) => write!(f, "scenario serialization failed: {message}"),
            Self::InvalidSchemaVersion { expected, observed } => {
                write!(f, "schema version expected {expected}, observed {observed}")
            }
            Self::EmptyScenarioId => f.write_str("scenario id is empty"),
            Self::InvalidScenarioId => f.write_str("scenario id is not a lowercase slug"),
            Self::NoAdapters => f.write_str("scenario selects no adapters"),
            Self::DuplicateAdapter(adapter) => {
                write!(f, "adapter {} is selected twice", adapter.as_str())
            }
            Self::AdapterNotSelected(adapter) => {
                write!(f, "adapter {} is not selected", adapter.as_str())
            }
            Self::NoActions => f.write_str("scenario has no actions"),
            Self::InvalidAction(error) => error.fmt(f),
            Self::InvalidTiming(error) => error.fmt(f),
            Self::InvalidGeometry(error) => error.fmt(f),
            Self::InvalidCheckpoint(error) => error.fmt(f),
            Self::InvalidMotionCapture(error) => error.fmt(f),
            Self::InvalidSubstitution(error) => error.fmt(f),
            Self::InvalidExitCode(error) => error.fmt(f),
            Self::InvalidCleanup(error) => error.fmt(f),
        }
    }
}

impl fmt::Display for MotionCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFamilies => f.write_str("motion capture selects no families"),
            Self::NoMarkers => f.write_str("motion capture declares no markers"),
            Self::DuplicateFamily => f.write_str("motion capture family is duplicated"),
            Self::BoundaryOutOfRange { marker, ordinal } => {
                write!(
                    f,
                    "motion marker {marker} references action {ordinal} out of range"
                )
            }
            Self::MarkerOutOfOrder { marker } => {
                write!(f, "motion marker {marker} is out of boundary order")
            }
            Self::IncompatiblePhase { marker } => {
                write!(
                    f,
                    "motion marker {marker} phase is incompatible with its families"
                )
            }
            Self::InvalidRepeatCount { marker } => {
                write!(f, "motion marker {marker} has an invalid repeat count")
            }
            Self::MissingTerminalSettle => {
                f.write_str("motion capture must end with three settled stable repeats")
            }
        }
    }
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPaste => f.write_str("paste text is empty"),
            Self::EmptyTerminalReply => f.write_str("terminal reply is empty"),
            Self::ControlText => f.write_str("action text contains a control character"),
            Self::NullCharacter => f.write_str("key character must not be NUL"),
        }
    }
}

impl fmt::Display for TimingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero { index } => write!(f, "action {index} has zero tick"),
            Self::OutOfOrder {
                index,
                previous,
                current,
            } => write!(f, "action {index} tick {current} follows {previous}"),
            Self::CheckpointBeforeActions { checkpoint } => {
                write!(
                    f,
                    "checkpoint {} precedes the action timeline",
                    checkpoint.as_str()
                )
            }
            Self::CheckpointOutOfOrder { checkpoint } => {
                write!(f, "checkpoint {} is out of order", checkpoint.as_str())
            }
        }
    }
}

impl fmt::Display for GeometrySubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScenarioViewport => f.write_str("scenario viewport"),
            Self::Action(index) => write!(f, "action {index}"),
            Self::Checkpoint(name) => write!(f, "checkpoint {}", name.as_str()),
            Self::Substitution(name) => write!(f, "substitution at {}", name.as_str()),
        }
    }
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyViewport { subject } => write!(f, "{subject} has zero geometry"),
            Self::OutOfBounds { subject } => write!(f, "{subject} is outside its viewport"),
            Self::ViewportMismatch { subject } => {
                write!(f, "{subject} does not use the active viewport")
            }
            Self::DragHasNoDistance { action } => {
                write!(f, "drag action {action} has no distance")
            }
            Self::WheelAmountZero { action } => {
                write!(f, "wheel action {action} has zero amount")
            }
        }
    }
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count { observed } => {
                write!(f, "expected three checkpoints, observed {observed}")
            }
            Self::Missing(name) => write!(f, "missing checkpoint {}", name.as_str()),
            Self::Duplicate(name) => write!(f, "duplicate checkpoint {}", name.as_str()),
            Self::OutOfOrder { expected, observed } => {
                write!(
                    f,
                    "expected checkpoint {}, observed {}",
                    expected.as_str(),
                    observed.as_str()
                )
            }
            Self::StateMismatch {
                checkpoint,
                expected,
                observed,
            } => write!(
                f,
                "checkpoint {} expected state {}, observed {}",
                checkpoint.as_str(),
                expected.as_str(),
                observed.as_str()
            ),
            Self::EmptyCaptureId(name) => {
                write!(f, "checkpoint {} has an empty capture id", name.as_str())
            }
        }
    }
}

impl fmt::Display for SubstitutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyText => f.write_str("identity text is empty"),
            Self::ControlText => f.write_str("identity text contains a control character"),
            Self::SameText => f.write_str("identity substitution does not change text"),
            Self::NonIdentityReplacement => {
                f.write_str("replacement is not a canonical identity placeholder")
            }
            Self::RectangleGeometry => f.write_str("identity rectangle geometry is invalid"),
            Self::BroadRegion => f.write_str("identity rectangle is a broad or whole-region mask"),
            Self::PaddingMismatch => {
                f.write_str("identity text is not padded to the exact rectangle")
            }
            Self::StyleMismatch => f.write_str("identity source and target styles differ"),
            Self::WrappingMismatch => f.write_str("identity source and target wrapping differ"),
            Self::DuplicateScope { checkpoint } => {
                write!(
                    f,
                    "identity scope is duplicated at checkpoint {}",
                    checkpoint.as_str()
                )
            }
        }
    }
}

impl fmt::Display for ExitCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Negative(code) => write!(f, "exit code {code} is negative"),
            Self::TooLarge(code) => write!(f, "exit code {code} exceeds 255"),
        }
    }
}

impl fmt::Display for CleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceNotRestored => f.write_str("cleanup must restore the workspace"),
            Self::EvidenceNotPreserved => f.write_str("cleanup must preserve evidence"),
            Self::NoTemporaryPaths => f.write_str("cleanup must declare temporary paths"),
            Self::InvalidPath => f.write_str("cleanup path must be relative and traversal-free"),
            Self::DuplicatePath => f.write_str("cleanup paths must be unique"),
        }
    }
}

impl std::error::Error for ScenarioError {}
