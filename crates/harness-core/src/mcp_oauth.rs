use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpOauthRemoteAvailability {
    Available { transport: String },
    Unavailable { reason: String },
}
impl McpOauthRemoteAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { transport } => {
                format!("MCP OAuth remote: available (transport={transport})")
            }
            Self::Unavailable { reason } => format!("MCP OAuth remote: unavailable ({reason})"),
        }
    }
}
pub fn evaluate_mcp_oauth_remote_transports() -> McpOauthRemoteAvailability {
    McpOauthRemoteAvailability::Unavailable {
        reason: "no MCP OAuth remote transport configured".into(),
    }
}
