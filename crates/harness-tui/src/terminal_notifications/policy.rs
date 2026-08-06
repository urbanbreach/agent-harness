#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusState {
    Focused,
    Unfocused,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    ActionRequired,
    Complete,
    Failed,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationEvent {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub created_at_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionState {
    Active,
    Suppressed { until_tick: u64 },
}

pub struct NotificationPolicy {
    focus: FocusState,
    suppression: SuppressionState,
    last_emitted: Option<(NotificationKind, String)>,
    last_emitted_tick: u64,
    debounce_ticks: u64,
    rate_limit_per_100_ticks: u16,
    emitted_this_window: u16,
    window_start_tick: u64,
}

impl NotificationPolicy {
    pub fn new(debounce_ticks: u64, rate_limit: u16) -> Self {
        Self {
            focus: FocusState::Unknown,
            suppression: SuppressionState::Active,
            last_emitted: None,
            last_emitted_tick: 0,
            debounce_ticks,
            rate_limit_per_100_ticks: rate_limit,
            emitted_this_window: 0,
            window_start_tick: 0,
        }
    }

    pub fn set_focus(&mut self, focus: FocusState) {
        self.focus = focus;
    }

    pub fn focus(&self) -> FocusState {
        self.focus
    }

    /// ActionRequired events are never suppressed by focus.
    pub fn should_notify(&mut self, event: &NotificationEvent) -> bool {
        if self.focus == FocusState::Focused && event.kind != NotificationKind::ActionRequired {
            return false;
        }
        if let SuppressionState::Suppressed { until_tick } = self.suppression {
            if event.created_at_tick < until_tick {
                return false;
            }
        }
        if let Some((kind, title)) = &self.last_emitted {
            if *kind == event.kind
                && *title == format!("{}\0{}", event.title, event.body)
                && event.created_at_tick.saturating_sub(self.last_emitted_tick)
                    < self.debounce_ticks
            {
                return false;
            }
        }
        if event.created_at_tick.saturating_sub(self.window_start_tick) >= 100 {
            self.window_start_tick = event.created_at_tick;
            self.emitted_this_window = 0;
        }
        if self.emitted_this_window >= self.rate_limit_per_100_ticks {
            return false;
        }
        self.last_emitted = Some((event.kind, format!("{}\0{}", event.title, event.body)));
        self.last_emitted_tick = event.created_at_tick;
        self.emitted_this_window = self.emitted_this_window.saturating_add(1);
        true
    }

    pub fn suppress_for(&mut self, ticks: u64, current_tick: u64) {
        self.suppression = SuppressionState::Suppressed {
            until_tick: current_tick.saturating_add(ticks),
        };
    }

    pub fn reset(&mut self) {
        self.suppression = SuppressionState::Active;
        self.last_emitted = None;
        self.last_emitted_tick = 0;
        self.emitted_this_window = 0;
        self.window_start_tick = 0;
    }
}

impl Default for NotificationPolicy {
    fn default() -> Self {
        Self::new(5, 10)
    }
}
