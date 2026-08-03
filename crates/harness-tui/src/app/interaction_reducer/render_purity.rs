use super::intent::UiIntent;
use crate::keybindings::Action;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderEvent {
    Emitted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderSideEffect {
    Intent(UiIntent),
    Action(Action),
    Event(RenderEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPurityError {
    side_effects: Vec<RenderSideEffect>,
}

impl RenderPurityError {
    pub fn side_effects(&self) -> &[RenderSideEffect] {
        &self.side_effects
    }
}

impl fmt::Display for RenderPurityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "render emitted {} side effect(s)",
            self.side_effects.len()
        )
    }
}

impl std::error::Error for RenderPurityError {}

#[derive(Debug, Default)]
pub struct RenderPurityProbe {
    side_effects: Vec<RenderSideEffect>,
}

impl RenderPurityProbe {
    pub fn draw<R>(&mut self, render: impl FnOnce(&mut Self) -> R) -> Result<R, RenderPurityError> {
        self.side_effects.clear();
        let result = render(self);
        if self.side_effects.is_empty() {
            Ok(result)
        } else {
            Err(RenderPurityError {
                side_effects: self.side_effects.clone(),
            })
        }
    }

    pub fn record_intent(&mut self, intent: UiIntent) {
        self.side_effects.push(RenderSideEffect::Intent(intent));
    }

    pub fn record_action(&mut self, action: Action) {
        self.side_effects.push(RenderSideEffect::Action(action));
    }

    pub fn record_event(&mut self, event: RenderEvent) {
        self.side_effects.push(RenderSideEffect::Event(event));
    }

    pub fn side_effects(&self) -> &[RenderSideEffect] {
        &self.side_effects
    }
}
