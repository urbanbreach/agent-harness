#![allow(
    clippy::mod_module_files,
    reason = "The task contract requires this directory facade to be named mod.rs."
)]

pub mod intent;
pub mod render_purity;
pub mod state;
pub mod table_data;
pub mod transitions;

pub use intent::{
    keyboard_intent, mouse_intent, FocusDirection, GestureKind, MouseTarget, OverlayTarget,
    UiIntent,
};
pub use render_purity::{RenderEvent, RenderPurityError, RenderPurityProbe, RenderSideEffect};
pub use state::{GestureState, InteractionState, OverlayStackState, ScreenMode};
pub use table_data::{transition_cases, TransitionCase, TransitionOutcome};
pub use transitions::{TransitionError, TransitionReason, TransitionTable};
