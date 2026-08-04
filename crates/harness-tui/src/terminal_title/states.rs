use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleActivity {
    Idle,
    Streaming,
    ToolRunning,
    AwaitingPermission,
    AwaitingQuestion,
    Recovering,
    Failed,
    Completed,
}

impl TitleActivity {
    const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Streaming => "streaming",
            Self::ToolRunning => "tool_running",
            Self::AwaitingPermission => "awaiting_permission",
            Self::AwaitingQuestion => "awaiting_question",
            Self::Recovering => "recovering",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }

    const fn needs_attention(self) -> bool {
        matches!(self, Self::AwaitingPermission | Self::AwaitingQuestion)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlePhase {
    Steady,
    ActionRequired(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleState {
    pub activity: TitleActivity,
    pub phase: TitlePhase,
    pub last_emitted: Option<String>,
}

impl TitleState {
    pub fn new() -> Self {
        Self {
            activity: TitleActivity::Idle,
            phase: TitlePhase::Steady,
            last_emitted: None,
        }
    }

    pub fn set_activity(&mut self, activity: TitleActivity) {
        if self.activity == activity {
            return;
        }
        self.activity = activity;
        self.phase = if activity.needs_attention() {
            TitlePhase::ActionRequired(0)
        } else {
            TitlePhase::Steady
        };
    }

    pub fn tick(&mut self) {
        if let TitlePhase::ActionRequired(counter) = self.phase {
            self.phase = TitlePhase::ActionRequired((counter + 7) % 8);
        }
    }

    pub fn current_title(&self, session_name: &str) -> String {
        let attention =
            self.activity.needs_attention() && matches!(self.phase, TitlePhase::ActionRequired(_));
        let suffix = if attention { " ⚠" } else { "" };
        format!(
            "harness — {session_name} — {}{suffix}",
            self.activity.label()
        )
    }

    pub fn should_emit(&self, candidate: &str) -> bool {
        self.last_emitted.as_deref() != Some(candidate)
    }
}

impl Default for TitleState {
    fn default() -> Self {
        Self::new()
    }
}
