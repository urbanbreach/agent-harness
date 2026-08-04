use super::triggers::TipId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipPriority {
    pub rank: u8,
    pub display_seconds: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipTrigger {
    pub id: TipId,
    pub priority: TipPriority,
}

impl TipTrigger {
    pub fn priority_for(id: TipId) -> TipPriority {
        priority_for(id)
    }

    pub fn resolve_active(triggered: &[TipId]) -> Option<Self> {
        triggered
            .iter()
            .copied()
            .map(Self::from)
            .min_by_key(|trigger| (trigger.priority.rank, tip_order(trigger.id)))
    }
}

impl From<TipId> for TipTrigger {
    fn from(id: TipId) -> Self {
        Self {
            id,
            priority: priority_for(id),
        }
    }
}

pub fn resolve_active(triggered: &[TipId]) -> Option<TipTrigger> {
    TipTrigger::resolve_active(triggered)
}

pub fn priority_for(id: TipId) -> TipPriority {
    match id {
        TipId::PermissionPrompted => TipPriority {
            rank: 0,
            display_seconds: 0,
        },
        TipId::NoModelSelected => TipPriority {
            rank: 1,
            display_seconds: 0,
        },
        TipId::FirstRun => TipPriority {
            rank: 2,
            display_seconds: 10,
        },
        TipId::CompactViewport => TipPriority {
            rank: 3,
            display_seconds: 6,
        },
        TipId::ReducedMotion => TipPriority {
            rank: 4,
            display_seconds: 8,
        },
        TipId::QueueHasItems => TipPriority {
            rank: 5,
            display_seconds: 6,
        },
        TipId::ComposerEmpty => TipPriority {
            rank: 6,
            display_seconds: 5,
        },
        TipId::StreamingStarted => TipPriority {
            rank: 7,
            display_seconds: 4,
        },
        TipId::ToolRunning => TipPriority {
            rank: 8,
            display_seconds: 4,
        },
        TipId::LargeTranscript => TipPriority {
            rank: 9,
            display_seconds: 6,
        },
    }
}

fn tip_order(id: TipId) -> u8 {
    match id {
        TipId::FirstRun => 0,
        TipId::ComposerEmpty => 1,
        TipId::StreamingStarted => 2,
        TipId::PermissionPrompted => 3,
        TipId::ToolRunning => 4,
        TipId::LargeTranscript => 5,
        TipId::ReducedMotion => 6,
        TipId::CompactViewport => 7,
        TipId::NoModelSelected => 8,
        TipId::QueueHasItems => 9,
    }
}
