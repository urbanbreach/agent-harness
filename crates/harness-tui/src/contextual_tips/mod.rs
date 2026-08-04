//! Contextual tips, shortcut help, and guidance lifecycle.

pub mod lifecycle;
pub mod priority;
pub mod triggers;

pub use lifecycle::{TipDismissal, TipLifetime, TipManager};
pub use priority::{TipPriority, TipTrigger};
pub use triggers::{TipContext, TipId};
