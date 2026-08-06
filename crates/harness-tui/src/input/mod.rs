#![allow(
    clippy::mod_module_files,
    reason = "Task 12 requires a directory facade with mod.rs"
)]

pub mod ctrl_c;
pub mod esc;
pub mod key;
pub mod normalizer;
pub mod paste;
pub mod resize;

pub use ctrl_c::{CtrlCAction, CtrlCTracker, DEFAULT_CTRL_C_WINDOW};
pub use esc::{EscAction, EscLayer, EscRouter};
pub use key::{KeyProtocol, ModifierVariant, NormalizedKey, KEY_PROTOCOLS, MODIFIER_VARIANTS};
pub use normalizer::{InputNormalizer, NormalizedInput, NormalizerError};
pub use paste::{
    NormalizedPaste, PasteDetector, PasteKind, PasteOutput, PASTE_BURST_WINDOW, PASTE_START_WINDOW,
};
pub use resize::{ResizeDebouncer, RESIZE_DEBOUNCE};
