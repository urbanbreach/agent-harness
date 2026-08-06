//! Safe terminal title lifecycle: state derivation, animation, sanitation, reset.

pub mod sanitize;
pub mod states;
pub mod writer;

pub use sanitize::sanitize_title;
pub use states::{TitleActivity, TitlePhase, TitleState};
pub use writer::{TitleWriteError, TitleWriter};
