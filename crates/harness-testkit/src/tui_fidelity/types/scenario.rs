use super::{
    action::{AdapterKind, SemanticState},
    geometry::{CellRect, TextPlacement, Tick, Viewport},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointName {
    Rest,
    Mid,
    Settled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalType {
    #[default]
    #[serde(rename = "xterm-256color")]
    Xterm256Color,
    #[serde(rename = "xterm")]
    Xterm,
}

impl TerminalType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Xterm256Color => "xterm-256color",
            Self::Xterm => "xterm",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    FullSession,
    ActionTail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameCapture {
    pub capture_id: String,
    pub viewport: Viewport,
    pub state: SemanticState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub name: CheckpointName,
    pub at_tick: Tick,
    pub frame: FrameCapture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScope {
    WorkspacePath,
    ProductLogo,
    ProductTitle,
    BuildVersion,
    ProviderName,
    AccountName,
}

impl IdentityScope {
    pub const fn placeholder(self) -> &'static str {
        match self {
            Self::WorkspacePath => "[WORKSPACE]",
            Self::ProductLogo => "[LOGO]",
            Self::ProductTitle => "[PRODUCT]",
            Self::BuildVersion => "[VERSION]",
            Self::ProviderName => "[PROVIDER]",
            Self::AccountName => "[ACCOUNT]",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentitySubstitution {
    pub checkpoint: CheckpointName,
    pub scope: IdentityScope,
    pub rectangle: CellRect,
    pub source: TextPlacement,
    pub target: TextPlacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedExit {
    pub code: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupExpectation {
    pub restore_workspace: bool,
    pub preserve_evidence: bool,
    pub temporary_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub schema_version: String,
    pub id: super::geometry::ScenarioId,
    pub adapters: Vec<AdapterKind>,
    pub viewport: Viewport,
    #[serde(default)]
    pub terminal_type: TerminalType,
    #[serde(default)]
    pub capture_mode: CaptureMode,
    pub actions: Vec<super::action::ScenarioAction>,
    pub motion_capture: super::motion_capture::MotionCaptureContract,
    pub checkpoints: Vec<Checkpoint>,
    pub substitutions: Vec<IdentitySubstitution>,
    pub expected_exit: ExpectedExit,
    pub cleanup: CleanupExpectation,
}
