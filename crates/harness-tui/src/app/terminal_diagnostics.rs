//! Terminal diagnostic output with FPS/scroll debug behind explicit controls
//! and unsupported capability reporting.
//!
//! No network calls. No telemetry, no analytics, no hosted content fetching.

/// A terminal capability that is not supported by the current terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedCapability {
    /// Color support is missing or limited.
    Color,
    /// OSC 52 clipboard is not available.
    Clipboard,
    /// Mouse capture is not available.
    Mouse,
}

/// Terminal diagnostics state — tracks debug flags and unsupported capabilities.
#[derive(Debug, Clone, Default)]
pub struct TerminalDiagnostics {
    fps_debug: bool,
    scroll_debug: bool,
    unsupported: Vec<UnsupportedCapability>,
}

impl TerminalDiagnostics {
    /// Create a new diagnostics state with all debug flags off.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if FPS debug overlay is enabled.
    pub fn fps_debug_enabled(&self) -> bool {
        self.fps_debug
    }

    /// Enable FPS debug overlay.
    pub fn enable_fps_debug(&mut self) {
        self.fps_debug = true;
    }

    /// Disable FPS debug overlay.
    pub fn disable_fps_debug(&mut self) {
        self.fps_debug = false;
    }

    /// Returns true if scroll debug overlay is enabled.
    pub fn scroll_debug_enabled(&self) -> bool {
        self.scroll_debug
    }

    /// Enable scroll debug overlay.
    pub fn enable_scroll_debug(&mut self) {
        self.scroll_debug = true;
    }

    /// Disable scroll debug overlay.
    pub fn disable_scroll_debug(&mut self) {
        self.scroll_debug = false;
    }

    /// Report an unsupported capability.
    pub fn report_unsupported(&mut self, cap: UnsupportedCapability) {
        if !self.unsupported.contains(&cap) {
            self.unsupported.push(cap);
        }
    }

    /// Returns the list of unsupported capabilities.
    pub fn unsupported_capabilities(&self) -> &[UnsupportedCapability] {
        &self.unsupported
    }

    /// Produce diagnostic output lines for display.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push("Terminal Diagnostics:".to_string());

        let color_label = if self.unsupported.contains(&UnsupportedCapability::Color) {
            "unsupported"
        } else {
            "supported"
        };
        lines.push(format!("  color: {color_label}"));

        let clipboard_label = if self.unsupported.contains(&UnsupportedCapability::Clipboard) {
            "unsupported"
        } else {
            "supported"
        };
        lines.push(format!("  clipboard: {clipboard_label}"));

        let mouse_label = if self.unsupported.contains(&UnsupportedCapability::Mouse) {
            "unsupported"
        } else {
            "supported"
        };
        lines.push(format!("  mouse: {mouse_label}"));

        if self.fps_debug {
            lines.push("  fps debug: enabled".to_string());
        }
        if self.scroll_debug {
            lines.push("  scroll debug: enabled".to_string());
        }

        lines
    }
}
