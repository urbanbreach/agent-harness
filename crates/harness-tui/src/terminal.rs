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
pub mod key;
pub mod lifecycle;
pub mod multiplexer;
pub mod unicode_width;
pub mod writer;

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
pub use lifecycle::{
    AltScreenMode, ScreenBuffer, TeardownPlan, TerminalCapabilities, TerminalLifecycle,
    TerminalLifecycleError,
};
pub use multiplexer::TerminalMultiplexer;
pub use unicode_width::{char_display_width, UnicodeWidthEntry, UnicodeWidthRecord};
pub use writer::{
    SyncFrameGuard, SynchronizedWriter, BEGIN_SYNCHRONIZED_UPDATE, END_SYNCHRONIZED_UPDATE,
};
