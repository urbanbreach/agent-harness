//! Browser / enterprise OIDC-SSO auth surface (honest MVP).
//!
//! Generic browser-device OIDC and enterprise SSO are **not** implemented.
//! Callers receive structured unavailability rather than a silent allow.

use serde::{Deserialize, Serialize};

/// Browser/device OIDC-SSO availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BrowserOidcAvailability {
    /// OIDC browser/device flow can run (not used by this MVP).
    Available,
    /// OIDC browser/device/SSO product surface is not implemented.
    Unavailable { reason: String },
}

impl BrowserOidcAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim OIDC product).
    pub fn one_line(&self) -> String {
        match self {
            Self::Available => "browser OIDC: available".to_string(),
            Self::Unavailable { reason } => {
                format!("browser OIDC: unavailable ({reason})")
            }
        }
    }
}

/// Evaluate browser OIDC / enterprise SSO availability.
///
/// MVP: always structured unavailable. Existing provider-specific auth (Codex
/// OAuth, Copilot, API keys) lives under `auth` and is separate.
pub fn evaluate_browser_oidc_availability() -> BrowserOidcAvailability {
    BrowserOidcAvailability::Unavailable {
        reason: "browser/device OIDC and enterprise SSO product surface is not implemented; \
                 use provider-specific auth (codex/copilot/api key) instead"
            .to_string(),
    }
}

/// Start a browser/device OIDC flow (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BrowserOidcStartResult {
    Unavailable {
        reason: String,
        issuer_hint: String,
        client_id_hint: String,
    },
}

impl BrowserOidcStartResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                issuer_hint,
                client_id_hint,
            } => format!(
                "browser OIDC start: unavailable issuer=`{issuer_hint}` client=`{client_id_hint}` ({reason})"
            ),
        }
    }
}

/// Complete a browser/device OIDC flow with an authorization code (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BrowserOidcCompleteResult {
    Unavailable {
        reason: String,
        authorization_code_hint: String,
    },
}

impl BrowserOidcCompleteResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                authorization_code_hint,
            } => format!(
                "browser OIDC complete: unavailable code=`{authorization_code_hint}` ({reason})"
            ),
        }
    }
}

/// Operator-facing counts for OIDC operation outcomes (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BrowserOidcOutcomeSummary {
    pub start_unavailable: usize,
    pub complete_unavailable: usize,
    pub total: usize,
}

impl BrowserOidcOutcomeSummary {
    pub fn one_line(&self) -> String {
        format!(
            "browser OIDC outcomes: start={}, complete={} unavailable ({} total)",
            self.start_unavailable, self.complete_unavailable, self.total
        )
    }

    pub const fn all_unavailable(&self) -> bool {
        self.total > 0 && self.start_unavailable + self.complete_unavailable == self.total
    }
}

/// Summarize fail-closed OIDC outcomes for operator surfaces.
pub fn summarize_browser_oidc_outcomes(
    start: Option<&BrowserOidcStartResult>,
    complete: Option<&BrowserOidcCompleteResult>,
) -> BrowserOidcOutcomeSummary {
    let mut summary = BrowserOidcOutcomeSummary::default();
    if start.is_some() {
        summary.start_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    if complete.is_some() {
        summary.complete_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    summary
}

fn oidc_unavailable_reason() -> String {
    match evaluate_browser_oidc_availability() {
        BrowserOidcAvailability::Available => {
            "OIDC marked available but browser/device backend is not implemented".to_string()
        }
        BrowserOidcAvailability::Unavailable { reason } => reason,
    }
}

pub fn start_browser_oidc_flow(
    issuer_hint: impl Into<String>,
    client_id_hint: impl Into<String>,
) -> BrowserOidcStartResult {
    BrowserOidcStartResult::Unavailable {
        reason: oidc_unavailable_reason(),
        issuer_hint: issuer_hint.into(),
        client_id_hint: client_id_hint.into(),
    }
}

pub fn complete_browser_oidc_flow(
    authorization_code_hint: impl Into<String>,
) -> BrowserOidcCompleteResult {
    let code = authorization_code_hint.into();
    let redacted = if code.trim().is_empty() {
        String::new()
    } else {
        format!("{}…", code.chars().take(4).collect::<String>())
    };
    BrowserOidcCompleteResult::Unavailable {
        reason: oidc_unavailable_reason(),
        authorization_code_hint: redacted,
    }
}

/// Default multi-endpoint start probes for the product walk.
pub const DEFAULT_BROWSER_OIDC_START_PROBES: &[(&str, &str)] = &[
    ("(probe)", "(client)"),
    ("(probe-alt)", "(client-alt)"),
    ("https://issuer.example", "(client-web)"),
];

/// Default multi-endpoint complete probes for the product walk.
pub const DEFAULT_BROWSER_OIDC_COMPLETE_PROBES: &[&str] =
    &["(probe-code)", "(probe-code-alt)", "(probe-device)"];

/// Multi-endpoint browser OIDC product probe (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserOidcProductProbe {
    pub availability: BrowserOidcAvailability,
    pub starts: Vec<BrowserOidcStartResult>,
    pub completes: Vec<BrowserOidcCompleteResult>,
    pub last_start: BrowserOidcStartResult,
    pub last_complete: BrowserOidcCompleteResult,
    pub summary: BrowserOidcOutcomeSummary,
}

impl BrowserOidcProductProbe {
    pub fn is_unavailable(&self) -> bool {
        self.availability.is_unavailable() && self.summary.all_unavailable()
    }
}

/// Walk start across multiple issuer/client pairs (each fail-closed).
pub fn walk_browser_oidc_start(probes: &[(&str, &str)]) -> Vec<BrowserOidcStartResult> {
    probes
        .iter()
        .map(|(issuer, client)| start_browser_oidc_flow(*issuer, *client))
        .collect()
}

/// Walk complete across multiple authorization codes (codes redacted).
pub fn walk_browser_oidc_complete(codes: &[&str]) -> Vec<BrowserOidcCompleteResult> {
    codes
        .iter()
        .map(|code| complete_browser_oidc_flow(*code))
        .collect()
}

/// Product path: multi-endpoint start×N + complete×N, bind last of each.
///
/// Summary is last-of-each (total=2) for operator surfaces. Never claims a live
/// browser/device OIDC success path.
pub fn probe_browser_oidc_product() -> BrowserOidcProductProbe {
    let availability = evaluate_browser_oidc_availability();
    let starts = walk_browser_oidc_start(DEFAULT_BROWSER_OIDC_START_PROBES);
    let completes = walk_browser_oidc_complete(DEFAULT_BROWSER_OIDC_COMPLETE_PROBES);
    let last_start = starts
        .last()
        .cloned()
        .unwrap_or_else(|| start_browser_oidc_flow("(probe)", "(client)"));
    let last_complete = completes
        .last()
        .cloned()
        .unwrap_or_else(|| complete_browser_oidc_flow("(probe-code)"));
    let summary = summarize_browser_oidc_outcomes(Some(&last_start), Some(&last_complete));
    BrowserOidcProductProbe {
        availability,
        starts,
        completes,
        last_start,
        last_complete,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_oidc_reports_structured_unavailable() {
        // arrange
        // act
        // assert
        // When
        let availability = evaluate_browser_oidc_availability();

        // Then: never claim Available without a real browser/OIDC backend
        assert!(availability.is_unavailable());
        assert!(!availability.is_available());
        match availability {
            BrowserOidcAvailability::Unavailable { reason } => {
                assert!(reason.contains("OIDC") || reason.contains("SSO"));
                assert!(reason.contains("not implemented"));
            }
            BrowserOidcAvailability::Available => panic!("must not claim available"),
        }
    }

    #[test]
    fn start_and_complete_fail_closed_with_operator_hints() {
        // arrange
        // act
        // assert
        // Given / When
        let start = start_browser_oidc_flow("https://issuer.example", "client-abc");
        let complete = complete_browser_oidc_flow("abcd1234secret");

        // Then
        match start {
            BrowserOidcStartResult::Unavailable {
                reason,
                issuer_hint,
                client_id_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(issuer_hint, "https://issuer.example");
                assert_eq!(client_id_hint, "client-abc");
            }
        }
        match complete {
            BrowserOidcCompleteResult::Unavailable {
                reason,
                authorization_code_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(authorization_code_hint, "abcd…");
                assert!(!authorization_code_hint.contains("secret"));
            }
        }
    }

    #[test]
    fn browser_oidc_operator_diagnostics_cover_availability_and_outcomes() {
        // arrange
        // act
        // assert
        // Given
        let availability = evaluate_browser_oidc_availability();
        let start = start_browser_oidc_flow("https://issuer.example", "client-abc");
        let complete = complete_browser_oidc_flow("abcd1234secret");

        // When
        let summary = summarize_browser_oidc_outcomes(Some(&start), Some(&complete));

        // Then
        assert!(availability
            .one_line()
            .contains("browser OIDC: unavailable"));
        assert!(start.one_line().contains("issuer=`https://issuer.example`"));
        assert!(start.one_line().contains("client=`client-abc`"));
        assert!(complete.one_line().contains("code=`abcd…`"));
        assert!(!complete.one_line().contains("secret"));
        assert_eq!(
            summary,
            BrowserOidcOutcomeSummary {
                start_unavailable: 1,
                complete_unavailable: 1,
                total: 2,
            }
        );
        assert!(summary.all_unavailable());
        assert!(summary.one_line().contains("2 total"));
    }

    #[test]
    fn multi_endpoint_product_probe_all_unavailable_with_redacted_codes() {
        // arrange
        // act
        // assert
        // Given / When
        let probe = probe_browser_oidc_product();

        // Then: 3× start + 3× complete, last-of-each summary total=2
        assert_eq!(probe.starts.len(), 3);
        assert_eq!(probe.completes.len(), 3);
        assert!(probe.availability.is_unavailable());
        assert!(probe.is_unavailable());
        assert_eq!(probe.summary.total, 2);
        assert!(probe.summary.all_unavailable());
        assert!(
            probe
                .last_start
                .one_line()
                .contains("issuer=`https://issuer.example`")
                && probe
                    .last_start
                    .one_line()
                    .contains("client=`(client-web)`"),
            "last start: {}",
            probe.last_start.one_line()
        );
        assert!(
            probe.last_complete.one_line().contains("code=`(pro…`"),
            "last complete: {}",
            probe.last_complete.one_line()
        );
        assert!(!probe.last_complete.one_line().contains("probe-device"));
        for start in &probe.starts {
            assert!(start.one_line().contains("unavailable"));
        }
        for complete in &probe.completes {
            assert!(complete.one_line().contains("unavailable"));
            assert!(!complete.one_line().contains("probe-device"));
            assert!(!complete.one_line().contains("probe-code"));
        }
    }
}
