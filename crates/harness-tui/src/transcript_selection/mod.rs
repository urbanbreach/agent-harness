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

pub use copy_metadata::{
    copy_with_metadata, copy_with_metadata_and_links, BlockKind, CopyMetadata, CopyMetadataPolicy,
};
pub(crate) use hyperlinks::safe_external_url;
pub use hyperlinks::{hyperlink_sequence, Hyperlink, HyperlinkError, HyperlinkMap, LinkRange};
pub use local_clipboard::{
    copy_local, copy_local_with_runner, ClipboardCommand, LocalClipboardError, LocalPlatform,
};
pub use osc52::{build_osc52, route_osc52, wrap_tmux, Osc52Error, TmuxSequence, OSC52_MAX_BYTES};
pub use selection::WrappedText;
pub use selection_types::{
    Autoscroll, CellPoint, DragResult, Grapheme, GraphemeRange, NavigationKey, SelectionError,
    SelectionMode, SelectionRange, Viewport,
};
