use crate::transcript_identity::TurnId;

use super::markers::{TimelineStatus, TimelineTurn};
use super::navigation_state::ResponsePosition;

pub(super) fn position(
    turns: &[TimelineTurn],
    selected_turn_id: Option<TurnId>,
) -> Option<ResponsePosition> {
    let selected = selected_turn_id?;
    let total = turns
        .iter()
        .filter(|turn| turn.marker.status == TimelineStatus::Completed)
        .count();
    turns
        .iter()
        .filter(|turn| turn.marker.status == TimelineStatus::Completed)
        .position(|turn| turn.turn_id() == selected)
        .map(|index| ResponsePosition {
            index: index.saturating_add(1),
            total,
        })
}

pub(super) fn target(
    turns: &[TimelineTurn],
    current: Option<usize>,
    forward: bool,
) -> Option<usize> {
    let first = turns
        .iter()
        .position(|turn| turn.marker.status == TimelineStatus::Completed)?;
    let last = turns
        .iter()
        .rposition(|turn| turn.marker.status == TimelineStatus::Completed)?;
    let Some(current) = current else {
        return Some(first);
    };
    if forward {
        turns
            .iter()
            .enumerate()
            .skip(current.saturating_add(1))
            .find(|(_, turn)| turn.marker.status == TimelineStatus::Completed)
            .map_or(Some(last), |(index, _)| Some(index))
    } else {
        turns
            .iter()
            .enumerate()
            .take(current)
            .rev()
            .find(|(_, turn)| turn.marker.status == TimelineStatus::Completed)
            .map_or(Some(first), |(index, _)| Some(index))
    }
}
