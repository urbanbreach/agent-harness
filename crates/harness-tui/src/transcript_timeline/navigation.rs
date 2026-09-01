// allow: SIZE_OK — timeline navigation keeps selection and exact-anchor transitions together
use crossterm::event::KeyEvent;

use crate::transcript_identity::{TranscriptIdentity, TurnId};

use super::hit_map::TimelineHitMap;
pub use super::key_navigation::key_jump;
use super::markers::{TimelineStatus, TimelineTurn};
use super::navigation_state::{
    ResponsePosition, ScrollAnchor, TimelineNavigationError, TimelineNavigationSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineJump {
    NextTurn,
    PreviousTurn,
    NextResponse,
    PreviousResponse,
    JumpToFailed,
    JumpToStreaming,
}

pub type KeyJump = TimelineJump;

#[derive(Debug, Clone)]
pub struct TimelineNavigation {
    identity: TranscriptIdentity,
    turns: Vec<TimelineTurn>,
    viewport_height: usize,
    scroll_top: usize,
    selected_turn_id: Option<TurnId>,
    anchor: Option<ScrollAnchor>,
}

impl TimelineNavigation {
    pub fn from_turns(
        turns: Vec<TimelineTurn>,
        viewport_height: u16,
    ) -> Result<Self, TimelineNavigationError> {
        let identity =
            TranscriptIdentity::from_replay(turns.iter().map(TimelineTurn::replay_turn))?;
        Self::new(identity, turns, viewport_height)
    }

    pub fn new(
        identity: TranscriptIdentity,
        turns: Vec<TimelineTurn>,
        viewport_height: u16,
    ) -> Result<Self, TimelineNavigationError> {
        validate_turns(&identity, &turns)?;
        Ok(Self {
            identity,
            turns,
            viewport_height: usize::from(viewport_height),
            scroll_top: 0,
            selected_turn_id: None,
            anchor: None,
        })
    }

    pub fn jump(
        &mut self,
        jump: TimelineJump,
    ) -> Result<TimelineNavigationSnapshot, TimelineNavigationError> {
        let Some(target_index) = self.target_index(jump)? else {
            return Ok(self.snapshot());
        };
        let target_id = self.turns[target_index].turn_id();
        self.select_turn(target_id)
    }

    pub fn handle_key(
        &mut self,
        key: KeyEvent,
    ) -> Result<Option<TimelineNavigationSnapshot>, TimelineNavigationError> {
        let Some(jump) = key_jump(key) else {
            return Ok(None);
        };
        self.jump(jump).map(Some)
    }

    pub fn select_at(
        &mut self,
        hit_map: &TimelineHitMap,
        x: u16,
        y: u16,
    ) -> Result<Option<TimelineNavigationSnapshot>, TimelineNavigationError> {
        let Some(turn_id) = hit_map.hit_test(x, y) else {
            return Ok(None);
        };
        self.select_turn(turn_id).map(Some)
    }

    pub fn select_turn(
        &mut self,
        turn_id: TurnId,
    ) -> Result<TimelineNavigationSnapshot, TimelineNavigationError> {
        let Some(turn) = self.turns.iter().find(|turn| turn.turn_id() == turn_id) else {
            return Err(TimelineNavigationError::SelectedTurnMissing(turn_id));
        };
        self.selected_turn_id = Some(turn_id);
        self.scroll_top = ensure_visible(self.scroll_top, turn.row, self.viewport_height);
        self.anchor = Some(ScrollAnchor::capture(turn, self.scroll_top));
        Ok(self.snapshot())
    }

    pub fn update_document(
        &mut self,
        identity: TranscriptIdentity,
        turns: Vec<TimelineTurn>,
    ) -> Result<TimelineNavigationSnapshot, TimelineNavigationError> {
        validate_turns(&identity, &turns)?;
        let previous_anchor = self.anchor;
        let previous_selection = self.selected_turn_id;
        self.identity = identity;
        self.turns = turns;
        self.selected_turn_id = previous_selection
            .filter(|turn_id| self.turns.iter().any(|turn| turn.turn_id() == *turn_id));
        let fallback_scroll = self.scroll_top;
        self.scroll_top = match (previous_anchor, self.selected_turn_id) {
            (Some(anchor), Some(_)) => anchor
                .resolve(&self.identity, &self.turns)
                .unwrap_or(fallback_scroll),
            _ => 0,
        };
        self.anchor = self.selected_turn_id.and_then(|turn_id| {
            self.turns
                .iter()
                .find(|turn| turn.turn_id() == turn_id)
                .map(|turn| ScrollAnchor::capture(turn, self.scroll_top))
        });
        Ok(self.snapshot())
    }

    pub fn update_last_turn(
        &mut self,
        turn: TimelineTurn,
    ) -> Result<TimelineNavigationSnapshot, TimelineNavigationError> {
        let Some(last) = self.turns.last_mut() else {
            return Err(TimelineNavigationError::TurnIdentityMismatch(
                turn.turn_id(),
            ));
        };
        if last.turn_id() != turn.turn_id() {
            return Err(TimelineNavigationError::TurnIdentityMismatch(
                turn.turn_id(),
            ));
        }
        *last = turn;
        self.anchor = self.selected_turn_id.and_then(|turn_id| {
            self.turns
                .last()
                .filter(|last| last.turn_id() == turn_id)
                .map(|last| ScrollAnchor::capture(last, self.scroll_top))
                .or(self.anchor)
        });
        Ok(self.snapshot())
    }

    pub const fn selected_turn_id(&self) -> Option<TurnId> {
        self.selected_turn_id
    }

    pub const fn scroll_top(&self) -> usize {
        self.scroll_top
    }

    pub const fn anchor(&self) -> Option<ScrollAnchor> {
        self.anchor
    }

    pub fn response_position(&self) -> Option<ResponsePosition> {
        super::response_navigation::position(&self.turns, self.selected_turn_id)
    }

    pub fn resize(&mut self, viewport_height: u16) {
        self.viewport_height = usize::from(viewport_height);
        let Some(turn) = self
            .selected_turn_id
            .and_then(|turn_id| self.turns.iter().find(|turn| turn.turn_id() == turn_id))
        else {
            return;
        };
        self.scroll_top = ensure_visible(self.scroll_top, turn.row, self.viewport_height);
        self.anchor = Some(ScrollAnchor::capture(turn, self.scroll_top));
    }

    fn target_index(&self, jump: TimelineJump) -> Result<Option<usize>, TimelineNavigationError> {
        if self.turns.is_empty() {
            return Ok(None);
        }
        let current = self.current_index()?;
        let index = match jump {
            TimelineJump::NextTurn => {
                Some(current.map_or(0, |index| (index + 1).min(self.turns.len() - 1)))
            }
            TimelineJump::PreviousTurn => Some(current.map_or(0, |index| index.saturating_sub(1))),
            TimelineJump::NextResponse => {
                super::response_navigation::target(&self.turns, current, true)
            }
            TimelineJump::PreviousResponse => {
                super::response_navigation::target(&self.turns, current, false)
            }
            TimelineJump::JumpToFailed => self.find_status(current, TimelineStatus::Failed),
            TimelineJump::JumpToStreaming => self.find_status(current, TimelineStatus::Streaming),
        };
        Ok(index)
    }

    fn current_index(&self) -> Result<Option<usize>, TimelineNavigationError> {
        let Some(selected) = self.selected_turn_id else {
            return Ok(None);
        };
        self.turns
            .iter()
            .position(|turn| turn.turn_id() == selected)
            .map(Some)
            .ok_or(TimelineNavigationError::SelectedTurnMissing(selected))
    }

    fn find_status(&self, current: Option<usize>, status: TimelineStatus) -> Option<usize> {
        let start = current.map_or(0, |index| index.saturating_add(1));
        self.turns
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, turn)| turn.marker.status == status)
            .map(|(index, _)| index)
            .or_else(|| {
                self.turns
                    .iter()
                    .enumerate()
                    .take(start.min(self.turns.len()))
                    .find(|(_, turn)| turn.marker.status == status)
                    .map(|(index, _)| index)
            })
    }

    fn snapshot(&self) -> TimelineNavigationSnapshot {
        TimelineNavigationSnapshot {
            selected_turn_id: self.selected_turn_id,
            scroll_top: self.scroll_top,
            anchor: self.anchor,
            response_position: self.response_position(),
        }
    }
}

fn validate_turns(
    identity: &TranscriptIdentity,
    turns: &[TimelineTurn],
) -> Result<(), TimelineNavigationError> {
    for turn in turns {
        if !identity
            .turns()
            .iter()
            .any(|candidate| candidate.id() == turn.turn_id())
        {
            return Err(TimelineNavigationError::TurnIdentityMismatch(
                turn.turn_id(),
            ));
        }
    }
    Ok(())
}

fn ensure_visible(scroll_top: usize, row: usize, viewport_height: usize) -> usize {
    if row < scroll_top {
        return row;
    }
    let Some(last_visible_row) = scroll_top.checked_add(viewport_height.saturating_sub(1)) else {
        return row;
    };
    if row > last_visible_row {
        row.saturating_sub(viewport_height.saturating_sub(1))
    } else {
        scroll_top
    }
}
