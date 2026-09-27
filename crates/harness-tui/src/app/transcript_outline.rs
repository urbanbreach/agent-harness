use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::{ActivityEntry, ActivityStatus, ToolCallDisplayStatus};
use crate::shell_geometry::{layout_for_rect, ShellState};
use crate::theme_tokens::LifecycleState;
use crate::transcript_blocks::{default_fold, BlockKind, BlockLifecycle, FoldState};
use crate::transcript_identity::{ReplayTurn, TurnId};
use crate::transcript_scroll::{FollowState, TranscriptLayout};
use crate::transcript_timeline::{ResponsePosition, TimelineJump, TimelineMarker, TimelineStatus};

#[derive(PartialEq, Eq)]
struct OutlineTurn {
    replay: ReplayTurn,
    marker: TimelineMarker,
    row: usize,
    heights: Vec<u32>,
}

impl OutlineTurn {
    fn bottom(&self) -> usize {
        self.heights.iter().fold(self.row, |row, height| {
            row.saturating_add(usize::try_from(*height).unwrap_or(usize::MAX))
        })
    }
}

/// The margin timeline stores geometry only. Content stays in the activity projection.
pub(crate) struct TranscriptOutline {
    viewport: Rect,
    navigation_height: usize,
    turns: Vec<OutlineTurn>,
    selected: Option<TurnId>,
    navigation_top: usize,
    pub(super) scroll_top: usize,
    follow: FollowState,
}

impl TranscriptOutline {
    pub(super) fn new(viewport: Rect) -> Self {
        let viewport = layout_for_rect(viewport, ShellState::Streaming).transcript_viewport;
        Self {
            viewport,
            navigation_height: usize::from(viewport.height),
            turns: Vec::new(),
            selected: None,
            navigation_top: 0,
            scroll_top: 0,
            follow: FollowState::new(),
        }
    }

    pub(super) fn update(
        &mut self,
        activities: &std::collections::VecDeque<ActivityEntry>,
        viewport: Rect,
        from: usize,
    ) {
        // Output can arrive while the terminal temporarily reports no drawable area.
        let viewport = if viewport.width == 0 || viewport.height == 0 {
            self.viewport
        } else {
            layout_for_rect(viewport, ShellState::Streaming).transcript_viewport
        };
        let from = if viewport.width != self.viewport.width {
            0
        } else {
            from.min(self.turns.len()).min(activities.len())
        };
        if viewport == self.viewport && from == activities.len() && from == self.turns.len() {
            return;
        }
        let anchor = (!self.follow.is_following())
            .then(|| self.anchor())
            .flatten();
        let selected_offset = self
            .selected_index()
            .map(|index| self.turns[index].row.saturating_sub(self.navigation_top));
        let mut changed = viewport != self.viewport || activities.len() != self.turns.len();
        self.viewport = viewport;
        let mut row = from
            .checked_sub(1)
            .map_or(0, |index| self.turns[index].bottom());
        for (index, activity) in activities.iter().enumerate().skip(from) {
            let heights = activity_blocks(activity)
                .map(|block| block.rows(viewport.width))
                .collect::<Vec<_>>();
            let replay = replay_turn(index, activity, heights.len());
            let turn = OutlineTurn {
                replay,
                marker: TimelineMarker::from_replay(
                    replay,
                    timeline_status(activity.status),
                    lifecycle_state(activity),
                ),
                row,
                heights,
            };
            row = turn.bottom();
            if let Some(previous) = self.turns.get_mut(index) {
                if *previous != turn {
                    *previous = turn;
                    changed = true;
                }
            } else {
                self.turns.push(turn);
            }
        }
        self.turns.truncate(activities.len());
        if !changed {
            return;
        }
        if let Some(index) = self.selected_index() {
            self.navigation_top = self.turns[index]
                .row
                .saturating_sub(selected_offset.unwrap_or(0));
        } else {
            self.selected = None;
            self.navigation_top = 0;
        }
        let max = self.max_scroll();
        let _ = self.follow.content_changed(rows_as_f64(max));
        self.scroll_top = if self.follow.is_following() {
            max
        } else {
            anchor
                .and_then(|anchor| self.resolve_anchor(anchor))
                .unwrap_or(self.scroll_top)
                .min(max)
        };
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        self.index_of(self.selected?)
    }

    pub(super) fn index_of(&self, id: TurnId) -> Option<usize> {
        self.turns
            .iter()
            .position(|turn| turn.replay.turn_id() == id)
    }

    pub(super) fn select(&mut self, index: usize) -> bool {
        let Some(turn) = self.turns.get(index) else {
            return false;
        };
        self.selected = Some(turn.replay.turn_id());
        let last_visible = self
            .navigation_top
            .saturating_add(self.navigation_height.saturating_sub(1));
        if turn.row < self.navigation_top {
            self.navigation_top = turn.row;
        } else if turn.row > last_visible {
            self.navigation_top = turn
                .row
                .saturating_sub(self.navigation_height.saturating_sub(1));
        }
        self.scroll_top = self.navigation_top;
        let _ = self.follow.scroll_by(
            rows_as_f64(self.scroll_top) - self.follow.offset(),
            rows_as_f64(self.max_scroll()),
        );
        true
    }

    pub(super) fn jump(&mut self, jump: TimelineJump) -> usize {
        if self.turns.is_empty() {
            return self.navigation_top;
        }
        let current = self.selected_index();
        let target = match jump {
            TimelineJump::NextTurn => {
                Some(current.map_or(0, |index| (index + 1).min(self.turns.len() - 1)))
            }
            TimelineJump::PreviousTurn => Some(current.map_or(0, |index| index.saturating_sub(1))),
            TimelineJump::JumpToFailed | TimelineJump::JumpToStreaming => {
                let status = if jump == TimelineJump::JumpToFailed {
                    TimelineStatus::Failed
                } else {
                    TimelineStatus::Streaming
                };
                let start = current.map_or(0, |index| index + 1);
                (start..self.turns.len())
                    .chain(0..start)
                    .find(|index| self.turns[*index].marker.status == status)
            }
            TimelineJump::NextResponse | TimelineJump::PreviousResponse => None,
        };
        if let Some(index) = target {
            self.select(index);
        }
        self.navigation_top
    }

    pub(crate) fn response_position(&self) -> Option<ResponsePosition> {
        let selected = self.selected?;
        let mut total = 0;
        let mut index = None;
        for turn in &self.turns {
            if turn.marker.status == TimelineStatus::Completed {
                total += 1;
                if turn.replay.turn_id() == selected {
                    index = Some(total);
                }
            }
        }
        Some(ResponsePosition {
            index: index?,
            total,
        })
    }

    pub(crate) fn markers(&self) -> impl Iterator<Item = (Rect, TimelineMarker)> + '_ {
        let first = self
            .turns
            .partition_point(|turn| turn.row < self.scroll_top);
        self.turns[first..]
            .iter()
            .take_while(|turn| {
                turn.row
                    < self
                        .scroll_top
                        .saturating_add(usize::from(self.viewport.height))
            })
            .filter_map(|turn| {
                let width = u16::try_from(turn.marker.glyph().width()).ok()?;
                (width > 0 && width < self.viewport.width).then(|| {
                    (
                        Rect::new(
                            self.viewport.x.saturating_add(1),
                            self.viewport.y.saturating_add(
                                u16::try_from(turn.row - self.scroll_top).unwrap_or(u16::MAX),
                            ),
                            width,
                            1,
                        ),
                        turn.marker,
                    )
                })
            })
    }

    /// Materialize the old block geometry only for a viewer return point or dashboard peek.
    pub(super) fn layout(&self) -> Option<TranscriptLayout> {
        TranscriptLayout::from_heights(
            self.turns.iter().flat_map(|turn| {
                turn.heights.iter().enumerate().map(|(index, height)| {
                    (
                        turn.replay
                            .block_id(u64::try_from(index).unwrap_or(u64::MAX)),
                        f64::from(*height),
                    )
                })
            }),
            f64::from(self.viewport.height.max(1)),
        )
        .ok()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "resolved anchors are non-negative, bounded row counts"
    )]
    pub(super) fn restore_viewer_anchor(
        &mut self,
        anchor: crate::transcript_scroll::LogicalAnchor,
    ) {
        if let Some(row) = self
            .layout()
            .and_then(|layout| anchor.resolve(&layout).ok())
        {
            self.scroll_top = row as usize;
        }
    }

    fn max_scroll(&self) -> usize {
        self.turns
            .last()
            .map_or(0, OutlineTurn::bottom)
            .saturating_sub(usize::from(self.viewport.height.max(1)))
    }

    fn anchor(&self) -> Option<(TurnId, usize, usize)> {
        let point = self.scroll_top.min(self.max_scroll());
        for turn in &self.turns {
            let mut row = turn.row;
            for (index, height) in turn.heights.iter().enumerate() {
                let bottom = row.saturating_add(usize::try_from(*height).unwrap_or(usize::MAX));
                if point <= bottom {
                    return Some((turn.replay.turn_id(), index, point.saturating_sub(row)));
                }
                row = bottom;
            }
        }
        None
    }

    fn resolve_anchor(&self, (id, block, offset): (TurnId, usize, usize)) -> Option<usize> {
        let turn = &self.turns[self.index_of(id)?];
        let height = usize::try_from(*turn.heights.get(block)?).ok()?;
        Some(
            turn.heights[..block]
                .iter()
                .fold(turn.row, |row, height| {
                    row.saturating_add(usize::try_from(*height).unwrap_or(usize::MAX))
                })
                .saturating_add(offset.min(height)),
        )
    }
}

pub(super) fn rows_as_f64(rows: usize) -> f64 {
    f64::from(u32::try_from(rows).unwrap_or(u32::MAX))
}

pub(super) fn replay_turn(
    index: usize,
    activity: &ActivityEntry,
    block_count: usize,
) -> ReplayTurn {
    ReplayTurn::event(
        activity.first_seq.max(1),
        u64::try_from(index).unwrap_or(u64::MAX),
        u64::try_from(block_count).unwrap_or(u64::MAX),
    )
}

pub(super) struct ActivityBlock<'a> {
    pub kind: BlockKind,
    pub lifecycle: BlockLifecycle,
    pub content: &'a str,
    pub raw: Option<&'a serde_json::Value>,
}

impl ActivityBlock<'_> {
    pub(super) fn rows(&self, width: u16) -> u32 {
        let rows = if default_fold(self.kind, self.lifecycle) == FoldState::Collapsed {
            1
        } else {
            self.content.split('\n').fold(0usize, |rows, line| {
                rows.saturating_add(line.width().max(1).div_ceil(usize::from(width.max(1))))
            })
        };
        u32::try_from(rows).unwrap_or(u32::MAX)
    }
}

pub(super) fn activity_blocks(activity: &ActivityEntry) -> impl Iterator<Item = ActivityBlock<'_>> {
    let lifecycle = match activity.status {
        ActivityStatus::Queued => BlockLifecycle::Waiting,
        ActivityStatus::Streaming => BlockLifecycle::Streaming,
        ActivityStatus::Done => BlockLifecycle::Completed,
        ActivityStatus::Error => BlockLifecycle::Failed,
    };
    let text_block = move |kind, content| ActivityBlock {
        kind,
        lifecycle,
        content,
        raw: None,
    };
    let empty = activity.user_message.is_none()
        && activity.thinking_text.is_empty()
        && activity.transcript_text.is_empty()
        && activity.tool_calls.is_empty();
    activity
        .user_message
        .as_ref()
        .map(|user| text_block(BlockKind::User, user.text.as_str()))
        .into_iter()
        .chain(
            (!activity.thinking_text.is_empty())
                .then(|| text_block(BlockKind::Thinking, activity.thinking_text.as_str())),
        )
        .chain(
            (!activity.transcript_text.is_empty())
                .then(|| text_block(BlockKind::Assistant, activity.transcript_text.as_str())),
        )
        .chain(activity.tool_calls.iter().map(|tool| ActivityBlock {
            kind: BlockKind::Tool,
            lifecycle: match tool.status {
                ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => {
                    BlockLifecycle::Waiting
                }
                ToolCallDisplayStatus::Running => BlockLifecycle::Tool,
                ToolCallDisplayStatus::Succeeded => BlockLifecycle::Completed,
                ToolCallDisplayStatus::Failed => BlockLifecycle::Failed,
            },
            content: tool.output_summary.as_deref().unwrap_or(&tool.args_summary),
            raw: tool.output_json.as_ref(),
        }))
        .chain(empty.then(|| {
            text_block(
                BlockKind::System,
                match activity.status {
                    ActivityStatus::Queued => "queued",
                    ActivityStatus::Streaming => "streaming…",
                    ActivityStatus::Done => "done",
                    ActivityStatus::Error => "error",
                },
            )
        }))
}

fn timeline_status(status: ActivityStatus) -> TimelineStatus {
    match status {
        ActivityStatus::Queued => TimelineStatus::Queued,
        ActivityStatus::Streaming => TimelineStatus::Streaming,
        ActivityStatus::Done => TimelineStatus::Completed,
        ActivityStatus::Error => TimelineStatus::Failed,
    }
}

fn lifecycle_state(activity: &ActivityEntry) -> LifecycleState {
    match activity.status {
        ActivityStatus::Queued => LifecycleState::Queued,
        ActivityStatus::Streaming if !activity.tool_calls.is_empty() => LifecycleState::Tool,
        ActivityStatus::Streaming if !activity.thinking_text.is_empty() => LifecycleState::Thinking,
        ActivityStatus::Streaming => LifecycleState::Streaming,
        ActivityStatus::Done => LifecycleState::Completed,
        ActivityStatus::Error => LifecycleState::Failed,
    }
}
