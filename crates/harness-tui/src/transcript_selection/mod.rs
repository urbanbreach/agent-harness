#![allow(
    clippy::mod_module_files,
    reason = "Task 27 requires a focused directory facade"
)]

mod copy_metadata;
mod grapheme;
mod hyperlinks;
mod keyboard;
mod local_clipboard;
mod osc52;
mod selection;
mod selection_types;

pub use copy_metadata::{BlockKind, CopyMetadata, CopyMetadataPolicy, copy_with_metadata};
pub use hyperlinks::{Hyperlink, HyperlinkError, HyperlinkMap, LinkRange, hyperlink_sequence};
pub use local_clipboard::{
    ClipboardCommand, LocalClipboardError, LocalPlatform, copy_local, copy_local_with_runner,
};
pub use osc52::{OSC52_MAX_BYTES, Osc52Error, TmuxSequence, build_osc52, route_osc52, wrap_tmux};
pub use selection::WrappedText;
pub use selection_types::{
    Autoscroll, CellPoint, DragResult, Grapheme, GraphemeRange, NavigationKey, SelectionError,
    SelectionMode, SelectionRange, Viewport,
};
