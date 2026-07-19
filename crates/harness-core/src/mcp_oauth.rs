//! MCP OAuth + remote transport availability (honest MVP).
//!
//! Full MCP OAuth and remote transport lifecycle are **not** implemented.
//! Config-backed local/stdio MCP registration remains under tools/MCP and is
//! separate. This module only reports structured unavailability for OAuth remote.

use serde::{Deserialize, Serialize};

/// MCP OAuth remote transport availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpOauthRemoteAvailability {
    /// OAuth remote transport can run (not used by this MVP).
    Available { transport: String },
    /// OAuth remote / remote transport product surface is not implemented.
    Unavailable { reason: String },
}

impl McpOauthRemoteAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim OAuth remote product).
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

/// Evaluate MCP OAuth + remote transport availability.
///
/// MVP: always structured unavailable. Does not fake token storage or remote
/// streamable HTTP OAuth success.
pub fn evaluate_mcp_oauth_remote_transports() -> McpOauthRemoteAvailability {
    McpOauthRemoteAvailability::Unavailable {
        reason: "MCP OAuth and remote transport lifecycle are not implemented; \
                 config-backed local/stdio MCP registration remains available separately"
            .to_string(),
    }
}

/// Begin an MCP OAuth authorization flow (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpOauthBeginResult {
    Unavailable {
        reason: String,
        server_id: String,
        authorization_url_hint: String,
    },
}

impl McpOauthBeginResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                server_id,
                authorization_url_hint,
            } => format!(
                "MCP OAuth begin: unavailable server=`{server_id}` url=`{authorization_url_hint}` ({reason})"
            ),
        }
    }
}

/// Exchange an MCP OAuth authorization code for tokens (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpOauthTokenExchangeResult {
    Unavailable {
        reason: String,
        server_id: String,
        authorization_code_hint: String,
    },
}

impl McpOauthTokenExchangeResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                server_id,
                authorization_code_hint,
            } => format!(
                "MCP OAuth exchange: unavailable server=`{server_id}` code=`{authorization_code_hint}` ({reason})"
            ),
        }
    }
}

/// Open a remote MCP transport (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpRemoteTransportOpenResult {
    Unavailable {
        reason: String,
        server_id: String,
        endpoint_hint: String,
    },
}

impl McpRemoteTransportOpenResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                server_id,
                endpoint_hint,
            } => format!(
                "MCP remote open: unavailable server=`{server_id}` endpoint=`{endpoint_hint}` ({reason})"
            ),
        }
    }
}

/// Operator-facing counts for MCP OAuth/remote outcomes (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct McpOauthOutcomeSummary {
    pub begin_unavailable: usize,
    pub exchange_unavailable: usize,
    pub open_unavailable: usize,
    pub total: usize,
}

impl McpOauthOutcomeSummary {
    pub fn one_line(&self) -> String {
        format!(
            "MCP OAuth outcomes: begin={}, exchange={}, open={} unavailable ({} total)",
            self.begin_unavailable, self.exchange_unavailable, self.open_unavailable, self.total
        )
    }

    pub const fn all_unavailable(&self) -> bool {
        self.total > 0
            && self.begin_unavailable + self.exchange_unavailable + self.open_unavailable
                == self.total
    }
}

/// Summarize fail-closed MCP OAuth/remote outcomes for operator surfaces.
pub fn summarize_mcp_oauth_outcomes(
    begin: Option<&McpOauthBeginResult>,
    exchange: Option<&McpOauthTokenExchangeResult>,
    open: Option<&McpRemoteTransportOpenResult>,
) -> McpOauthOutcomeSummary {
    let mut summary = McpOauthOutcomeSummary::default();
    if begin.is_some() {
        summary.begin_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    if exchange.is_some() {
        summary.exchange_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    if open.is_some() {
        summary.open_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    summary
}

fn mcp_oauth_unavailable_reason() -> String {
    match evaluate_mcp_oauth_remote_transports() {
        McpOauthRemoteAvailability::Available { transport } => format!(
            "MCP OAuth remote marked available for transport `{transport}` but backend is not implemented"
        ),
        McpOauthRemoteAvailability::Unavailable { reason } => reason,
    }
}

pub fn begin_mcp_oauth_flow(
    server_id: impl Into<String>,
    authorization_url_hint: impl Into<String>,
) -> McpOauthBeginResult {
    McpOauthBeginResult::Unavailable {
        reason: mcp_oauth_unavailable_reason(),
        server_id: server_id.into(),
        authorization_url_hint: authorization_url_hint.into(),
    }
}

pub fn exchange_mcp_oauth_token(
    server_id: impl Into<String>,
    authorization_code_hint: impl Into<String>,
) -> McpOauthTokenExchangeResult {
    let code = authorization_code_hint.into();
    let redacted = if code.trim().is_empty() {
        String::new()
    } else {
        format!("{}…", code.chars().take(4).collect::<String>())
    };
    McpOauthTokenExchangeResult::Unavailable {
        reason: mcp_oauth_unavailable_reason(),
        server_id: server_id.into(),
        authorization_code_hint: redacted,
    }
}

pub fn open_mcp_remote_transport(
    server_id: impl Into<String>,
    endpoint_hint: impl Into<String>,
) -> McpRemoteTransportOpenResult {
    McpRemoteTransportOpenResult::Unavailable {
        reason: mcp_oauth_unavailable_reason(),
        server_id: server_id.into(),
        endpoint_hint: endpoint_hint.into(),
    }
}

/// Default multi-endpoint begin probes for the product walk.
pub const DEFAULT_MCP_OAUTH_BEGIN_PROBES: &[(&str, &str)] = &[
    ("(probe)", "https://auth.example/oauth"),
    ("(probe-alt)", "https://auth.example/oauth/alt"),
    ("(probe-remote)", "https://mcp-auth.example/authorize"),
];

/// Default multi-endpoint exchange probes for the product walk.
pub const DEFAULT_MCP_OAUTH_EXCHANGE_PROBES: &[(&str, &str)] = &[
    ("(probe)", "(probe-code)"),
    ("(probe-alt)", "(probe-code-alt)"),
    ("(probe-remote)", "(probe-device)"),
];

/// Default multi-endpoint open probes for the product walk.
pub const DEFAULT_MCP_OAUTH_OPEN_PROBES: &[(&str, &str)] = &[
    ("(probe)", "https://mcp.example/sse"),
    ("(probe-alt)", "https://mcp.example/sse/alt"),
    ("(probe-remote)", "https://mcp.example/message"),
];

/// Multi-endpoint MCP OAuth/remote product probe (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOauthProductProbe {
    pub availability: McpOauthRemoteAvailability,
    pub begins: Vec<McpOauthBeginResult>,
    pub exchanges: Vec<McpOauthTokenExchangeResult>,
    pub opens: Vec<McpRemoteTransportOpenResult>,
    pub last_begin: McpOauthBeginResult,
    pub last_exchange: McpOauthTokenExchangeResult,
    pub last_open: McpRemoteTransportOpenResult,
    pub summary: McpOauthOutcomeSummary,
}

impl McpOauthProductProbe {
    pub fn is_unavailable(&self) -> bool {
        self.availability.is_unavailable() && self.summary.all_unavailable()
    }
}

/// Walk begin across multiple server/url pairs (each fail-closed).
pub fn walk_mcp_oauth_begin(probes: &[(&str, &str)]) -> Vec<McpOauthBeginResult> {
    probes
        .iter()
        .map(|(server_id, url)| begin_mcp_oauth_flow(*server_id, *url))
        .collect()
}

/// Walk token exchange across multiple server/code pairs (codes redacted).
pub fn walk_mcp_oauth_exchange(probes: &[(&str, &str)]) -> Vec<McpOauthTokenExchangeResult> {
    probes
        .iter()
        .map(|(server_id, code)| exchange_mcp_oauth_token(*server_id, *code))
        .collect()
}

/// Walk remote open across multiple server/endpoint pairs (each fail-closed).
pub fn walk_mcp_oauth_open(probes: &[(&str, &str)]) -> Vec<McpRemoteTransportOpenResult> {
    probes
        .iter()
        .map(|(server_id, endpoint)| open_mcp_remote_transport(*server_id, *endpoint))
        .collect()
}

/// Product path: multi-endpoint begin×N + exchange×N + open×N, bind last of each.
///
/// Summary is last-of-each (total=3) for operator surfaces. Never claims token
/// storage or live remote transport success.
pub fn probe_mcp_oauth_remote_product() -> McpOauthProductProbe {
    let availability = evaluate_mcp_oauth_remote_transports();
    let begins = walk_mcp_oauth_begin(DEFAULT_MCP_OAUTH_BEGIN_PROBES);
    let exchanges = walk_mcp_oauth_exchange(DEFAULT_MCP_OAUTH_EXCHANGE_PROBES);
    let opens = walk_mcp_oauth_open(DEFAULT_MCP_OAUTH_OPEN_PROBES);
    let last_begin = begins
        .last()
        .cloned()
        .unwrap_or_else(|| begin_mcp_oauth_flow("(probe)", "https://auth.example/oauth"));
    let last_exchange = exchanges
        .last()
        .cloned()
        .unwrap_or_else(|| exchange_mcp_oauth_token("(probe)", "(probe-code)"));
    let last_open = opens
        .last()
        .cloned()
        .unwrap_or_else(|| open_mcp_remote_transport("(probe)", "https://mcp.example/sse"));
    let summary =
        summarize_mcp_oauth_outcomes(Some(&last_begin), Some(&last_exchange), Some(&last_open));
    McpOauthProductProbe {
        availability,
        begins,
        exchanges,
        opens,
        last_begin,
        last_exchange,
        last_open,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_oauth_remote_reports_structured_unavailable() {
        // arrange
        // act
        // assert
        // When
        let availability = evaluate_mcp_oauth_remote_transports();

        // Then
        assert!(availability.is_unavailable());
        assert!(!availability.is_available());
        match availability {
            McpOauthRemoteAvailability::Unavailable { reason } => {
                assert!(reason.contains("OAuth") || reason.contains("remote"));
                assert!(reason.contains("not implemented"));
            }
            McpOauthRemoteAvailability::Available { .. } => {
                panic!("must not claim OAuth remote available")
            }
        }
    }

    #[test]
    fn begin_exchange_open_fail_closed_with_operator_hints() {
        // arrange
        // act
        // assert
        // Given / When
        let begin = begin_mcp_oauth_flow("docs-server", "https://auth.example/oauth");
        let exchange = exchange_mcp_oauth_token("docs-server", "abcd1234secret");
        let open = open_mcp_remote_transport("docs-server", "https://mcp.example/sse");

        // Then
        match begin {
            McpOauthBeginResult::Unavailable {
                reason,
                server_id,
                authorization_url_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(server_id, "docs-server");
                assert_eq!(authorization_url_hint, "https://auth.example/oauth");
            }
        }
        match exchange {
            McpOauthTokenExchangeResult::Unavailable {
                reason,
                server_id,
                authorization_code_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(server_id, "docs-server");
                assert_eq!(authorization_code_hint, "abcd…");
                assert!(!authorization_code_hint.contains("secret"));
            }
        }
        match open {
            McpRemoteTransportOpenResult::Unavailable {
                reason,
                server_id,
                endpoint_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(server_id, "docs-server");
                assert_eq!(endpoint_hint, "https://mcp.example/sse");
            }
        }
    }

    #[test]
    fn mcp_oauth_operator_diagnostics_cover_availability_and_outcomes() {
        // arrange
        // act
        // assert
        // Given
        let availability = evaluate_mcp_oauth_remote_transports();
        let begin = begin_mcp_oauth_flow("docs-server", "https://auth.example/oauth");
        let exchange = exchange_mcp_oauth_token("docs-server", "abcd1234secret");
        let open = open_mcp_remote_transport("docs-server", "https://mcp.example/sse");

        // When
        let summary = summarize_mcp_oauth_outcomes(Some(&begin), Some(&exchange), Some(&open));

        // Then
        assert!(availability
            .one_line()
            .contains("MCP OAuth remote: unavailable"));
        assert!(begin.one_line().contains("server=`docs-server`"));
        assert!(begin
            .one_line()
            .contains("url=`https://auth.example/oauth`"));
        assert!(exchange.one_line().contains("code=`abcd…`"));
        assert!(!exchange.one_line().contains("secret"));
        assert!(open
            .one_line()
            .contains("endpoint=`https://mcp.example/sse`"));
        assert_eq!(
            summary,
            McpOauthOutcomeSummary {
                begin_unavailable: 1,
                exchange_unavailable: 1,
                open_unavailable: 1,
                total: 3,
            }
        );
        assert!(summary.all_unavailable());
        assert!(summary.one_line().contains("3 total"));
    }

    #[test]
    fn multi_endpoint_product_probe_all_unavailable_with_last_bindings() {
        // arrange
        // act
        // assert
        // Given / When
        let probe = probe_mcp_oauth_remote_product();

        // Then: 3× begin + 3× exchange + 3× open, last-of-each summary total=3
        assert_eq!(probe.begins.len(), 3);
        assert_eq!(probe.exchanges.len(), 3);
        assert_eq!(probe.opens.len(), 3);
        assert!(probe.availability.is_unavailable());
        assert!(probe.is_unavailable());
        assert_eq!(probe.summary.total, 3);
        assert!(probe.summary.all_unavailable());
        assert!(
            probe
                .last_begin
                .one_line()
                .contains("server=`(probe-remote)`")
                && probe
                    .last_begin
                    .one_line()
                    .contains("url=`https://mcp-auth.example/authorize`"),
            "last begin: {}",
            probe.last_begin.one_line()
        );
        assert!(
            probe
                .last_exchange
                .one_line()
                .contains("server=`(probe-remote)`")
                && probe.last_exchange.one_line().contains("code=`(pro…`"),
            "last exchange: {}",
            probe.last_exchange.one_line()
        );
        assert!(!probe.last_exchange.one_line().contains("probe-device"));
        assert!(
            probe
                .last_open
                .one_line()
                .contains("server=`(probe-remote)`")
                && probe
                    .last_open
                    .one_line()
                    .contains("endpoint=`https://mcp.example/message`"),
            "last open: {}",
            probe.last_open.one_line()
        );
        for begin in &probe.begins {
            assert!(begin.one_line().contains("unavailable"));
        }
        for exchange in &probe.exchanges {
            assert!(exchange.one_line().contains("unavailable"));
            assert!(!exchange.one_line().contains("probe-device"));
        }
        for open in &probe.opens {
            assert!(open.one_line().contains("unavailable"));
        }
    }
}
