use crate::dashboard::SelectionKey;
use crate::dashboard::{DashboardReadModel, DashboardStatus};
use crate::transcript_blocks::BlockSnapshot;
use crate::transcript_integration::TranscriptViewModel;
use crate::transcript_scroll::FollowMode;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DashboardDrafts {
    drafts: BTreeMap<SelectionKey, String>,
}

impl DashboardDrafts {
    pub const fn new() -> Self {
        Self {
            drafts: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, session_id: SelectionKey, draft: impl Into<String>) {
        self.drafts.insert(session_id, draft.into());
    }

    pub fn get(&self, session_id: &SelectionKey) -> Option<&str> {
        self.drafts.get(session_id).map(String::as_str)
    }

    pub fn remove(&mut self, session_id: &SelectionKey) -> Option<String> {
        self.drafts.remove(session_id)
    }

    pub fn len(&self) -> usize {
        self.drafts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.drafts.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardPeekView {
    pub session_id: SelectionKey,
    pub blocks: Vec<BlockSnapshot>,
    pub draft: String,
    pub unread_count: usize,
    pub follow: FollowMode,
    pub scroll_top: f64,
    pub revision: u64,
    pub status: super::PeekSessionStatus,
}

impl super::DashboardPeek {
    pub fn select(&mut self, session_id: &SelectionKey) -> Result<(), super::DashboardPeekError> {
        if !self.order.contains(session_id) {
            return Err(self.unknown(session_id));
        }
        self.selected = Some(session_id.clone());
        Ok(())
    }

    pub fn finish(&mut self, session_id: &SelectionKey) -> Result<(), super::DashboardPeekError> {
        self.state_mut(session_id)?.status = super::PeekSessionStatus::Finished;
        Ok(())
    }

    pub fn sync_dashboard(
        &mut self,
        model: &DashboardReadModel,
    ) -> Result<(), super::DashboardPeekError> {
        let session_ids: Vec<SelectionKey> = model
            .rows
            .iter()
            .map(|row| row.selection_key.clone())
            .collect();
        self.sync_sessions(&session_ids)?;
        for row in &model.rows {
            if let Some(state) = self.sessions.get_mut(&row.selection_key) {
                state.status = status_for(row.status);
            }
        }
        Ok(())
    }

    pub fn set_draft(&mut self, draft: impl Into<String>) -> Result<(), super::DashboardPeekError> {
        let session_id = self.selected_id()?;
        self.drafts.set(session_id, draft);
        Ok(())
    }

    pub fn draft_for(&self, session_id: &SelectionKey) -> Option<&str> {
        self.drafts.get(session_id)
    }

    pub fn replace_from_view(
        &mut self,
        session_id: &SelectionKey,
        view: &TranscriptViewModel,
    ) -> Result<super::TailUpdate, super::DashboardPeekError> {
        let update = self.replace_blocks(session_id, &view.blocks)?;
        if let Some(layout) = view.layout.clone() {
            self.set_layout(session_id, layout)?;
        }
        Ok(update)
    }

    pub fn view(&self) -> Result<DashboardPeekView, super::DashboardPeekError> {
        let session_id = self.selected_id()?;
        self.view_for(&session_id)
    }

    pub fn view_for(
        &self,
        session_id: &SelectionKey,
    ) -> Result<DashboardPeekView, super::DashboardPeekError> {
        let state = self.state(session_id)?;
        let draft = self
            .drafts
            .get(session_id)
            .map_or_else(String::new, str::to_owned);
        Ok(DashboardPeekView {
            session_id: session_id.clone(),
            blocks: state.tail.snapshot(),
            draft,
            unread_count: state.unread_count,
            follow: state.scroll.follow(),
            scroll_top: state.scroll.scroll_top(),
            revision: state.tail.revision(),
            status: state.status,
        })
    }

    pub fn cached_block_count(&self, session_id: &SelectionKey) -> Option<usize> {
        self.sessions.get(session_id).map(|state| state.tail.len())
    }

    pub fn cached_session_count(&self) -> usize {
        self.sessions.len()
    }
}

fn status_for(status: DashboardStatus) -> super::PeekSessionStatus {
    match status {
        DashboardStatus::Running | DashboardStatus::Queued | DashboardStatus::Streaming => {
            super::PeekSessionStatus::Active
        }
        DashboardStatus::Completed
        | DashboardStatus::Failed
        | DashboardStatus::Cancelled
        | DashboardStatus::Stale => super::PeekSessionStatus::Finished,
    }
}
