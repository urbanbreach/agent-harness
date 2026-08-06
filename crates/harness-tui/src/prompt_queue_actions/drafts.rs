use super::state::QueueState;

pub(crate) fn preserve_draft(before: &QueueState, mut after: QueueState) -> QueueState {
    after.draft.clone_from(&before.draft);
    after
}

pub fn draft_preserved(before: &QueueState, after: &QueueState) -> bool {
    before.draft == after.draft
}
