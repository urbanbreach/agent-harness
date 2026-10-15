//! Eligibility and savings the planner would produce, without changing a request.
use super::analysis::Analysis;
use super::planner::saved_tokens;

/// Why the system prompt is not imaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum SystemReason {
    /// Nothing to image: absent or empty prompt, no matching regions, or setting off.
    Empty,
    /// Frames would not save at least ten percent of the text tokens.
    Margin,
    /// The frames exceed the six-frame cap or the image budget, or the total image cap
    /// would retract them.
    Budget,
    /// No user message could carry the frames.
    NoCarrier,
    /// The reader cannot see images.
    VisionInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum SystemOutcome {
    Swap { frames: usize, saved_tokens: u64 },
    Skipped(SystemReason),
}

/// A tool result that the request would send as frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct ToolSaving {
    pub tool_call_id: String,
    pub frames: usize,
    pub saved_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct InlineEstimate {
    pub system: SystemOutcome,
    pub tools: Vec<ToolSaving>,
}

impl InlineEstimate {
    pub(in crate::coord) fn vision_inactive() -> Self {
        Self {
            system: SystemOutcome::Skipped(SystemReason::VisionInactive),
            tools: Vec::new(),
        }
    }

    pub(in crate::coord) fn total_saved_tokens(&self) -> u64 {
        let system = match self.system {
            SystemOutcome::Swap { saved_tokens, .. } => saved_tokens,
            SystemOutcome::Skipped(_) => 0,
        };
        self.tools.iter().fold(system, |total, tool| {
            total.saturating_add(tool.saved_tokens)
        })
    }
}

/// What the plan after the total cap backstop applies.
pub(super) fn estimate(analysis: &Analysis, raw_price: u32) -> InlineEstimate {
    let system = match (&analysis.system, analysis.selection.system) {
        (Some(swap), true) => SystemOutcome::Swap {
            frames: swap.frames.len(),
            saved_tokens: saved_tokens(swap.tokens, swap.frames.len(), raw_price),
        },
        (Some(_), false) => SystemOutcome::Skipped(SystemReason::Budget),
        (None, _) => SystemOutcome::Skipped(analysis.reason.unwrap_or(SystemReason::Empty)),
    };
    InlineEstimate {
        system,
        tools: tools(analysis, raw_price),
    }
}

/// Applied tool swaps with their savings.
pub(super) fn tools(analysis: &Analysis, raw_price: u32) -> Vec<ToolSaving> {
    analysis
        .tools
        .built
        .iter()
        .filter(|tool| analysis.selection.tools.contains(&tool.index))
        .map(|tool| ToolSaving {
            tool_call_id: tool.id.clone(),
            frames: tool.frames.len(),
            saved_tokens: saved_tokens(tool.tokens, tool.frames.len(), raw_price),
        })
        .collect()
}
