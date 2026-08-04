use crate::dashboard::SelectionKey;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterState {
    pub selected_id: Option<SelectionKey>,
    pub filter: String,
    pub scroll: usize,
    pub draft: String,
}

impl RosterState {
    pub fn new(
        selected_id: SelectionKey,
        filter: impl Into<String>,
        scroll: usize,
        draft: impl Into<String>,
    ) -> Self {
        Self {
            selected_id: Some(selected_id),
            filter: filter.into(),
            scroll,
            draft: draft.into(),
        }
    }

    pub fn empty(filter: impl Into<String>, draft: impl Into<String>) -> Self {
        Self {
            selected_id: None,
            filter: filter.into(),
            scroll: 0,
            draft: draft.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationSnapshot {
    pub session_id: SelectionKey,
    pub roster: RosterState,
}

impl NavigationSnapshot {
    pub(crate) fn new(session_id: SelectionKey, roster: RosterState) -> Self {
        Self { session_id, roster }
    }
}
