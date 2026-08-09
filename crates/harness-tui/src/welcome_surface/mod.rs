//! Responsive welcome surface: hero, menu, prompt, and status hierarchy.

pub mod hit_map;
pub mod layout;
pub mod state;

pub use hit_map::{WelcomeHit, WelcomeHitMap};
pub use layout::{WelcomeLayout, WelcomeRegion};
pub use state::{InputResult, WelcomeAction, WelcomeFocus, WelcomeInput, WelcomeState};
