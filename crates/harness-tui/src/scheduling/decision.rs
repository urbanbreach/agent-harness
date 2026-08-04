#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameReason {
    Animation,
    Flush,
    AnimationAndFlush,
    AnimationPending,
    FlushPending,
    AnimationAndFlushPending,
    ReducedMotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDecision {
    pub render: bool,
    pub deadline_ms: Option<u64>,
    pub reason: FrameReason,
}

impl FrameDecision {
    pub(crate) const fn pending(deadline_ms: u64, reason: FrameReason) -> Self {
        Self {
            render: false,
            deadline_ms: Some(deadline_ms),
            reason,
        }
    }

    pub(crate) const fn render(deadline_ms: Option<u64>, reason: FrameReason) -> Self {
        Self {
            render: true,
            deadline_ms,
            reason,
        }
    }
}
