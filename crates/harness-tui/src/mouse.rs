//! Mouse interaction leaf types for the TUI responsive/terminal shard.
//!
//! These are plain value objects with no shared registry or app-state
//! dependency. They capture the mouse capture state and interaction
//! modes that the TERM-CAP-MOUSE manifest row requires.

/// Mouse capture mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseCaptureMode {
    /// Mouse capture is disabled (legacy terminal or user preference).
    #[default]
    Disabled,
    /// Normal mouse tracking (button events only).
    Normal,
    /// Button-event tracking (drag/scroll).
    ButtonEvent,
    /// All mouse tracking (motion + button + scroll).
    All,
}

impl MouseCaptureMode {
    pub const fn is_enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub const fn supports_scroll(self) -> bool {
        matches!(self, Self::ButtonEvent | Self::All)
    }

    pub const fn supports_drag(self) -> bool {
        matches!(self, Self::ButtonEvent | Self::All)
    }
}

/// Mouse interaction leaf — a pure value type for mouse state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseLeaf {
    pub capture_mode: MouseCaptureMode,
    /// Wheel scroll is active for transcript/terminal panel.
    pub wheel_scroll_enabled: bool,
    /// Click-to-focus is active.
    pub click_focus_enabled: bool,
    /// Selection drag is active.
    pub selection_drag_enabled: bool,
}

impl MouseLeaf {
    /// Full mouse support (all features enabled).
    pub const fn full() -> Self {
        Self {
            capture_mode: MouseCaptureMode::All,
            wheel_scroll_enabled: true,
            click_focus_enabled: true,
            selection_drag_enabled: true,
        }
    }

    /// No mouse support (legacy terminal).
    pub const fn disabled() -> Self {
        Self {
            capture_mode: MouseCaptureMode::Disabled,
            wheel_scroll_enabled: false,
            click_focus_enabled: false,
            selection_drag_enabled: false,
        }
    }

    /// Reduced mouse support (button events only, no drag/scroll).
    pub const fn reduced() -> Self {
        Self {
            capture_mode: MouseCaptureMode::Normal,
            wheel_scroll_enabled: false,
            click_focus_enabled: true,
            selection_drag_enabled: false,
        }
    }
}

impl Default for MouseLeaf {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Which physical button a mouse event refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Wheel scroll direction for a scroll event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// The semantic kind of a mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    /// A button was pressed.
    Down(MouseButton),
    /// A button was released.
    Up(MouseButton),
    /// Pointer moved while a button was held.
    Drag(MouseButton),
    /// Pointer moved with no button held.
    Moved,
    /// A wheel/scroll event.
    Scroll(MouseScrollDirection),
}

/// A decoded mouse event with zero-based cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
}

impl MouseEvent {
    pub const fn new(kind: MouseEventKind, column: u16, row: u16) -> Self {
        Self { kind, column, row }
    }

    /// Shared bit layout for the decoded button code (after any 0x20 / wire
    /// un-offsetting has already been applied by the caller).
    ///
    /// - bits 0..1: button (0=Left, 1=Middle, 2=Right, 3=release/none)
    /// - bit 2 (4): Shift, bit 3 (8): Alt/Meta, bit 4 (16): Ctrl
    /// - bit 5 (32): motion
    /// - bit 6 (64): wheel
    fn kind_from_button_code(code: u16, motion: bool, press: bool) -> MouseEventKind {
        const MOTION: u16 = 32;
        const WHEEL: u16 = 64;
        // Wheel events must be tested first: their direction is the low 2 bits
        // (0=Up, 1=Down, 2=Left, 3=Right), and low bits == 3 would otherwise be
        // misread as a release.
        if code & WHEEL != 0 {
            return match code & 3 {
                0 => MouseEventKind::Scroll(MouseScrollDirection::Up),
                1 => MouseEventKind::Scroll(MouseScrollDirection::Down),
                2 => MouseEventKind::Scroll(MouseScrollDirection::Left),
                _ => MouseEventKind::Scroll(MouseScrollDirection::Right),
            };
        }
        let button = match code & 3 {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            // low bits == 3 is "release / no button" in legacy encoding.
            _ => {
                return if code & MOTION != 0 {
                    MouseEventKind::Moved
                } else {
                    // X10 release does not encode the button; Left is the
                    // conventional lossy default.
                    MouseEventKind::Up(MouseButton::Left)
                };
            }
        };
        if code & MOTION != 0 || motion {
            MouseEventKind::Drag(button)
        } else if press {
            MouseEventKind::Down(button)
        } else {
            MouseEventKind::Up(button)
        }
    }
}

/// Decode an SGR (mode 1006) mouse payload: `ESC [ <cb;cx;cy M|m`.
///
/// `cb`/`cx`/`cy` are the decoded decimal parameters (not byte-offset);
/// `press` is `true` for the `M` terminator and `false` for `m` (release).
/// Coordinates are converted to zero-based cell positions.
pub fn decode_sgr(cb: u16, cx: u16, cy: u16, press: bool) -> MouseEvent {
    let column = cx.saturating_sub(1);
    let row = cy.saturating_sub(1);
    let motion = cb & 32 != 0;
    let kind = MouseEvent::kind_from_button_code(cb, motion, press);
    MouseEvent::new(kind, column, row)
}

/// Decode a legacy X10/UTF-8 (default) mouse payload: `ESC [ M <3 bytes>`.
///
/// The three raw bytes are each value + 0x20; this function un-offsets them,
/// converts coordinates to zero-based, and decodes the button code.
pub fn decode_legacy(cb_raw: u8, cx_raw: u8, cy_raw: u8) -> MouseEvent {
    let cb = u16::from(cb_raw.saturating_sub(0x20));
    let column = u16::from(cx_raw.saturating_sub(0x20)).saturating_sub(1);
    let row = u16::from(cy_raw.saturating_sub(0x20)).saturating_sub(1);
    let motion = cb & 32 != 0;
    // Legacy presses and releases are both `M`; press is inferred as "not a
    // release code". kind_from_button_code maps the release code to Up(Left).
    let press = cb & 3 != 3;
    let kind = MouseEvent::kind_from_button_code(cb, motion, press);
    MouseEvent::new(kind, column, row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_mouse_has_all_features() {
        // arrange
        // act
        let leaf = MouseLeaf::full();

        // assert
        assert!(leaf.capture_mode.is_enabled());
        assert!(leaf.capture_mode.supports_scroll());
        assert!(leaf.capture_mode.supports_drag());
        assert!(leaf.wheel_scroll_enabled);
        assert!(leaf.click_focus_enabled);
        assert!(leaf.selection_drag_enabled);
    }

    #[test]
    fn disabled_mouse_has_no_features() {
        // arrange
        // act
        let leaf = MouseLeaf::disabled();

        // assert
        assert!(!leaf.capture_mode.is_enabled());
        assert!(!leaf.wheel_scroll_enabled);
        assert!(!leaf.click_focus_enabled);
        assert!(!leaf.selection_drag_enabled);
    }

    #[test]
    fn reduced_mouse_has_click_only() {
        // arrange
        // act
        let leaf = MouseLeaf::reduced();

        // assert
        assert!(leaf.capture_mode.is_enabled());
        assert!(!leaf.capture_mode.supports_scroll());
        assert!(!leaf.capture_mode.supports_drag());
        assert!(!leaf.wheel_scroll_enabled);
        assert!(leaf.click_focus_enabled);
        assert!(!leaf.selection_drag_enabled);
    }

    #[test]
    fn button_event_mode_supports_scroll_and_drag() {
        // arrange
        // act
        // assert
        assert!(MouseCaptureMode::ButtonEvent.supports_scroll());
        assert!(MouseCaptureMode::ButtonEvent.supports_drag());
    }

    #[test]
    fn normal_mode_does_not_support_scroll_or_drag() {
        // arrange
        // act
        // assert
        assert!(!MouseCaptureMode::Normal.supports_scroll());
        assert!(!MouseCaptureMode::Normal.supports_drag());
    }
}
