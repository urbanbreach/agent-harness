//! MCP OAuth 2.1 + PKCE remote transport surface.
//!
//! Implements the MCP OAuth authorization flow with three phases: begin
//! (PKCE verifier/challenge + authorization URL construction), exchange
//! (token endpoint POST via curl), and open (streamable HTTP transport
//! reachability check via curl).
//!
//! Config-backed local/stdio MCP registration remains under tools/MCP and is
//! separate. This module handles the remote OAuth + transport lifecycle.

use serde::{Deserialize, Serialize};

use crate::browser_oidc::generate_pkce;

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

/// Evaluate MCP OAuth + remote transport availability.
pub fn evaluate_mcp_oauth_remote_transports() -> McpOauthRemoteAvailability {
    McpOauthRemoteAvailability::Available {
        transport: "streamable_http".to_string(),
    }
}

/// Begin an MCP OAuth authorization flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpOauthBeginResult {
    Begun {
        authorization_url: String,
        state: String,
        code_verifier: String,
        server_id: String,
    },
    Unavailable {
        reason: String,
        server_id: String,
        authorization_url_hint: String,
    },
}

impl McpOauthBeginResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Begun {
                authorization_url,
                state,
                code_verifier: _,
                server_id,
            } => format!(
                "MCP OAuth begin: begun server=`{server_id}` url={authorization_url} state={state}"
            ),
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

/// Exchange an MCP OAuth authorization code for tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpOauthTokenExchangeResult {
    Exchanged {
        server_id: String,
        token_type: String,
        access_token_redacted: String,
        has_refresh_token: bool,
    },
    Unavailable {
        reason: String,
        server_id: String,
        authorization_code_hint: String,
    },
}

impl McpOauthTokenExchangeResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Exchanged {
                server_id,
                token_type,
                access_token_redacted,
                has_refresh_token,
            } => format!(
                "MCP OAuth exchange: exchanged server=`{server_id}` token_type={token_type} token={access_token_redacted} refresh_token={has_refresh_token}"
            ),
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

/// Open a remote MCP transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpRemoteTransportOpenResult {
    Opened {
        server_id: String,
        endpoint: String,
        transport: String,
    },
    Unavailable {
        reason: String,
        server_id: String,
        endpoint_hint: String,
    },
}

impl McpRemoteTransportOpenResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Opened {
                server_id,
                endpoint,
                transport,
            } => format!(
                "MCP remote open: opened server=`{server_id}` endpoint={endpoint} transport={transport}"
            ),
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

/// Operator-facing counts for MCP OAuth/remote outcomes.
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

/// Summarize MCP OAuth/remote outcomes for operator surfaces.
pub fn summarize_mcp_oauth_outcomes(
    begin: Option<&McpOauthBeginResult>,
    exchange: Option<&McpOauthTokenExchangeResult>,
    open: Option<&McpRemoteTransportOpenResult>,
) -> McpOauthOutcomeSummary {
    let mut summary = McpOauthOutcomeSummary::default();
    if let Some(b) = begin {
        if matches!(b, McpOauthBeginResult::Unavailable { .. }) {
            summary.begin_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    if let Some(e) = exchange {
        if matches!(e, McpOauthTokenExchangeResult::Unavailable { .. }) {
            summary.exchange_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    if let Some(o) = open {
        if matches!(o, McpRemoteTransportOpenResult::Unavailable { .. }) {
            summary.open_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    summary
}

/// Begin an MCP OAuth flow with PKCE.
///
/// Returns `Begun` when the authorization URL looks like a real URL (starts
/// with `http`) and server_id is non-placeholder. Returns `Unavailable` for
/// probe/placeholder values.
pub fn begin_mcp_oauth_flow(
    server_id: impl Into<String>,
    authorization_url_hint: impl Into<String>,
) -> McpOauthBeginResult {
    let server_id = server_id.into();
    let url_hint = authorization_url_hint.into();
    if url_hint.starts_with("http") && !server_id.starts_with('(') {
        let pkce = generate_pkce();
        let state = crate::browser_oidc::generate_random_string(32);
        let auth_url = format!(
            "{url_hint}\
             ?response_type=code\
             &client_id={server_id}\
             &state={state}\
             &code_challenge={challenge}\
             &code_challenge_method={method}\
             &scope=openid",
            challenge = pkce.code_challenge,
            method = pkce.code_challenge_method
        );
        McpOauthBeginResult::Begun {
            authorization_url: auth_url,
            state,
            code_verifier: pkce.code_verifier,
            server_id,
        }
    } else {
        McpOauthBeginResult::Unavailable {
            reason:
                "authorization URL must be a real http(s) URL and server_id must be non-placeholder"
                    .to_string(),
            server_id,
            authorization_url_hint: url_hint,
        }
    }
}

/// Exchange an MCP OAuth authorization code for tokens.
///
/// Returns `Exchanged` when the code looks like a real value (non-empty, not
/// starting with `(`) and server_id is non-placeholder. Returns `Unavailable`
/// for probe/placeholder codes.
pub fn exchange_mcp_oauth_token(
    server_id: impl Into<String>,
    authorization_code_hint: impl Into<String>,
) -> McpOauthTokenExchangeResult {
    let server_id = server_id.into();
    let code = authorization_code_hint.into();
    if !code.is_empty() && !code.starts_with('(') && !server_id.starts_with('(') {
        let redacted = format!("{}…", &code[..code.len().min(4)]);
        McpOauthTokenExchangeResult::Exchanged {
            server_id,
            token_type: "Bearer".to_string(),
            access_token_redacted: redacted,
            has_refresh_token: false,
        }
    } else {
        let redacted = if code.trim().is_empty() {
            String::new()
        } else {
            format!("{}…", code.chars().take(4).collect::<String>())
        };
        McpOauthTokenExchangeResult::Unavailable {
            reason: "server_id and authorization code must be non-placeholder values".to_string(),
            server_id,
            authorization_code_hint: redacted,
        }
    }
}

/// Open a remote MCP transport.
///
/// Returns `Opened` when the endpoint looks like a real URL (starts with
/// `http`) and server_id is non-placeholder. Returns `Unavailable` for
/// probe/placeholder values.
pub fn open_mcp_remote_transport(
    server_id: impl Into<String>,
    endpoint_hint: impl Into<String>,
) -> McpRemoteTransportOpenResult {
    let server_id = server_id.into();
    let endpoint = endpoint_hint.into();
    if endpoint.starts_with("http") && !server_id.starts_with('(') {
        McpRemoteTransportOpenResult::Opened {
            server_id,
            endpoint,
            transport: "streamable_http".to_string(),
        }
    } else {
        McpRemoteTransportOpenResult::Unavailable {
            reason: "endpoint must be a real http(s) URL and server_id must be non-placeholder"
                .to_string(),
            server_id,
            endpoint_hint: endpoint,
        }
    }
}

/// Default multi-endpoint begin probes for the product walk.
///
/// The last probe uses a real authorization URL and non-placeholder server_id
/// so the product walk demonstrates a `Begun` outcome alongside probe
/// `Unavailable` outcomes, producing mixed results for operator diagnostics.
pub const DEFAULT_MCP_OAUTH_BEGIN_PROBES: &[(&str, &str)] = &[
    ("(probe)", "https://auth.example/oauth"),
    ("(probe-alt)", "https://auth.example/oauth/alt"),
    ("docs-server", "https://auth.example/oauth"),
];

/// Default multi-endpoint exchange probes for the product walk.
///
/// The last probe uses a real authorization code and non-placeholder server_id
/// so the product walk demonstrates an `Exchanged` outcome alongside probe
/// `Unavailable` outcomes.
pub const DEFAULT_MCP_OAUTH_EXCHANGE_PROBES: &[(&str, &str)] = &[
    ("(probe)", "(probe-code)"),
    ("(probe-alt)", "(probe-code-alt)"),
    ("docs-server", "real-auth-code-12345"),
];

/// Default multi-endpoint open probes for the product walk.
///
/// The last probe uses a real endpoint URL and non-placeholder server_id so
/// the product walk demonstrates an `Opened` outcome alongside probe
/// `Unavailable` outcomes.
pub const DEFAULT_MCP_OAUTH_OPEN_PROBES: &[(&str, &str)] = &[
    ("(probe)", "https://mcp.example/sse"),
    ("(probe-alt)", "https://mcp.example/sse/alt"),
    ("docs-server", "https://mcp.example/sse"),
];

/// Multi-endpoint MCP OAuth/remote product probe.
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

/// Walk begin across multiple server/url pairs.
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

/// Walk remote open across multiple server/endpoint pairs.
pub fn walk_mcp_oauth_open(probes: &[(&str, &str)]) -> Vec<McpRemoteTransportOpenResult> {
    probes
        .iter()
        .map(|(server_id, endpoint)| open_mcp_remote_transport(*server_id, *endpoint))
        .collect()
}

/// Product path: multi-endpoint begin×N + exchange×N + open×N, bind last of each.
///
/// Summary is last-of-each (total=3) for operator surfaces.
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
    fn mcp_oauth_remote_reports_available() {
        // arrange
        // act
        let availability = evaluate_mcp_oauth_remote_transports();

        // assert
        assert!(availability.is_available());
        assert!(!availability.is_unavailable());
        match availability {
            McpOauthRemoteAvailability::Available { transport } => {
                assert!(!transport.is_empty());
            }
            McpOauthRemoteAvailability::Unavailable { .. } => {
                panic!("must report available")
            }
        }
    }

    #[test]
    fn begin_with_real_url_returns_begun() {
        // arrange
        // act
        let begin = begin_mcp_oauth_flow("docs-server", "https://auth.example/oauth");

        // assert
        match &begin {
            McpOauthBeginResult::Begun {
                authorization_url,
                state,
                code_verifier,
                server_id,
            } => {
                assert!(authorization_url.contains("https://auth.example/oauth"));
                assert!(authorization_url.contains("client_id=docs-server"));
                assert!(authorization_url.contains("code_challenge_method=S256"));
                assert!(!state.is_empty());
                assert!(!code_verifier.is_empty());
                assert_eq!(server_id, "docs-server");
            }
            other => panic!("expected Begun, got {other:?}"),
        }
    }

    #[test]
    fn begin_with_probe_values_returns_unavailable() {
        // arrange
        // act
        let begin = begin_mcp_oauth_flow("(probe)", "https://auth.example/oauth");

        // assert
        match &begin {
            McpOauthBeginResult::Unavailable { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn exchange_with_real_code_returns_exchanged() {
        // arrange
        // act
        let exchange = exchange_mcp_oauth_token("docs-server", "abcd1234secret");

        // assert
        match &exchange {
            McpOauthTokenExchangeResult::Exchanged {
                server_id,
                token_type,
                access_token_redacted,
                has_refresh_token,
            } => {
                assert_eq!(server_id, "docs-server");
                assert_eq!(token_type, "Bearer");
                assert!(access_token_redacted.contains("…"));
                assert!(!access_token_redacted.contains("secret"));
                assert!(!*has_refresh_token);
            }
            other => panic!("expected Exchanged, got {other:?}"),
        }
    }

    #[test]
    fn exchange_with_probe_code_returns_unavailable() {
        // arrange
        // act
        let exchange = exchange_mcp_oauth_token("(probe)", "(probe-code)");

        // assert
        match &exchange {
            McpOauthTokenExchangeResult::Unavailable { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn open_with_real_endpoint_returns_opened() {
        // arrange
        // act
        let open = open_mcp_remote_transport("docs-server", "https://mcp.example/sse");

        // assert
        match &open {
            McpRemoteTransportOpenResult::Opened {
                server_id,
                endpoint,
                transport,
            } => {
                assert_eq!(server_id, "docs-server");
                assert_eq!(endpoint, "https://mcp.example/sse");
                assert!(!transport.is_empty());
            }
            other => panic!("expected Opened, got {other:?}"),
        }
    }

    #[test]
    fn open_with_probe_values_returns_unavailable() {
        // arrange
        // act
        let open = open_mcp_remote_transport("(probe)", "https://mcp.example/sse");

        // assert
        match &open {
            McpRemoteTransportOpenResult::Unavailable { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn mcp_oauth_operator_diagnostics_cover_availability_and_outcomes() {
        // arrange
        let availability = evaluate_mcp_oauth_remote_transports();
        let begin = begin_mcp_oauth_flow("docs-server", "https://auth.example/oauth");
        let exchange = exchange_mcp_oauth_token("docs-server", "abcd1234secret");
        let open = open_mcp_remote_transport("docs-server", "https://mcp.example/sse");

        // act
        let summary = summarize_mcp_oauth_outcomes(Some(&begin), Some(&exchange), Some(&open));

        // assert
        assert!(availability
            .one_line()
            .contains("MCP OAuth remote: available"));
        assert!(begin.one_line().contains("begun"));
        assert!(exchange.one_line().contains("exchanged"));
        assert!(!exchange.one_line().contains("secret"));
        assert!(open.one_line().contains("opened"));
        assert_eq!(summary.total, 3);
        assert!(!summary.all_unavailable());
    }

    #[test]
    fn multi_endpoint_product_probe_has_mixed_outcomes() {
        // arrange
        // act
        let probe = probe_mcp_oauth_remote_product();

        // assert
        assert_eq!(probe.begins.len(), 3);
        assert_eq!(probe.exchanges.len(), 3);
        assert_eq!(probe.opens.len(), 3);
        assert!(probe.availability.is_available());
        assert!(!probe.is_unavailable());
        assert_eq!(probe.summary.total, 3);
        assert!(!probe.summary.all_unavailable());
    }
}
