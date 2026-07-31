//! Startup surface leaf view.
//!
//! Models the P0-START rows (welcome panel, breadcrumb/warning,
//! welcome-to-composer transition) as a plain value type with no
//! app-state or registry dependency. A test or render shard constructs
//! this from real `AppState` fields via [`StartupLeafView::from_app`].

use crate::app::{Focus, LifecycleShellState};

/// Which startup sub-state the shell is in.
///
/// Maps to the manifest rows:
/// - `WelcomePanel`       → P0-START-01
/// - `BreadcrumbWarning`  → P0-START-02
/// - `DraftActive`        → P0-START-03 (welcome cleared, composer has draft)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartupPhase {
    #[default]
    None,
    WelcomePanel,
    BreadcrumbWarning,
    DraftActive,
}

/// Deterministic view state for the startup surface.
///
/// No app-state or registry dependency — a plain `Copy` value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StartupLeafView {
    pub phase: StartupPhase,
    pub welcome_visible: bool,
    pub breadcrumb_visible: bool,
    pub composer_focusable: bool,
}

impl StartupLeafView {
    pub const fn new(phase: StartupPhase, welcome: bool, breadcrumb: bool, composer: bool) -> Self {
        Self {
            phase,
            welcome_visible: welcome,
            breadcrumb_visible: breadcrumb,
            composer_focusable: composer,
        }
    }

    /// Derive the startup leaf view from real app state fields.
    ///
    /// This is the only bridge to `AppState`; the leaf itself stores no
    /// reference and can be freely copied.
    pub fn from_app(
        lifecycle_shell: LifecycleShellState,
        startup_mode: bool,
        _focus: Focus,
        prompt_has_draft: bool,
    ) -> Self {
        let startup_shell = startup_mode && matches!(lifecycle_shell, LifecycleShellState::Startup);
        if !startup_shell {
            return Self::default();
        }
        let composer_focusable = true;
        if prompt_has_draft {
            Self::new(StartupPhase::DraftActive, false, true, composer_focusable)
        } else {
            Self::new(StartupPhase::WelcomePanel, true, true, composer_focusable)
        }
    }

    /// The manifest's `expected_focus_owner` for every P0-START row is
    /// `"composer"`. At startup the composer is always the input target;
    /// typing transitions internal `Focus` to `Prompt`.
    pub const fn focus_owner(self) -> &'static str {
        "composer"
    }

    /// P0-START-03: typing clears the welcome panel.
    pub const fn welcome_cleared_by_draft(self) -> bool {
        matches!(self.phase, StartupPhase::DraftActive)
    }
}
