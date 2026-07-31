//! Terminal lifecycle leaf: raw mode, screen buffer, synchronized output, and
//! bracketed-paste state, gated by terminal capabilities.
//!
//! Pure state machine mirroring the deterministic lifecycle hooks the runtime
//! owns. Every transition is capability-gated and fail-closed: requesting a
//! mode the terminal does not support is an error, not a silent no-op.

/// Error returned when a lifecycle transition is invalid or unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalLifecycleError {
    /// Raw mode is not supported (capability absent).
    RawModeUnsupported,
    /// Attempted to leave raw mode while not in raw mode.
    RawModeNotActive,
    /// The alternate screen was requested unconditionally but is unsupported.
    AlternateScreenUnsupported,
    /// Synchronized output (DEC mode 2026) is not supported.
    SynchronizedOutputUnsupported,
    /// Attempted to disable synchronized output while not active.
    SynchronizedOutputNotActive,
    /// Bracketed paste mode is not supported.
    BracketedPasteUnsupported,
    /// Attempted to disable bracketed paste while not active.
    BracketedPasteNotActive,
}

impl core::fmt::Display for TerminalLifecycleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::RawModeUnsupported => "raw mode is not supported by this terminal",
            Self::RawModeNotActive => "raw mode is not active",
            Self::AlternateScreenUnsupported => "alternate screen is not supported",
            Self::SynchronizedOutputUnsupported => "synchronized output is not supported",
            Self::SynchronizedOutputNotActive => "synchronized output is not active",
            Self::BracketedPasteUnsupported => "bracketed paste is not supported",
            Self::BracketedPasteNotActive => "bracketed paste is not active",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for TerminalLifecycleError {}

/// Terminal capabilities that gate lifecycle transitions (pure input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilities {
    /// Raw mode entry is supported (implies a TTY).
    pub raw_mode: bool,
    /// The alternate screen buffer is supported.
    pub alternate_screen: bool,
    /// Synchronized output (DEC mode 2026) is supported.
    pub synchronized_output: bool,
    /// Bracketed paste mode is supported.
    pub bracketed_paste: bool,
}

impl TerminalCapabilities {
    /// A fully capable terminal.
    pub const fn full() -> Self {
        Self {
            raw_mode: true,
            alternate_screen: true,
            synchronized_output: true,
            bracketed_paste: true,
        }
    }

    /// A terminal with no optional capabilities.
    pub const fn none() -> Self {
        Self {
            raw_mode: false,
            alternate_screen: false,
            synchronized_output: false,
            bracketed_paste: false,
        }
    }
}

impl Default for TerminalCapabilities {
    fn default() -> Self {
        Self::none()
    }
}

/// Alternate-screen policy, mirroring the Auto/Always/Never terminal
/// conditional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AltScreenMode {
    /// Enter the alternate screen only when the terminal supports it.
    #[default]
    Auto,
    /// Always enter the alternate screen (error if unsupported).
    Always,
    /// Never enter the alternate screen.
    Never,
}

/// The screen buffer currently in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenBuffer {
    /// The primary (main) screen buffer.
    #[default]
    Main,
    /// The alternate screen buffer.
    Alternate,
}

/// Reversals required to restore the terminal to its pre-launch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TeardownPlan {
    /// Raw mode must be disabled.
    pub disable_raw_mode: bool,
    /// The terminal must leave the alternate screen.
    pub leave_alternate_screen: bool,
    /// Synchronized output must be disabled.
    pub disable_synchronized_output: bool,
    /// Bracketed paste must be disabled.
    pub disable_bracketed_paste: bool,
}

/// Aggregate terminal lifecycle state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalLifecycle {
    raw_mode_active: bool,
    buffer: ScreenBuffer,
    synchronized_active: bool,
    bracketed_paste_active: bool,
}

impl TerminalLifecycle {
    /// A fresh lifecycle in the terminal's default (cooked, main-screen) state.
    pub const fn new() -> Self {
        Self {
            raw_mode_active: false,
            buffer: ScreenBuffer::Main,
            synchronized_active: false,
            bracketed_paste_active: false,
        }
    }

    /// Whether raw mode is currently active.
    pub const fn is_raw_mode_active(&self) -> bool {
        self.raw_mode_active
    }

    /// The active screen buffer.
    pub const fn screen_buffer(&self) -> ScreenBuffer {
        self.buffer
    }

    /// Whether synchronized output is currently active.
    pub const fn is_synchronized_active(&self) -> bool {
        self.synchronized_active
    }

    /// Whether bracketed paste is currently active.
    pub const fn is_bracketed_paste_active(&self) -> bool {
        self.bracketed_paste_active
    }

    /// Enter raw mode. Idempotent; fails closed when the capability is absent.
    pub fn enter_raw_mode(
        &mut self,
        caps: &TerminalCapabilities,
    ) -> Result<(), TerminalLifecycleError> {
        if !caps.raw_mode {
            return Err(TerminalLifecycleError::RawModeUnsupported);
        }
        self.raw_mode_active = true;
        Ok(())
    }

    /// Leave raw mode. Fails when not currently active.
    pub fn exit_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        if !self.raw_mode_active {
            return Err(TerminalLifecycleError::RawModeNotActive);
        }
        self.raw_mode_active = false;
        Ok(())
    }

    /// Resolve the alternate-screen policy against terminal capability and
    /// switch buffers. `Never`, and unsupported-under-`Auto`, stay on the main
    /// screen; `Always` on an unsupported terminal fails closed.
    pub fn enter_alternate_screen(
        &mut self,
        caps: &TerminalCapabilities,
        mode: AltScreenMode,
    ) -> Result<(), TerminalLifecycleError> {
        let desired = match mode {
            AltScreenMode::Auto => caps.alternate_screen,
            AltScreenMode::Always => true,
            AltScreenMode::Never => false,
        };
        if desired && !caps.alternate_screen {
            return Err(TerminalLifecycleError::AlternateScreenUnsupported);
        }
        self.buffer = if desired {
            ScreenBuffer::Alternate
        } else {
            ScreenBuffer::Main
        };
        Ok(())
    }

    /// Leave the alternate screen, returning to the main buffer (idempotent).
    pub fn leave_alternate_screen(&mut self) {
        self.buffer = ScreenBuffer::Main;
    }

    /// Enable synchronized output (DEC mode 2026). Idempotent; fails closed
    /// when unsupported.
    pub fn enable_synchronized_output(
        &mut self,
        caps: &TerminalCapabilities,
    ) -> Result<(), TerminalLifecycleError> {
        if !caps.synchronized_output {
            return Err(TerminalLifecycleError::SynchronizedOutputUnsupported);
        }
        self.synchronized_active = true;
        Ok(())
    }

    /// Disable synchronized output. Fails when not active.
    pub fn disable_synchronized_output(&mut self) -> Result<(), TerminalLifecycleError> {
        if !self.synchronized_active {
            return Err(TerminalLifecycleError::SynchronizedOutputNotActive);
        }
        self.synchronized_active = false;
        Ok(())
    }

    /// Enable bracketed paste. Idempotent; fails closed when unsupported.
    pub fn enable_bracketed_paste(
        &mut self,
        caps: &TerminalCapabilities,
    ) -> Result<(), TerminalLifecycleError> {
        if !caps.bracketed_paste {
            return Err(TerminalLifecycleError::BracketedPasteUnsupported);
        }
        self.bracketed_paste_active = true;
        Ok(())
    }

    /// Disable bracketed paste. Fails when not active.
    pub fn disable_bracketed_paste(&mut self) -> Result<(), TerminalLifecycleError> {
        if !self.bracketed_paste_active {
            return Err(TerminalLifecycleError::BracketedPasteNotActive);
        }
        self.bracketed_paste_active = false;
        Ok(())
    }

    /// The reversals required to restore the terminal to its pre-launch state.
    pub const fn teardown_plan(&self) -> TeardownPlan {
        TeardownPlan {
            disable_raw_mode: self.raw_mode_active,
            leave_alternate_screen: matches!(self.buffer, ScreenBuffer::Alternate),
            disable_synchronized_output: self.synchronized_active,
            disable_bracketed_paste: self.bracketed_paste_active,
        }
    }
}
