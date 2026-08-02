//! Focus management for the TUI panes.
//!
//! Provides a 6-pane focus model mirroring the reference's `ActivePane`
//! with tab-cycling, direct navigation, and focus indicators.

/// The six focusable panes in the agent view.
///
/// Mirrors the reference's `ActivePane` enum for pane focus transitions
/// triggered by Tab, action dispatch, or mouse click.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActivePane {
    /// The main transcript/scrollback pane.
    Scrollback,
    /// The todo/task-list side pane.
    Todo,
    /// The prompt queue pane.
    Queue,
    /// The prompt input/composer.
    #[default]
    Prompt,
    /// The background tasks pane.
    Tasks,
    /// The catalog/extensions pane.
    Catalog,
}

impl ActivePane {
    /// All panes in canonical Tab-cycle order.
    pub const CYCLE_ORDER: [ActivePane; 6] = [
        ActivePane::Scrollback,
        ActivePane::Todo,
        ActivePane::Queue,
        ActivePane::Prompt,
        ActivePane::Tasks,
        ActivePane::Catalog,
    ];

    /// Stable string identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            ActivePane::Scrollback => "scrollback",
            ActivePane::Todo => "todo",
            ActivePane::Queue => "queue",
            ActivePane::Prompt => "prompt",
            ActivePane::Tasks => "tasks",
            ActivePane::Catalog => "catalog",
        }
    }

    /// The next pane in Tab-cycle order.
    pub fn next(self) -> Self {
        let idx = self.cycle_index();
        Self::CYCLE_ORDER[(idx + 1) % Self::CYCLE_ORDER.len()]
    }

    /// The previous pane in reverse Tab-cycle order.
    pub fn prev(self) -> Self {
        let idx = self.cycle_index();
        Self::CYCLE_ORDER[(idx + Self::CYCLE_ORDER.len() - 1) % Self::CYCLE_ORDER.len()]
    }

    /// Index in the cycle order array.
    fn cycle_index(self) -> usize {
        // Keep variant order in sync with `CYCLE_ORDER`.
        match self {
            ActivePane::Scrollback => 0,
            ActivePane::Todo => 1,
            ActivePane::Queue => 2,
            ActivePane::Prompt => 3,
            ActivePane::Tasks => 4,
            ActivePane::Catalog => 5,
        }
    }
}

/// Controls focus transitions between panes.
///
/// Tracks the currently focused pane and provides navigation operations.
/// Focus changes are recorded as a transition log for deterministic testing.
#[derive(Debug, Clone)]
pub struct FocusController {
    current: ActivePane,
    history: Vec<ActivePane>,
}

impl FocusController {
    /// Create a focus controller with the given initial pane.
    pub fn new(initial: ActivePane) -> Self {
        Self {
            current: initial,
            history: vec![initial],
        }
    }

    /// The currently focused pane.
    pub fn current(&self) -> ActivePane {
        self.current
    }

    /// Whether the given pane is currently focused.
    pub fn is_focused(&self, pane: ActivePane) -> bool {
        self.current == pane
    }

    /// Move focus to the next pane in cycle order. Returns the new pane.
    pub fn focus_next(&mut self) -> ActivePane {
        let next = self.current.next();
        self.transition_to(next)
    }

    /// Move focus to the previous pane in cycle order. Returns the new pane.
    pub fn focus_prev(&mut self) -> ActivePane {
        let prev = self.current.prev();
        self.transition_to(prev)
    }

    /// Directly set focus to a specific pane. Returns the new pane.
    pub fn focus_pane(&mut self, pane: ActivePane) -> ActivePane {
        self.transition_to(pane)
    }

    /// Internal transition helper that records history.
    fn transition_to(&mut self, pane: ActivePane) -> ActivePane {
        if self.current != pane {
            self.current = pane;
            self.history.push(pane);
        }
        self.current
    }

    /// The full transition history (including initial state).
    pub fn history(&self) -> &[ActivePane] {
        &self.history
    }

    /// Number of transitions that occurred.
    pub fn transition_count(&self) -> usize {
        self.history.len().saturating_sub(1)
    }
}

impl Default for FocusController {
    fn default() -> Self {
        Self::new(ActivePane::default())
    }
}
