use std::collections::HashSet;

use super::triggers::{evaluate_triggers, TipId};
use super::{priority::TipTrigger, triggers::TipContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipDismissal {
    pub tip: TipId,
    pub dismissed_at_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipLifetime {
    Transient { ticks_remaining: u16 },
    Persistent,
    Dismissed { at_tick: u64 },
}

pub struct TipManager {
    active: Option<TipId>,
    lifetime: TipLifetime,
    dismissed: HashSet<TipId>,
    current_tick: u64,
}

impl TipManager {
    pub fn new() -> Self {
        Self {
            active: None,
            lifetime: TipLifetime::Transient { ticks_remaining: 0 },
            dismissed: HashSet::new(),
            current_tick: 0,
        }
    }

    pub fn tick(&mut self) {
        self.current_tick += 1;
        if let TipLifetime::Transient { ticks_remaining } = &mut self.lifetime {
            *ticks_remaining = ticks_remaining.saturating_sub(1);
            if *ticks_remaining == 0 {
                self.active = None;
            }
        }
    }

    pub fn update(&mut self, ctx: &TipContext) -> Option<TipId> {
        let triggered: Vec<_> = evaluate_triggers(ctx)
            .into_iter()
            .filter(|tip| !self.dismissed.contains(tip))
            .collect();
        let resolved = TipTrigger::resolve_active(&triggered);
        match resolved {
            Some(trigger) if self.active != Some(trigger.id) => {
                self.active = Some(trigger.id);
                self.lifetime = if trigger.priority.display_seconds == 0 {
                    TipLifetime::Persistent
                } else {
                    TipLifetime::Transient {
                        ticks_remaining: trigger.priority.display_seconds,
                    }
                };
            }
            Some(_) => {}
            None => self.active = None,
        }
        self.active
    }

    /// Dismissed tips cannot recur until `clear_dismissals` is called.
    pub fn dismiss(&mut self, tip: TipId) {
        self.dismissed.insert(tip);
        if self.active == Some(tip) {
            self.active = None;
            self.lifetime = TipLifetime::Dismissed {
                at_tick: self.current_tick,
            };
        }
    }

    pub fn active(&self) -> Option<TipId> {
        self.active
    }

    pub fn is_dismissed(&self, tip: TipId) -> bool {
        self.dismissed.contains(&tip)
    }

    pub fn clear_dismissals(&mut self) {
        self.dismissed.clear();
    }
}

impl Default for TipManager {
    fn default() -> Self {
        Self::new()
    }
}
