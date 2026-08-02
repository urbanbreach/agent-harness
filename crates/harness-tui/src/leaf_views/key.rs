//! Key / shortcut-footer leaf view.
//!
//! Models P0-KEY-01 (contextual shortcut footer). The footer grammar
//! changes with composer state: welcome footer vs draft footer.

/// Which footer grammar is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FooterGrammar {
    #[default]
    None,
    /// Welcome / idle footer shown before any draft text.
    Welcome,
    /// Draft footer shown after the user types (Enter:send | Shift+Tab:mode | Ctrl+x:shortcuts).
    Draft,
}

/// Deterministic view state for the shortcut footer.
///
/// No app-state or registry dependency — a plain `Copy` value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyLeafView {
    pub grammar: FooterGrammar,
    pub footer_visible: bool,
}

impl KeyLeafView {
    pub const fn new(grammar: FooterGrammar, visible: bool) -> Self {
        Self {
            grammar,
            footer_visible: visible,
        }
    }

    /// Derive the key leaf view from real composer state.
    ///
    /// `startup_shell_visible`: whether the startup welcome panel is showing.
    /// `prompt_has_draft`: whether the composer buffer has any text.
    pub fn from_state(startup_shell_visible: bool, prompt_has_draft: bool) -> Self {
        if startup_shell_visible && !prompt_has_draft {
            Self::new(FooterGrammar::Welcome, true)
        } else if prompt_has_draft {
            Self::new(FooterGrammar::Draft, true)
        } else {
            Self::new(FooterGrammar::Welcome, true)
        }
    }

    /// P0-KEY-01: the footer vocabulary changes with composer state.
    pub const fn footer_changes_with_composer(self) -> bool {
        matches!(self.grammar, FooterGrammar::Draft)
    }

    /// The expected draft footer shortcut tokens.
    pub const fn draft_footer_tokens(self) -> &'static [&'static str] {
        &["Enter", "Shift+Tab", "Ctrl+x"]
    }
}
