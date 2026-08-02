//! Typed terminal input event model for the input decoder.
//!
//! These are the decoded semantic events produced by [`super::decode`] from a
//! raw ANSI/xterm byte stream. They are intentionally crossterm-independent so
//! the decoder stays a pure, testable contract with no backend coupling.

use crate::mouse::MouseEvent;

/// Modifier state for a decoded key event.
///
/// Bit flags follow the xterm/Kitty modifier encoding offset: the encoded
/// modifier parameter on the wire is `1 + SHIFT + ALT + CTRL + META`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const ALT: Self = Self(2);
    pub const CTRL: Self = Self(4);
    pub const META: Self = Self(8);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b1111)
    }

    /// Decode the xterm/Kitty on-the-wire modifier parameter.
    ///
    /// The wire value is `1 + sum(active modifiers)`, so `1` means "no
    /// modifier", `2` is Shift, `5` is Ctrl, etc. Values below 1 are clamped to
    /// none.
    pub const fn from_xterm_param(param: u16) -> Self {
        if param <= 1 {
            Self::NONE
        } else {
            // Low byte of `param - 1`; `from_bits` masks to the 4 modifier bits.
            Self::from_bits((param - 1).to_le_bytes()[0])
        }
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn shift(self) -> bool {
        self.contains(Self::SHIFT)
    }

    pub const fn alt(self) -> bool {
        self.contains(Self::ALT)
    }

    pub const fn ctrl(self) -> bool {
        self.contains(Self::CTRL)
    }
}

/// A decoded key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    /// A printable character.
    Char(char),
    /// Function key `F(1..=20)`.
    F(u8),
    /// Enter / Return (CR or LF).
    Enter,
    /// Tab.
    Tab,
    /// Backspace (C0 0x08 or DEL 0x7F depending on terminal mode).
    Backspace,
    /// Delete (forward).
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Cursor Up.
    Up,
    /// Cursor Down.
    Down,
    /// Cursor Left.
    Left,
    /// Cursor Right.
    Right,
    /// Bare Escape (no following recognized sequence).
    Esc,
    /// NUL byte (Ctrl+Space / Ctrl+@).
    Null,
}

/// A decoded key event: a code plus its modifier state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyEvent {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn plain(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    pub const fn char(c: char) -> Self {
        Self::plain(KeyCode::Char(c))
    }
}

/// Terminal focus transition (DECFOCUS: `CSI I` / `CSI O`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEvent {
    /// Terminal window gained focus.
    Gained,
    /// Terminal window lost focus.
    Lost,
}

/// A terminal resize report (columns, rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEvent {
    pub cols: u16,
    pub rows: u16,
}

impl ResizeEvent {
    pub const fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

/// A fully decoded terminal input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalInputEvent {
    /// A keyboard event.
    Key(KeyEvent),
    /// A mouse event (button, drag, scroll, or motion).
    Mouse(MouseEvent),
    /// A focus transition.
    Focus(FocusEvent),
    /// A bracketed-paste payload (the literal text between `200~` and `201~`).
    Paste(String),
    /// A terminal resize report.
    Resize(ResizeEvent),
    /// An unrecognized byte run preserved verbatim for diagnostics.
    Unknown(Vec<u8>),
}
