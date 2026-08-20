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
pub enum SubstitutionKind {
    IdentityText,
    TruthfulDynamicText,
}

impl SubstitutionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdentityText => "identity_text",
            Self::TruthfulDynamicText => "truthful_dynamic_text",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstitutionField {
    WorkspacePath,
    HomePath,
    ProductLogo,
    ProductTitle,
    BuildVersion,
    ReleaseDate,
    ReleaseHistory,
    ProviderName,
    ModelName,
    AccountName,
    SessionId,
    AuthIdentity,
}

impl SubstitutionField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkspacePath => "workspace_path",
            Self::HomePath => "home_path",
            Self::ProductLogo => "product_logo",
            Self::ProductTitle => "product_title",
            Self::BuildVersion => "build_version",
            Self::ReleaseDate => "release_date",
            Self::ReleaseHistory => "release_history",
            Self::ProviderName => "provider_name",
            Self::ModelName => "model_name",
            Self::AccountName => "account_name",
            Self::SessionId => "session_id",
            Self::AuthIdentity => "auth_identity",
        }
    }

    pub const fn placeholder(self) -> &'static str {
        match self {
            Self::WorkspacePath => "[WORKSPACE]",
            Self::HomePath => "[HOME]",
            Self::ProductLogo => "[LOGO]",
            Self::ProductTitle => "[PRODUCT]",
            Self::BuildVersion => "[VERSION]",
            Self::ReleaseDate => "[DATE]",
            Self::ReleaseHistory => "[RELEASE_HISTORY]",
            Self::ProviderName => "[PROVIDER]",
            Self::ModelName => "[MODEL]",
            Self::AccountName => "[ACCOUNT]",
            Self::SessionId => "[SESSION]",
            Self::AuthIdentity => "[AUTH]",
        }
    }

    pub const fn permits(self, kind: SubstitutionKind) -> bool {
        match kind {
            SubstitutionKind::IdentityText => {
                matches!(self, Self::ProductLogo | Self::ProductTitle)
            }
            SubstitutionKind::TruthfulDynamicText => matches!(
                self,
                Self::WorkspacePath
                    | Self::HomePath
                    | Self::BuildVersion
                    | Self::ReleaseDate
                    | Self::ReleaseHistory
                    | Self::ProviderName
                    | Self::ModelName
                    | Self::AccountName
                    | Self::SessionId
                    | Self::AuthIdentity
            ),
        }
    }

    pub fn mask_label(self, kind: SubstitutionKind) -> String {
        format!("{}:{}", kind.as_str(), self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSubstitution {
    pub checkpoint: CheckpointName,
    pub kind: SubstitutionKind,
    pub field: SubstitutionField,
    pub canonical_placeholder: String,
    pub reference_provenance: String,
    pub candidate_provenance: String,
    pub rectangle: CellRect,
    pub reference: TextPlacement,
    pub candidate: TextPlacement,
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
    pub substitutions: Vec<TextSubstitution>,
    pub expected_exit: ExpectedExit,
    pub cleanup: CleanupExpectation,
}
