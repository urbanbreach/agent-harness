//! Contextual tips state with seen counts and dismissal tracking.
//!
//! No network calls. No hosted content fetching.

use std::collections::{HashMap, HashSet};

/// A single tip entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipEntry {
    pub id: String,
    pub body: String,
}

/// Tip state — tracks the active tip, per-tip seen counts, and dismissed tips.
#[derive(Debug, Clone, Default)]
pub struct TipState {
    active: Option<TipEntry>,
    seen_counts: HashMap<String, usize>,
    dismissed: HashSet<String>,
}

impl TipState {
    /// Create a new empty tip state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show a tip, incrementing its seen count and setting it as active.
    pub fn show(&mut self, id: &str, body: &str) {
        *self.seen_counts.entry(id.to_string()).or_insert(0) += 1;
        self.active = Some(TipEntry {
            id: id.to_string(),
            body: body.to_string(),
        });
    }

    /// Returns the currently active tip, if any.
    pub fn active(&self) -> Option<&TipEntry> {
        self.active.as_ref()
    }

    /// Dismiss the active tip, marking it as dismissed.
    pub fn dismiss(&mut self) {
        if let Some(tip) = self.active.take() {
            self.dismissed.insert(tip.id);
        }
    }

    /// Returns the number of times a tip has been shown.
    pub fn seen_count(&self, id: &str) -> usize {
        self.seen_counts.get(id).copied().unwrap_or(0)
    }

    /// Returns true if a tip has been dismissed.
    pub fn is_dismissed(&self, id: &str) -> bool {
        self.dismissed.contains(id)
    }
}
