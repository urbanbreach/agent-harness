//! Compatibility status for the removed hosted workspace integration.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHubAvailability {
    Available { endpoint: String },
    Unavailable { reason: String },
}

impl WorkspaceHubAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Available { endpoint } => {
                format!("workspace hub: available (endpoint={endpoint})")
            }
            Self::Unavailable { reason } => format!("workspace hub: unavailable ({reason})"),
        }
    }
}

pub fn evaluate_workspace_hub() -> WorkspaceHubAvailability {
    WorkspaceHubAvailability::Unavailable {
        reason: "hosted workspace integration removed".to_string(),
    }
}
