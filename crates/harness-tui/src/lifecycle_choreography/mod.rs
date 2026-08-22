//! Unified lifecycle choreography across shell, composer, transcript, dashboard, and ambient UI.

pub mod coordinator;
mod state;
pub mod surface;
pub mod transitions;

pub use coordinator::{LifecycleAuthority, LifecycleSnapshot};
pub use state::LifecycleState;
pub use surface::{ActionAvailability, FocusDirective, SurfaceState};
pub use transitions::{TransitionError, TransitionTable};
