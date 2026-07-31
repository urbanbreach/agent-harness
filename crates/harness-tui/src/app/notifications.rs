//! Local notification state for task-completion-while-unfocused, permission
//! focus alerts, notification-storm bounding, and focus-race safety.
//!
//! No network calls. No telemetry, no analytics, no hosted content fetching.

use crate::leaf_actions::group_f_notices::NoticeLevel;

/// Maximum concurrent notifications (storm bounding).
pub const MAX_CONCURRENT_NOTIFICATIONS: usize = 3;

/// Kind of notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    /// A background task completed while the TUI was unfocused.
    TaskCompleted,
    /// A permission request arrived while the TUI was unfocused.
    PermissionAlert,
    /// Generic informational notification.
    Info,
}

/// A single notification entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationEntry {
    pub kind: NotificationKind,
    pub level: NoticeLevel,
    pub message: String,
    pub seq: u64,
}

/// Notification state — tracks active notifications with storm bounding
/// and focus-aware delivery.
#[derive(Debug, Clone, Default)]
pub struct NotificationState {
    entries: Vec<NotificationEntry>,
    next_seq: u64,
    focused: bool,
}

impl NotificationState {
    /// Create a new notification state (unfocused, empty).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether the TUI is focused. Notifications are only delivered
    /// when unfocused.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Returns true if the TUI is currently focused.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Returns true if notifications should be delivered (i.e., unfocused).
    pub fn should_deliver(&self) -> bool {
        !self.focused
    }

    /// Push a notification. Returns the assigned sequence number.
    /// Storm bounding: oldest notification is dropped if over max.
    pub fn push(&mut self, kind: NotificationKind, level: NoticeLevel, message: &str) -> u64 {
        self.next_seq += 1;
        let seq = self.next_seq;
        self.entries.push(NotificationEntry {
            kind,
            level,
            message: message.to_string(),
            seq,
        });
        while self.entries.len() > MAX_CONCURRENT_NOTIFICATIONS {
            self.entries.remove(0);
        }
        seq
    }

    /// Returns the current notification entries.
    pub fn entries(&self) -> &[NotificationEntry] {
        &self.entries
    }

    /// Dismiss a notification by its sequence number.
    pub fn dismiss(&mut self, seq: u64) {
        self.entries.retain(|e| e.seq != seq);
    }

    /// Clear all notifications.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
