use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BrowserOidcAvailability {
    Available,
    Unavailable { reason: String },
}
impl BrowserOidcAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available => "browser OIDC: available".into(),
            Self::Unavailable { reason } => format!("browser OIDC: unavailable ({reason})"),
        }
    }
}
pub fn evaluate_browser_oidc_availability() -> BrowserOidcAvailability {
    BrowserOidcAvailability::Unavailable {
        reason: "no OIDC issuer configured".into(),
    }
}
pub fn launch_browser(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "invalid browser URL")?;
    if url.chars().any(char::is_control)
        || !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("browser URL must use HTTP(S) without embedded credentials or controls".into());
    }
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(unix, windows)))]
    return Err("browser launcher is unavailable on this platform".into());
    #[cfg(any(unix, windows))]
    {
        command
            .arg(parsed.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        // Reap the launcher without keeping the authentication UI waiting for a browser process.
        std::thread::Builder::new()
            .name("browser-launch".into())
            .spawn(move || match command.spawn() {
                Ok(mut child) => {
                    let _ = tx.send(Ok(()));
                    let _ = child.wait();
                }
                Err(_) => {
                    let _ = tx.send(Err(
                        "browser launcher failed; open the authorization URL manually".to_owned(),
                    ));
                }
            })
            .map_err(|_| "cannot start browser launcher")?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "browser launcher did not respond")?
    }
}
