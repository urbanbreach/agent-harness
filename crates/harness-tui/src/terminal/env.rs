//! Frozen environment snapshot used for terminal brand/multiplexer detection.
//!
//! Held separately from the brand and multiplexer enums so detection stays a
//! pure function over a value object. The matching helpers are `pub(crate)`:
//! internal detection inputs, not full public API.

/// A frozen snapshot of the environment variables used for detection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalEnv {
    pub term_program: Option<String>,
    pub term: Option<String>,
    pub lc_terminal: Option<String>,
    pub terminal_emulator: Option<String>,
    pub tmux: Option<String>,
    pub screen_sty: Option<String>,
    pub zellij: Option<String>,
    pub cmux: Option<String>,
    pub warp_session_id: Option<String>,
    pub kitty_window_id: Option<String>,
    pub ghostty_resources_dir: Option<String>,
    pub vte_version: Option<String>,
    pub terminator_uuid: Option<String>,
    pub wt_session: Option<String>,
    pub byobu: Option<String>,
    pub byobu_backend: Option<String>,
    pub cursor_session: Option<String>,
    pub windsurf: Option<String>,
    pub rio_log_level: Option<String>,
}

impl TerminalEnv {
    pub(crate) fn eq_ci(value: Option<&String>, needle: &str) -> bool {
        value.is_some_and(|v| normalize(v) == normalize(needle))
    }

    pub(crate) fn term_program_is(&self, needle: &str) -> bool {
        Self::eq_ci(self.term_program.as_ref(), needle)
    }

    pub(crate) fn lc_terminal_is(&self, needle: &str) -> bool {
        Self::eq_ci(self.lc_terminal.as_ref(), needle)
    }

    pub(crate) fn term_contains(&self, needle: &str) -> bool {
        self.term
            .as_deref()
            .is_some_and(|t| t.to_ascii_lowercase().contains(needle))
    }

    pub(crate) fn term_starts_with(&self, needle: &str) -> bool {
        self.term
            .as_deref()
            .is_some_and(|t| t.to_ascii_lowercase().starts_with(needle))
    }

    pub(crate) fn terminal_emulator_contains(&self, needle: &str) -> bool {
        self.terminal_emulator
            .as_deref()
            .is_some_and(|t| t.to_ascii_lowercase().contains(needle))
    }
}

/// Normalize a TERM_PROGRAM-style value: lowercase with hyphens, spaces, and
/// dots collapsed to underscores (`Apple_Terminal` → `apple_terminal`).
pub(crate) fn normalize(needle: &str) -> String {
    needle
        .to_ascii_lowercase()
        .chars()
        .map(|c| match c {
            '-' | ' ' | '.' => '_',
            other => other,
        })
        .collect()
}
