//! Availability of the unsupported MCP OAuth remote authentication workflow.
//! Configured MCP transports are implemented by harness-tools.

use serde::{Deserialize, Serialize};

/// MCP OAuth remote transport availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpOauthRemoteAvailability {
    Available { transport: String },
    Unavailable { reason: String },
}

impl McpOauthRemoteAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Available { transport } => {
                format!("MCP OAuth remote: available (transport={transport})")
            }
            Self::Unavailable { reason } => {
                format!("MCP OAuth remote: unavailable ({reason})")
            }
        }
    }
}

/// MCP OAuth has no public remote authentication configuration.
pub fn evaluate_mcp_oauth_remote_transports() -> McpOauthRemoteAvailability {
    McpOauthRemoteAvailability::Unavailable {
        reason: "no MCP OAuth remote transport configured".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_oauth_remote_reports_unavailable_when_unconfigured() {
        let availability = evaluate_mcp_oauth_remote_transports();
        assert!(!availability.is_available());
        assert!(availability.is_unavailable());
        assert!(availability
            .one_line()
            .contains("no MCP OAuth remote transport configured"));
    }
}
