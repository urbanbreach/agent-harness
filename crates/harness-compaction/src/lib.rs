//! Pure context-compaction algorithms for Harness.
//!
//! Nothing here performs I/O, reads clocks or awaits. The coordinator gathers entries,
//! times and provider responses, calls these functions, and owns every side effect:
//! provider requests, journal events, artifacts and live progress.
pub mod archive;
pub mod cut;
pub mod files;
pub mod item;
pub mod method;
pub mod payload;
pub mod policy;
pub mod prepare;
pub mod prompts;
pub mod prune;
pub mod request;
pub mod retry;
pub mod serialize;
pub mod shake;
pub mod tokens;

pub use harness_tokenizer as tokenizer;
pub use item::{
    ImageDetail, ImageMeta, ImageOrigin, Item, ItemKind, ItemPart, NoteKind, Origin, Stop,
    ToolFacts,
};
pub use method::{CompactionMethod, ManualMode, ShakeMode};
