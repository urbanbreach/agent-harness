//! Terminal capability resolution with graceful degradation.
//!
//! Combines a detected [`TerminalName`], the active multiplexer, the alt-screen
//! policy, and TTY status into a [`TerminalContext`]. The context resolves the
//! `terminal_conditional` inventory rows (context-level capability methods) and
//! bridges onto the manifest [`TerminalCapabilityLeaf`] so rendering and input
//! decoding degrade gracefully on terminals that lack a feature.

use super::brand::TerminalName;
use super::capability::{ColorMode, KeyboardMode, TerminalCapabilityLeaf};
use super::env::TerminalEnv;
use super::lifecycle::AltScreenMode;
use super::multiplexer::TerminalMultiplexer;

/// Resolved terminal context: brand + multiplexer + policy + TTY status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalContext {
    pub brand: TerminalName,
    pub multiplexer: TerminalMultiplexer,
    pub alt_screen: AltScreenMode,
    pub is_tty: bool,
    pub is_byobu: bool,
}

impl TerminalContext {
    /// Probe a context purely from an environment snapshot and TTY status.
    pub fn probe(env: &TerminalEnv, is_tty: bool) -> Self {
        Self {
            brand: TerminalName::detect(env),
            multiplexer: TerminalMultiplexer::detect(env),
            alt_screen: AltScreenMode::Auto,
            is_tty,
            is_byobu: env.byobu.is_some(),
        }
    }

    pub const fn is_vte_based(&self) -> bool {
        self.brand.is_vte_based()
    }

    pub const fn is_tmux_backed(&self) -> bool {
        matches!(self.multiplexer, TerminalMultiplexer::Tmux)
    }

    /// The context runs inside byobu (which itself wraps tmux or screen).
    pub const fn byobu(&self) -> bool {
        self.is_byobu
    }

    /// The host repaints panes out of band, so the app must not assume it owns
    /// every paint cycle. True under a multiplexer or for self-rendering brands.
    pub const fn repaints_pane_out_of_band(&self) -> bool {
        self.multiplexer.is_detected() || matches!(self.brand, TerminalName::WarpTerminal)
    }

    /// Enabling mouse reporting causes sequences to leak as visible text on
    /// this terminal, so mouse capture must be disabled to avoid corruption.
    pub const fn mouse_reporting_leaks_as_raw_text(&self) -> bool {
        self.brand.is_capability_unclassified()
            || matches!(
                self.brand,
                TerminalName::AppleTerminal | TerminalName::JetBrains
            )
    }

    /// Shift+Enter cannot be distinguished from a bare Enter on this terminal.
    /// Note this is independent of generic enhanced-keyboard support: some
    /// terminals with the protocol still mishandle this specific binding.
    pub const fn shift_enter_unavailable(&self) -> bool {
        matches!(
            self.brand,
            TerminalName::AppleTerminal
                | TerminalName::Vte
                | TerminalName::Terminator
                | TerminalName::JetBrains
                | TerminalName::Otty
                | TerminalName::Unknown
                | TerminalName::WindowsTerminal
        )
    }

    /// Ctrl+. is captured by the OS or IME and cannot be relied upon.
    pub const fn ctrl_dot_unreliable(&self) -> bool {
        self.brand.is_vscode_family() || matches!(self.brand, TerminalName::AppleTerminal)
    }

    /// Capability CSI queries are expected to receive a trustworthy response.
    pub const fn csi_queries_available(&self) -> bool {
        !self.brand.intercepts_csi_queries() && !self.multiplexer.intercepts_csi_queries()
    }

    /// OSC 52 clipboard writes are usable in this context.
    pub const fn osc52_available(&self) -> bool {
        self.brand.supports_osc52_clipboard() && self.is_tty
    }

    /// Resolve the manifest capability leaf, degrading each feature per the
    /// conditionals above and the supplied TTY status.
    pub const fn resolve(&self, color_mode: ColorMode) -> TerminalCapabilityLeaf {
        let mouse_capture = self.is_tty && !self.mouse_reporting_leaks_as_raw_text();
        let focus_reporting = self.is_tty && !self.repaints_pane_out_of_band();
        let keyboard_mode = if self.brand.supports_enhanced_keyboard() {
            KeyboardMode::Enhanced
        } else {
            KeyboardMode::Legacy
        };
        TerminalCapabilityLeaf {
            color_mode,
            keyboard_mode,
            mouse_capture,
            bracketed_paste: self.is_tty,
            osc52_clipboard: self.osc52_available(),
            alternate_screen: engages_alt_screen(
                self.alt_screen,
                self.is_tty,
                self.multiplexer.is_detected(),
            ),
            focus_reporting,
        }
    }
}

/// Resolve the alternate-screen policy: `Auto` engages only on a bare TTY
/// (multiplexers may repaint the pane out of band), `Always` engages, `Never`
/// does not.
const fn engages_alt_screen(mode: AltScreenMode, is_tty: bool, is_multiplexed: bool) -> bool {
    match mode {
        AltScreenMode::Always => true,
        AltScreenMode::Never => false,
        AltScreenMode::Auto => is_tty && !is_multiplexed,
    }
}

/// One-shot fallback probe: environment + color mode + TTY → resolved leaf.
pub fn terminal_capability_fallback(
    env: &TerminalEnv,
    color_mode: ColorMode,
    is_tty: bool,
) -> TerminalCapabilityLeaf {
    TerminalContext::probe(env, is_tty).resolve(color_mode)
}
