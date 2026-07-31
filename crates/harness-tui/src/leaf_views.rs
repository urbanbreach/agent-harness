//! Deterministic leaf view helper types for TUI render shards.
//!
//! These types are plain value objects with no shared registry or app-state
//! dependency. They exist so TUI render shards can depend on a stable leaf
//! contract without pulling in the full `AppState` or `keybindings` registry.
//!
//! Integration owner: Todo 28. Leaf modules are contributed by Todos 22-24
//! and must not be edited by the integrator.

pub mod composer;
pub mod diff;
pub mod input;
pub mod key;
pub mod model;
pub mod overlay;
pub mod palette;
pub mod permission;
pub mod question;
pub mod session;
pub mod shell;
pub mod startup;
pub mod tool;
pub mod transcript;

pub use composer::ComposerLeafView;
pub use diff::DiffLeafView;
pub use input::{FocusOwner, InputLeafView};
pub use key::{FooterGrammar, KeyLeafView};
pub use model::ModelLeafView;
pub use overlay::OverlayLeafView;
pub use palette::PaletteLeafView;
pub use permission::{PermissionKindLeaf, PermissionLeafView, PermissionStateLeaf};
pub use question::{QuestionLeafView, QuestionStateLeaf};
pub use session::SessionLeafView;
pub use shell::{FocusLeaf, ShellKindLeaf, ShellLeafView};
pub use startup::{StartupLeafView, StartupPhase};
pub use tool::{ToolLeafView, ToolStatusLeaf};
pub use transcript::TranscriptLeafView;
