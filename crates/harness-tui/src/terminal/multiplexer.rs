//! Terminal multiplexer detection (tmux / screen / zellij / cmux / byobu).

use super::env::TerminalEnv;

/// A multiplexer (terminal multiplexer / session manager) the terminal runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TerminalMultiplexer {
    Tmux,
    Screen,
    Zellij,
    Cmux,
    #[default]
    Undetected,
}

impl TerminalMultiplexer {
    pub const ALL: [Self; 5] = [
        Self::Tmux,
        Self::Screen,
        Self::Zellij,
        Self::Cmux,
        Self::Undetected,
    ];

    pub fn detect(env: &TerminalEnv) -> Self {
        if env.byobu.is_some() {
            return match env.byobu_backend.as_deref() {
                Some(backend) if backend.contains("screen") => Self::Screen,
                _ => Self::Tmux,
            };
        }
        if env.cmux.is_some() {
            return Self::Cmux;
        }
        if env.tmux.is_some() {
            return Self::Tmux;
        }
        if env.zellij.is_some() {
            return Self::Zellij;
        }
        if env.screen_sty.is_some() {
            return Self::Screen;
        }
        Self::Undetected
    }

    pub const fn intercepts_csi_queries(self) -> bool {
        matches!(self, Self::Tmux | Self::Screen | Self::Zellij)
    }

    pub const fn is_detected(self) -> bool {
        !matches!(self, Self::Undetected)
    }
}
