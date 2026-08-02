//! Recovery state surface — Todo 26.
//!
//! Standalone leaf module modeling provider-fail recovery, cancel,
//! permission-timeout, recovery-retry, and truncated/corrupt replay states.
//! Included via `#[path]` in the parity test.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryState {
    None,
    ProviderFail,
    Cancelled,
    PermissionTimeout,
    RecoveryRetry,
    TruncatedReplay,
    CorruptReplay,
}

impl RecoveryState {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "",
            Self::ProviderFail => "provider failure",
            Self::Cancelled => "cancelled",
            Self::PermissionTimeout => "permission timed out",
            Self::RecoveryRetry => "retrying",
            Self::TruncatedReplay => "truncated replay",
            Self::CorruptReplay => "corrupt replay",
        }
    }

    pub fn is_recoverable(self) -> bool {
        matches!(
            self,
            Self::ProviderFail | Self::Cancelled | Self::PermissionTimeout | Self::RecoveryRetry
        )
    }

    pub fn composer_disabled(self) -> bool {
        matches!(
            self,
            Self::RecoveryRetry | Self::TruncatedReplay | Self::CorruptReplay
        )
    }

    pub fn composer_hint(self) -> &'static str {
        match self {
            Self::None => "Type a prompt for the next turn\u{2026}",
            Self::ProviderFail => "After review, adjust the draft, then retry or continue.",
            Self::Cancelled => "Type a prompt to retry the cancelled turn\u{2026}",
            Self::PermissionTimeout => {
                "Permission timed out \u{2014} retry the request or adjust permissions."
            }
            Self::RecoveryRetry => "Recovery in progress \u{2014} wait for live state to catch up.",
            Self::TruncatedReplay => "Replay is truncated \u{2014} some events may be missing.",
            Self::CorruptReplay => "Replay is corrupt \u{2014} event log may be damaged.",
        }
    }

    pub fn from_status_banner(banner: &str) -> Self {
        let lower = banner.to_ascii_lowercase();
        if lower.contains("disconnected") {
            Self::ProviderFail
        } else if lower.contains("lagged") || lower.contains("replaying") {
            Self::RecoveryRetry
        } else if lower.contains("failed") || lower.contains("error") {
            Self::ProviderFail
        } else if lower.contains("cancelled") {
            Self::Cancelled
        } else if lower.contains("timeout") {
            Self::PermissionTimeout
        } else if lower.contains("truncated") {
            Self::TruncatedReplay
        } else if lower.contains("corrupt") {
            Self::CorruptReplay
        } else {
            Self::None
        }
    }

    pub fn retry_transition(self) -> Self {
        match self {
            Self::ProviderFail => Self::RecoveryRetry,
            Self::Cancelled => Self::None,
            Self::PermissionTimeout => Self::RecoveryRetry,
            Self::RecoveryRetry => Self::None,
            Self::TruncatedReplay | Self::CorruptReplay => self,
            Self::None => Self::None,
        }
    }
}
