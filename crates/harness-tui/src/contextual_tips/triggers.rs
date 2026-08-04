use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipId {
    FirstRun,
    ComposerEmpty,
    StreamingStarted,
    PermissionPrompted,
    ToolRunning,
    LargeTranscript,
    ReducedMotion,
    CompactViewport,
    NoModelSelected,
    QueueHasItems,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TipContext {
    pub is_first_run: bool,
    pub composer_empty: bool,
    pub is_streaming: bool,
    pub permission_pending: bool,
    pub tool_running: bool,
    pub transcript_blocks: usize,
    pub reduced_motion: bool,
    pub viewport_compact: bool,
    pub model_selected: bool,
    pub queue_items: usize,
}

pub fn evaluate_triggers(ctx: &TipContext) -> Vec<TipId> {
    [
        (ctx.is_first_run, TipId::FirstRun),
        (
            ctx.composer_empty && !ctx.is_streaming,
            TipId::ComposerEmpty,
        ),
        (ctx.is_streaming, TipId::StreamingStarted),
        (ctx.permission_pending, TipId::PermissionPrompted),
        (ctx.tool_running, TipId::ToolRunning),
        (ctx.transcript_blocks > 50, TipId::LargeTranscript),
        (ctx.reduced_motion, TipId::ReducedMotion),
        (ctx.viewport_compact, TipId::CompactViewport),
        (!ctx.model_selected, TipId::NoModelSelected),
        (ctx.queue_items > 0, TipId::QueueHasItems),
    ]
    .into_iter()
    .filter_map(|(triggered, id)| triggered.then_some(id))
    .collect()
}
