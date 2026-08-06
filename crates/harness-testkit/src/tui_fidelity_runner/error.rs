use std::fmt;
use std::path::PathBuf;

use crate::tui_fidelity::{AdapterKind, CheckpointName, ScenarioError};

#[derive(Debug)]
pub enum RunnerError {
    Arguments {
        detail: String,
    },
    Scenario(ScenarioError),
    BinaryReceipt {
        path: PathBuf,
        detail: String,
    },
    MissingBinary {
        adapter: AdapterKind,
        path: PathBuf,
    },
    BinaryDigest {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    CandidateBinding {
        path: PathBuf,
        detail: String,
    },
    SelfComparison {
        sha256: String,
    },
    MissingBrowser {
        path: PathBuf,
    },
    MissingFont {
        family: String,
    },
    UnknownScenario {
        id: String,
    },
    DirtyReference {
        detail: String,
    },
    SourceGuard {
        detail: String,
    },
    SkippedReference,
    Timeout {
        adapter: AdapterKind,
    },
    PrematureExit {
        adapter: AdapterKind,
        code: i32,
    },
    ForcedKillOnly {
        adapter: AdapterKind,
    },
    UnexpectedExit {
        adapter: AdapterKind,
        expected: i32,
        actual: i32,
    },
    SurvivingChild {
        adapter: AdapterKind,
        pids: Vec<u32>,
    },
    MissingCheckpoint {
        adapter: AdapterKind,
        checkpoint: CheckpointName,
        path: PathBuf,
    },
    Renderer {
        checkpoint: CheckpointName,
        detail: String,
    },
    RendererTimeout {
        checkpoint: CheckpointName,
    },
    ExternalCommandTimeout {
        command: String,
    },
    InvalidRendererMetadata {
        checkpoint: CheckpointName,
        detail: String,
    },
    StaleEvidence {
        path: PathBuf,
    },
    Process {
        adapter: AdapterKind,
        detail: String,
    },
    Io {
        path: PathBuf,
        detail: String,
    },
    Cleanup {
        primary: Option<Box<RunnerError>>,
        detail: String,
    },
    Comparison {
        detail: String,
    },
}

impl fmt::Display for RunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments { detail } => write!(formatter, "arguments: {detail}"),
            Self::Scenario(error) => write!(formatter, "scenario: {error}"),
            Self::BinaryReceipt { path, detail } => {
                write!(formatter, "binary receipt {}: {detail}", path.display())
            }
            Self::MissingBinary { adapter, path } => write!(
                formatter,
                "{} binary is missing: {}",
                adapter.as_str(),
                path.display()
            ),
            Self::BinaryDigest {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "binary digest mismatch for {}: expected {expected}, got {actual}",
                path.display()
            ),
            Self::CandidateBinding { path, detail } => {
                write!(
                    formatter,
                    "candidate binary binding rejected for {}: {detail}",
                    path.display()
                )
            }
            Self::SelfComparison { sha256 } => write!(
                formatter,
                "reference and harness resolve to the same binary digest {sha256}"
            ),
            Self::MissingBrowser { path } => write!(
                formatter,
                "browser capability is missing: {}",
                path.display()
            ),
            Self::MissingFont { family } => {
                write!(formatter, "font capability is missing: {family}")
            }
            Self::UnknownScenario { id } => write!(formatter, "unknown scenario: {id}"),
            Self::DirtyReference { detail } => {
                write!(formatter, "dirty reference source: {detail}")
            }
            Self::SourceGuard { detail } => write!(formatter, "source guard failed: {detail}"),
            Self::SkippedReference => write!(formatter, "reference scenario reported Skipped"),
            Self::Timeout { adapter } => {
                write!(formatter, "{} scenario timed out", adapter.as_str())
            }
            Self::PrematureExit { adapter, code } => {
                write!(
                    formatter,
                    "{} exited prematurely with {code}",
                    adapter.as_str()
                )
            }
            Self::ForcedKillOnly { adapter } => write!(
                formatter,
                "{} completed only after forced termination",
                adapter.as_str()
            ),
            Self::UnexpectedExit {
                adapter,
                expected,
                actual,
            } => write!(
                formatter,
                "{} exit expected {expected}, got {actual}",
                adapter.as_str()
            ),
            Self::SurvivingChild { adapter, pids } => {
                let pids = pids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    formatter,
                    "{} left unexpected child PIDs [{pids}]",
                    adapter.as_str()
                )
            }
            Self::MissingCheckpoint {
                adapter,
                checkpoint,
                path,
            } => write!(
                formatter,
                "{} checkpoint {} artifact missing: {}",
                adapter.as_str(),
                checkpoint.as_str(),
                path.display()
            ),
            Self::Renderer { checkpoint, detail } => {
                write!(
                    formatter,
                    "renderer failed for {}: {detail}",
                    checkpoint.as_str()
                )
            }
            Self::RendererTimeout { checkpoint } => {
                write!(formatter, "renderer timed out for {}", checkpoint.as_str())
            }
            Self::ExternalCommandTimeout { command } => {
                write!(formatter, "external command timed out: {command}")
            }
            Self::InvalidRendererMetadata { checkpoint, detail } => write!(
                formatter,
                "renderer metadata invalid for {}: {detail}",
                checkpoint.as_str()
            ),
            Self::StaleEvidence { path } => write!(
                formatter,
                "evidence directory is not fresh: {}",
                path.display()
            ),
            Self::Process { adapter, detail } => {
                write!(formatter, "{} process failed: {detail}", adapter.as_str())
            }
            Self::Io { path, detail } => write!(formatter, "I/O {}: {detail}", path.display()),
            Self::Cleanup { primary, detail } => {
                if let Some(primary) = primary {
                    write!(formatter, "primary: {primary}; cleanup: {detail}")
                } else {
                    write!(formatter, "cleanup: {detail}")
                }
            }
            Self::Comparison { detail } => write!(formatter, "comparison: {detail}"),
        }
    }
}

impl std::error::Error for RunnerError {}

impl From<ScenarioError> for RunnerError {
    fn from(error: ScenarioError) -> Self {
        Self::Scenario(error)
    }
}
