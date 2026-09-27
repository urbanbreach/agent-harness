use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkspaceHubAvailability {
    Available { endpoint: String },
    Unavailable { reason: String },
}
impl WorkspaceHubAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { endpoint } => format!("workspace hub: available ({endpoint})"),
            Self::Unavailable { reason } => format!("workspace hub: unavailable ({reason})"),
        }
    }
}
pub fn evaluate_workspace_hub() -> WorkspaceHubAvailability {
    WorkspaceHubAvailability::Unavailable {
        reason: "no workspace hub endpoint configured".into(),
    }
}
