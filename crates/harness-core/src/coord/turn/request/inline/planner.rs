//! Prefix-local selection of swaps, the total image cap backstop and prefix replay.
use super::cache::SYSTEM_FRAME_CAP;
use std::collections::BTreeSet;

/// Text tokens a tool result needs before it is imaged.
pub(super) const FLOOR_TOKENS: u32 = 3_000;

/// Whether `frames` frames at the raw square price cost at most 0.9 of the text tokens.
pub(super) fn saves(tokens: u32, frames: usize, raw_price: u32) -> bool {
    let cost = u64::try_from(frames)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::from(raw_price));
    frames > 0 && cost.saturating_mul(10) <= u64::from(tokens).saturating_mul(9)
}

/// Text tokens an applied swap avoids.
pub(super) fn saved_tokens(tokens: u32, frames: usize, raw_price: u32) -> u64 {
    let cost = u64::try_from(frames)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::from(raw_price));
    u64::from(tokens).saturating_sub(cost)
}

/// The system prompt target, already past the margin gate.
#[derive(Debug, Clone, Copy)]
pub(super) struct SystemCandidate {
    /// Message index of the first user message, the frame carrier.
    pub carrier: usize,
    pub frames: usize,
}

/// A tool result candidate, already past the floor, frame and margin gates.
#[derive(Debug, Clone, Copy)]
pub(super) struct ToolCandidate {
    pub index: usize,
    pub frames: usize,
}

/// What the request sends as frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Selection {
    pub system: bool,
    pub tools: BTreeSet<usize>,
}

/// A swap that was sent as frames and later had to be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Item {
    System,
    Tool(usize),
}

/// The message whose arrival forced a swap back to text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Retraction {
    pub item: Item,
    pub at: usize,
}

pub(super) struct Plan<'a> {
    /// Total image allowance of the deployment.
    pub limit: u32,
    /// Source images through message `i`, inclusive.
    pub images_through: &'a [u32],
    pub system: Option<SystemCandidate>,
    /// Ascending by message index.
    pub tools: &'a [ToolCandidate],
}

impl Plan<'_> {
    /// Frames the system prompt may take: at most six, within what the images through its
    /// carrier leave.
    pub(super) fn system_room(&self, carrier: usize) -> u32 {
        let used = self.images_through.get(carrier).copied().unwrap_or(0);
        self.limit.saturating_sub(used).min(SYSTEM_FRAME_CAP)
    }

    fn tool_frames(&self, index: usize) -> usize {
        self.tools
            .iter()
            .find(|tool| tool.index == index)
            .map_or(0, |tool| tool.frames)
    }

    /// The prefix-local plan: each decision sees only the content up to its message.
    /// The system prompt claims budget first; an oversize candidate is skipped and
    /// selection continues.
    pub(super) fn natural(&self) -> Selection {
        let mut selection = Selection::default();
        let mut chosen = 0usize;
        let system = self.system.filter(|system| {
            (1..=usize::try_from(self.system_room(system.carrier)).unwrap_or(0))
                .contains(&system.frames)
        });
        selection.system = system.is_some();
        for tool in self.tools {
            let system_frames = system
                .filter(|system| system.carrier <= tool.index)
                .map_or(0, |system| system.frames);
            let used = usize::try_from(self.images_through.get(tool.index).copied().unwrap_or(0))
                .unwrap_or(usize::MAX);
            let limit = usize::try_from(self.limit).unwrap_or(usize::MAX);
            if used
                .saturating_add(system_frames)
                .saturating_add(chosen)
                .saturating_add(tool.frames)
                <= limit
            {
                chosen += tool.frames;
                selection.tools.insert(tool.index);
            }
        }
        selection
    }

    /// The natural plan as one request ending at message `end` sends it: when source
    /// images and chosen frames exceed the cap, the newest tool swaps go back to text
    /// first, then the system swap. Real source images are never removed.
    pub(super) fn prefix(&self, natural: &Selection, end: usize) -> Selection {
        let mut selection = Selection {
            system: natural.system && self.system.is_some_and(|system| system.carrier <= end),
            tools: natural.tools.range(..=end).copied().collect(),
        };
        let mut total = usize::try_from(self.images_through.get(end).copied().unwrap_or(0))
            .unwrap_or(usize::MAX);
        if selection.system {
            total = total.saturating_add(self.system.map_or(0, |system| system.frames));
        }
        for index in &selection.tools {
            total = total.saturating_add(self.tool_frames(*index));
        }
        let limit = usize::try_from(self.limit).unwrap_or(usize::MAX);
        while total > limit {
            if let Some(index) = selection.tools.pop_last() {
                total = total.saturating_sub(self.tool_frames(index));
            } else if selection.system {
                selection.system = false;
                total = total.saturating_sub(self.system.map_or(0, |system| system.frames));
            } else {
                break;
            }
        }
        selection
    }

    /// Replays the backstop on every prefix. Returns the full request's selection and,
    /// for each swap that was sent and later retracted, the first message that forced
    /// its retraction.
    pub(super) fn replay(&self, natural: &Selection) -> (Selection, Vec<Retraction>) {
        let mut retractions = Vec::new();
        let mut previous = Selection::default();
        let count = self.images_through.len();
        for end in 0..count {
            let grew = end > 0 && self.images_through[end] != self.images_through[end - 1];
            let arrives = natural.tools.contains(&end)
                || self.system.is_some_and(|system| system.carrier == end);
            if !(grew || arrives) {
                continue;
            }
            let current = self.prefix(natural, end);
            if previous.system && !current.system {
                retractions.push(Retraction {
                    item: Item::System,
                    at: end,
                });
            }
            for index in previous.tools.difference(&current.tools) {
                retractions.push(Retraction {
                    item: Item::Tool(*index),
                    at: end,
                });
            }
            previous = current;
        }
        let last = count
            .checked_sub(1)
            .map_or_else(Selection::default, |end| self.prefix(natural, end));
        (last, retractions)
    }
}
