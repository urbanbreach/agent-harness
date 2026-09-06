//! Browser OIDC availability and the browser launcher shared by provider authentication.

use std::process::Command;

use serde::{Deserialize, Serialize};

/// Browser/device OIDC-SSO availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BrowserOidcAvailability {
    Available,
    Unavailable { reason: String },
}

impl BrowserOidcAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Available => "browser OIDC: available".to_string(),
            Self::Unavailable { reason } => {
                format!("browser OIDC: unavailable ({reason})")
            }
        }
    }
}

/// Enterprise OIDC has no public issuer configuration or authentication workflow.
pub fn evaluate_browser_oidc_availability() -> BrowserOidcAvailability {
    BrowserOidcAvailability::Unavailable {
        reason: "no OIDC issuer configured; browser OIDC workflow is not yet config-reachable"
            .to_string(),
    }
}

/// Launch a browser to open the given URL.
///
/// Uses `xdg-open` on Linux, `open` on macOS, `start` on Windows.
/// Returns `Ok(())` if the browser command was spawned, `Err` with a
/// manual-URL fallback message otherwise.
pub fn launch_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let browser_cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let browser_cmd = "open";
    #[cfg(target_os = "windows")]
    let browser_cmd = "start";

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        return Err(format!(
            "unsupported platform for browser launch; open manually: {url}"
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        match Command::new(browser_cmd).arg(url).spawn() {
            Ok(_) => Ok(()),
            Err(err) => Err(format!(
                "failed to launch browser ({browser_cmd}): {err}; open manually: {url}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_oidc_reports_unavailable_when_unconfigured() {
        let availability = evaluate_browser_oidc_availability();
        assert!(!availability.is_available());
        assert!(availability.is_unavailable());
        assert!(availability
            .one_line()
            .contains("no OIDC issuer configured"));
    }
}
