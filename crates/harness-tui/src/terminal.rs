//! Terminal capability leaf types for the TUI responsive/terminal shard.
//!
//! These are plain value objects with no shared registry or app-state
//! dependency. They capture the deterministic terminal capability modes
//! (TERM-CAP-COLOR, TERM-CAP-KEYS, TERM-CAP-MOUSE, TERM-CAP-CLIPBOARD)
//! and Unicode width recording that the manifest terminal rows require.

pub mod brand;
pub mod capability;
pub mod cursor;
pub mod decode;
pub mod env;
pub mod event;
pub mod fallback;
pub mod frame_clock;
pub mod frame_output;
pub mod key;
pub mod lifecycle;
pub mod multiplexer;
pub mod presenter;
pub mod unicode_width;
pub mod writer;

use std::io::IsTerminal;

use crate::capability_matrix::{CapabilityClassifier, CapabilityMatrix};
pub use brand::TerminalName;
pub use capability::{
    ColorMode, KeyboardMode, TerminalCapabilityLeaf, TerminalCapabilityRecord,
    TerminalCapabilityRow,
};
pub use cursor::{CursorPosition, CursorShape, CursorState};
pub use decode::{decode_all, Decoder};
pub use env::TerminalEnv;
pub use event::{FocusEvent, KeyCode, KeyEvent, KeyModifiers, ResizeEvent, TerminalInputEvent};
pub use fallback::{terminal_capability_fallback, TerminalContext};
pub use frame_clock::{FrameClock, FramePhase, DEFAULT_FRAME_TICK_MS};
pub use frame_output::{
    FrameAck, FrameAckOutcome, FrameBackendMetrics, FrameKind, FrameOutput, FrameOutputBackend,
    FrameOutputFailure, FrameOutputMetrics, FrameOutputReceiver, FrameOutputWriter,
    FrameSubmission, FrameWriteStage, SerializedFrame,
};
pub use lifecycle::{
    AltScreenMode, ScreenBuffer, TeardownPlan, TerminalCapabilities, TerminalLifecycle,
    TerminalLifecycleError,
};
pub use multiplexer::TerminalMultiplexer;
pub use presenter::Presenter;
pub use unicode_width::{char_display_width, UnicodeWidthEntry, UnicodeWidthRecord};
pub use writer::{
    SyncFrameGuard, SynchronizedWriter, BEGIN_SYNCHRONIZED_UPDATE, END_SYNCHRONIZED_UPDATE,
};

pub(crate) struct ProductionTerminalSession {
    pub context: TerminalContext,
    pub capabilities: TerminalCapabilityLeaf,
    pub matrix: CapabilityMatrix,
    pub lifecycle: TerminalLifecycle,
    pub focused: bool,
    pub suspended: bool,
}

impl ProductionTerminalSession {
    pub fn negotiate() -> Self {
        let env = TerminalEnv {
            term_program: std::env::var("TERM_PROGRAM").ok(),
            term: std::env::var("TERM").ok(),
            lc_terminal: std::env::var("LC_TERMINAL").ok(),
            terminal_emulator: std::env::var("TERMINAL_EMULATOR").ok(),
            tmux: std::env::var("TMUX").ok(),
            screen_sty: std::env::var("STY").ok(),
            zellij: std::env::var("ZELLIJ").ok(),
            cmux: std::env::var("CMUX").ok(),
            warp_session_id: std::env::var("WARP_SESSION_ID").ok(),
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").ok(),
            ghostty_resources_dir: std::env::var("GHOSTTY_RESOURCES_DIR").ok(),
            vte_version: std::env::var("VTE_VERSION").ok(),
            terminator_uuid: std::env::var("TERMINATOR_UUID").ok(),
            wt_session: std::env::var("WT_SESSION").ok(),
            byobu: std::env::var("BYOBU_BACKEND").ok(),
            byobu_backend: std::env::var("BYOBU_BACKEND").ok(),
            cursor_session: std::env::var("CURSOR_SESSION").ok(),
            windsurf: std::env::var("WINDSURF_SESSION").ok(),
            rio_log_level: std::env::var("RIO_LOG_LEVEL").ok(),
        };
        let is_tty = std::io::stdout().is_terminal();
        let context = TerminalContext::probe(&env, is_tty);
        let color_mode = ColorMode::from_env(
            std::env::var("COLORTERM").ok().as_deref(),
            env.term.as_deref(),
        );
        let capabilities = context.resolve(color_mode);
        let classifier = CapabilityClassifier::new(
            env.term.clone().unwrap_or_default(),
            env.term_program.clone().unwrap_or_default(),
            std::env::var("COLORTERM").unwrap_or_default(),
            env.tmux.is_some(),
            env.zellij.is_some(),
            std::env::var_os("SSH_CONNECTION").is_some(),
            env.wt_session.is_some(),
            std::env::var_os("NO_COLOR").is_some(),
            env.vte_version
                .as_deref()
                .and_then(|version| version.parse().ok()),
        );
        Self {
            context,
            capabilities,
            matrix: CapabilityMatrix::new(classifier),
            lifecycle: TerminalLifecycle::new(),
            focused: true,
            suspended: false,
        }
    }

    pub fn record_setup(&mut self, raw_mode: bool, alternate_screen: bool, paste: bool) {
        let lifecycle_caps = TerminalCapabilities {
            raw_mode,
            alternate_screen,
            synchronized_output: true,
            bracketed_paste: paste,
        };
        if raw_mode {
            let _ = self.lifecycle.enter_raw_mode(&lifecycle_caps);
        }
        if alternate_screen {
            let _ = self
                .lifecycle
                .enter_alternate_screen(&lifecycle_caps, AltScreenMode::Always);
        }
        let _ = self.lifecycle.enable_synchronized_output(&lifecycle_caps);
        if paste {
            let _ = self.lifecycle.enable_bracketed_paste(&lifecycle_caps);
        }
    }

    pub fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
        self.suspended = !focused;
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    pub fn restore(&mut self) {
        self.suspended = false;
    }

    pub fn finish(&mut self) {
        if self.lifecycle.is_bracketed_paste_active() {
            let _ = self.lifecycle.disable_bracketed_paste();
        }
        if self.lifecycle.is_synchronized_active() {
            let _ = self.lifecycle.disable_synchronized_output();
        }
        if self.lifecycle.is_raw_mode_active() {
            let _ = self.lifecycle.exit_raw_mode();
        }
        self.lifecycle.leave_alternate_screen();
        self.suspended = false;
    }
}
