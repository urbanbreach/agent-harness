//! Retain the newest complete assistant/tool group even when it exceeds budget.
use crate::{Item, ItemKind, Origin};
use harness_providers::MessageRole;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutPoint {
    pub first_kept: usize,
    pub turn_start: Option<usize>,
    pub is_split: bool,
}
/// A harness-authored note (reminder, child completion, wake) is context, not a cut point.
fn host_note(item: &Item<'_>) -> bool {
    item.kind == ItemKind::Note && item.origin == Origin::Host
}

/// User and assistant entries can start the kept tail. Tool results never can, and harness
/// notes never do; branch summaries and the active summary keep their user role.
fn is_boundary(item: &Item<'_>) -> bool {
    matches!(item.role(), MessageRole::User | MessageRole::Assistant) && !host_note(item)
}

/// Entries that begin a turn: user-role entries other than harness notes.
fn starts_turn(item: &Item<'_>) -> bool {
    item.is_user() && !host_note(item)
}

pub fn find_cut_point(
    items: &[Item<'_>],
    start: usize,
    end: usize,
    keep_recent_tokens: u32,
) -> CutPoint {
    let end = end.min(items.len());
    let start = start.min(end);
    let boundary = |index: usize| is_boundary(&items[index]);
    let Some(mut selected) = (start..end).rev().find(|index| boundary(*index)) else {
        return CutPoint {
            first_kept: start,
            turn_start: None,
            is_split: false,
        };
    };
    let mut total = 0u32;
    for index in (start..end).rev() {
        total = total.saturating_add(items[index].tokens);
        if boundary(index) {
            if total > keep_recent_tokens {
                break;
            }
            selected = index;
        }
    }
    let turn_start = if starts_turn(&items[selected]) {
        None
    } else {
        (start..selected)
            .rev()
            .find(|index| starts_turn(&items[*index]))
    };
    CutPoint {
        first_kept: selected,
        turn_start,
        is_split: turn_start.is_some(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare::tests::item;
    use harness_providers::CompletionMessage;
    #[test]
    fn newest_group_and_turn_alignment() {
        let messages = [
            CompletionMessage::text(MessageRole::Assistant, "old"),
            CompletionMessage::text(MessageRole::Assistant, "new"),
            CompletionMessage::text(MessageRole::Tool, "result"),
        ];
        let mut items = messages.iter().map(item).collect::<Vec<_>>();
        items[0].tokens = 1_000;
        items[1].tokens = 1_000;
        items[2].tokens = 25_000;
        for (budget, first) in [(20_000, 1), (26_000, 1), (27_000, 0)] {
            assert_eq!(
                find_cut_point(&items, 0, 3, budget),
                CutPoint {
                    first_kept: first,
                    turn_start: None,
                    is_split: false
                }
            );
        }
        assert_eq!(find_cut_point(&items, 2, 3, 1).first_kept, 2);
    }
    #[test]
    fn host_notes_are_never_boundaries_or_turn_starts() {
        use crate::NoteKind;
        let user = (MessageRole::User, ItemKind::Message, Origin::User);
        let host = (MessageRole::User, ItemKind::Note, Origin::Host);
        let agent = (MessageRole::User, ItemKind::Note, Origin::Agent);
        let branch = (MessageRole::User, ItemKind::BranchSummary, Origin::Host);
        let assistant = (MessageRole::Assistant, ItemKind::Message, Origin::User);
        for (name, shape, expected) in [
            (
                "host note skipped as turn start",
                vec![user, assistant, host, assistant],
                (3, Some(0), true),
            ),
            (
                "trailing host note is not the cut",
                vec![user, assistant, host],
                (1, Some(0), true),
            ),
            (
                "agent steer is a turn start",
                vec![user, assistant, agent, assistant],
                (3, Some(2), true),
            ),
            (
                "branch summary is a turn start",
                vec![assistant, branch, assistant],
                (2, Some(1), true),
            ),
            (
                "only host notes means no boundary",
                vec![host, host],
                (0, None, false),
            ),
        ] {
            let messages = shape
                .iter()
                .map(|(role, _, _)| CompletionMessage::text(*role, "x"))
                .collect::<Vec<_>>();
            let mut items = messages.iter().map(item).collect::<Vec<_>>();
            for (item, (_, kind, origin)) in items.iter_mut().zip(&shape) {
                item.kind = *kind;
                item.origin = *origin;
                item.note_kind = (*kind == ItemKind::Note).then_some(NoteKind::Reminder);
                item.tokens = 10;
            }
            let cut = find_cut_point(&items, 0, items.len(), 1);
            assert_eq!(
                (cut.first_kept, cut.turn_start, cut.is_split),
                expected,
                "{name}"
            );
        }
    }
}
