//! Context-aware action dispatch for clean-room parity.
//!
//! Maps key events to actions with context filtering based on the active
//! screen/pane state. Mirrors the reference's `When` context system.

use super::{Action, KeyBinding};
use crossterm::event::KeyEvent;

/// Context conditions under which an action definition is active.
///
/// Mirrors the reference's 7 `When` variants for input-bubbling layer matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionContext {
    /// Action is always available regardless of focus state.
    Always,
    /// Only when the prompt input has focus.
    PromptFocused,
    /// Only when the scrollback/transcript pane has focus.
    ScrollbackFocused,
    /// Available on the agent conversation screen.
    AgentScreen,
    /// Available on the welcome/landing screen.
    WelcomeScreen,
    /// Available when the agent dashboard has focus.
    DashboardFocused,
    /// Available when a dashboard overlay is open.
    DashboardOverlay,
}

impl ActionContext {
    /// Whether this context is satisfied given the current active context.
    ///
    /// `Always` is satisfied in every context. `AgentScreen` is also satisfied
    /// when a more specific screen context is active (it's a superset).
    pub fn is_satisfied_by(self, current: ActionContext) -> bool {
        match self {
            ActionContext::Always => true,
            ActionContext::AgentScreen => matches!(
                current,
                ActionContext::AgentScreen
                    | ActionContext::PromptFocused
                    | ActionContext::ScrollbackFocused
            ),
            other => other == current,
        }
    }

    /// All context variants for exhaustive iteration.
    pub const fn all() -> &'static [ActionContext] {
        &[
            ActionContext::Always,
            ActionContext::PromptFocused,
            ActionContext::ScrollbackFocused,
            ActionContext::AgentScreen,
            ActionContext::WelcomeScreen,
            ActionContext::DashboardFocused,
            ActionContext::DashboardOverlay,
        ]
    }

    /// Stable string identifier for config/metadata.
    pub const fn as_str(self) -> &'static str {
        match self {
            ActionContext::Always => "always",
            ActionContext::PromptFocused => "prompt_focused",
            ActionContext::ScrollbackFocused => "scrollback_focused",
            ActionContext::AgentScreen => "agent_screen",
            ActionContext::WelcomeScreen => "welcome_screen",
            ActionContext::DashboardFocused => "dashboard_focused",
            ActionContext::DashboardOverlay => "dashboard_overlay",
        }
    }
}

/// A single action definition binding a key to an action under a context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDef {
    /// The key binding that triggers this action.
    pub binding: KeyBinding,
    /// The action to dispatch.
    pub action: Action,
    /// The context under which this definition is active.
    pub context: ActionContext,
    /// Human-readable label for palette/help display.
    pub label: &'static str,
}

impl ActionDef {
    /// Create a new action definition.
    pub const fn new(
        binding: KeyBinding,
        action: Action,
        context: ActionContext,
        label: &'static str,
    ) -> Self {
        Self {
            binding,
            action,
            context,
            label,
        }
    }

    /// Whether this definition matches the given key event under the given context.
    pub fn matches(&self, event: &KeyEvent, current_context: ActionContext) -> bool {
        self.binding.matches(event) && self.context.is_satisfied_by(current_context)
    }
}

/// Dispatches key events to actions with context-aware filtering.
///
/// Maintains an ordered list of action definitions and resolves the first
/// matching definition for a given key event + active context.
#[derive(Debug, Clone)]
pub struct ActionDispatcher {
    defs: Vec<ActionDef>,
}

impl ActionDispatcher {
    /// Create an empty dispatcher.
    pub fn new() -> Self {
        Self { defs: Vec::new() }
    }

    /// Register an action definition.
    pub fn register(&mut self, def: ActionDef) {
        self.defs.push(def);
    }

    /// Register multiple action definitions.
    pub fn register_all(&mut self, defs: impl IntoIterator<Item = ActionDef>) {
        self.defs.extend(defs);
    }

    /// Resolve a key event to an action given the current context.
    ///
    /// Returns the action from the first matching definition, or `None`
    /// if no definition matches under the current context.
    pub fn resolve(&self, event: &KeyEvent, context: ActionContext) -> Option<Action> {
        self.defs
            .iter()
            .find(|def| def.matches(event, context))
            .map(|def| def.action)
    }

    /// Resolve with full definition metadata (for help/palette display).
    pub fn resolve_def(&self, event: &KeyEvent, context: ActionContext) -> Option<&ActionDef> {
        self.defs.iter().find(|def| def.matches(event, context))
    }

    /// All definitions active under the given context.
    pub fn active_defs(&self, context: ActionContext) -> Vec<&ActionDef> {
        self.defs
            .iter()
            .filter(|def| def.context.is_satisfied_by(context))
            .collect()
    }

    /// Number of registered definitions.
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Whether no definitions are registered.
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

impl Default for ActionDispatcher {
    fn default() -> Self {
        Self::new()
    }
}
