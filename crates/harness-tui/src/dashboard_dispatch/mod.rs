mod composer;
mod queue;
mod stale_target;

pub use composer::{DashboardReplyComposer, ReplyAttachment, ReplyComposerError, ReplyPayload};
pub use queue::{
    CoordinatorValidationError, CoordinatorValidator, DispatchAction, DispatchError, DispatchIntent,
};
pub use stale_target::{
    AttachmentCapability, DispatchCapability, StaleTargetError, StaleTargetGuard,
    TargetCapabilities, TargetIdentity, TargetLifecycle, TargetSnapshot,
};

use std::collections::BTreeMap;

use crate::attachment_lifecycle::Attachment;
use crate::completion_controller::{CompletionItem, CompletionRequest, CompletionTrigger};
use crate::dashboard_peek::DashboardDrafts;

use self::queue::{PreparedDispatch, QueueMap, TargetDispatchQueue, composer_payload};

pub struct DashboardDispatch {
    targets: BTreeMap<TargetIdentity, TargetSnapshot>,
    composers: BTreeMap<TargetIdentity, DashboardReplyComposer>,
    queues: QueueMap,
    drafts: DashboardDrafts,
    selected: Option<TargetIdentity>,
}

impl DashboardDispatch {
    pub fn new() -> Self {
        Self {
            targets: BTreeMap::new(),
            composers: BTreeMap::new(),
            queues: BTreeMap::new(),
            drafts: DashboardDrafts::new(),
            selected: None,
        }
    }

    pub fn register_target(&mut self, snapshot: TargetSnapshot) -> Result<(), DispatchError> {
        let identity = snapshot.identity.clone();
        if let Some(composer) = self.composers.get_mut(&identity) {
            composer.refresh(&snapshot);
        } else {
            let draft = self
                .drafts
                .get(identity.selection_key())
                .unwrap_or_default();
            self.composers.insert(
                identity.clone(),
                DashboardReplyComposer::new(&snapshot, draft),
            );
        }
        self.queues
            .entry(identity.clone())
            .or_insert_with(|| TargetDispatchQueue::new(&snapshot))
            .refresh(snapshot.queue_lifecycle);
        self.targets.insert(identity, snapshot);
        Ok(())
    }

    pub fn select_target(&mut self, session_id: impl AsRef<str>) -> Result<(), DispatchError> {
        self.sync_selected_draft();
        let identity = TargetIdentity::new(session_id.as_ref());
        if !self.targets.contains_key(&identity) {
            return Err(DispatchError::UnknownTarget(session_id.as_ref().to_owned()));
        }
        self.selected = Some(identity);
        Ok(())
    }

    pub fn remove_target(&mut self, session_id: impl AsRef<str>) -> Result<(), DispatchError> {
        let identity = TargetIdentity::new(session_id.as_ref());
        let Some(snapshot) = self.targets.get(&identity).cloned() else {
            return Err(DispatchError::UnknownTarget(session_id.as_ref().to_owned()));
        };
        let removed = TargetSnapshot::new(
            identity,
            TargetLifecycle::Removed,
            snapshot.revision.saturating_add(1),
        );
        self.register_target(removed)
    }

    pub fn selected_target(&self) -> Option<&TargetIdentity> {
        self.selected.as_ref()
    }

    pub fn selected_composer(&self) -> Result<&DashboardReplyComposer, DispatchError> {
        let identity = self
            .selected
            .as_ref()
            .ok_or(DispatchError::NoSelectedTarget)?;
        self.composers
            .get(identity)
            .ok_or_else(|| DispatchError::UnknownTarget(identity.to_string()))
    }

    pub fn selected_composer_mut(&mut self) -> Result<&mut DashboardReplyComposer, DispatchError> {
        let identity = self
            .selected
            .clone()
            .ok_or(DispatchError::NoSelectedTarget)?;
        self.composers
            .get_mut(&identity)
            .ok_or_else(|| DispatchError::UnknownTarget(identity.to_string()))
    }

    pub fn draft_for(&self, session_id: impl AsRef<str>) -> Option<&str> {
        let identity = TargetIdentity::new(session_id.as_ref());
        self.drafts.get(identity.selection_key())
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), DispatchError> {
        self.selected_composer_mut()?.insert_text(text)?;
        self.sync_selected_draft();
        Ok(())
    }

    pub fn attach(&mut self, id: u64, attachment: Attachment) -> Result<(), DispatchError> {
        self.selected_composer_mut()?.attach(id, attachment)?;
        self.sync_selected_draft();
        Ok(())
    }

    pub fn begin_completion(
        &mut self,
        trigger: CompletionTrigger,
    ) -> Result<CompletionRequest, DispatchError> {
        Ok(self.selected_composer_mut()?.begin_completion(trigger)?)
    }

    pub fn apply_completion_results(
        &mut self,
        request: &CompletionRequest,
        results: Vec<CompletionItem>,
    ) -> Result<(), DispatchError> {
        self.selected_composer_mut()?
            .apply_completion_results(request, results)?;
        Ok(())
    }

    pub fn accept_completion_keyboard(&mut self) -> Result<(), DispatchError> {
        self.selected_composer_mut()?.accept_completion_keyboard()?;
        self.sync_selected_draft();
        Ok(())
    }

    pub fn queue(&mut self) -> Result<DispatchIntent, DispatchError> {
        self.dispatch(DispatchAction::Queue)
    }

    pub fn interject(&mut self) -> Result<DispatchIntent, DispatchError> {
        self.dispatch(DispatchAction::Interject)
    }

    pub fn send_now(&mut self) -> Result<DispatchIntent, DispatchError> {
        self.dispatch(DispatchAction::SendNow)
    }

    pub fn dispatch(&mut self, action: DispatchAction) -> Result<DispatchIntent, DispatchError> {
        let (identity, prepared) = self.prepare(action)?;
        self.commit(identity, prepared)
    }

    pub fn dispatch_with<V: CoordinatorValidator>(
        &mut self,
        action: DispatchAction,
        validator: &V,
    ) -> Result<DispatchIntent, DispatchError> {
        let (identity, prepared) = self.prepare(action)?;
        validator
            .validate(&prepared.intent)
            .map_err(DispatchError::Coordinator)?;
        self.commit(identity, prepared)
    }

    fn prepare(
        &mut self,
        action: DispatchAction,
    ) -> Result<(TargetIdentity, PreparedDispatch), DispatchError> {
        self.sync_selected_draft();
        let identity = self
            .selected
            .clone()
            .ok_or(DispatchError::NoSelectedTarget)?;
        let snapshot = self
            .targets
            .get(&identity)
            .ok_or_else(|| DispatchError::UnknownTarget(identity.to_string()))?;
        let composer = self
            .composers
            .get(&identity)
            .ok_or_else(|| DispatchError::UnknownTarget(identity.to_string()))?;
        let payload = composer_payload(composer)?;
        let queue = self
            .queues
            .get(&identity)
            .ok_or_else(|| DispatchError::UnknownTarget(identity.to_string()))?;
        let prepared = queue.prepare(snapshot, composer.guard(), action, payload)?;
        Ok((identity, prepared))
    }

    fn commit(
        &mut self,
        identity: TargetIdentity,
        prepared: PreparedDispatch,
    ) -> Result<DispatchIntent, DispatchError> {
        self.queues
            .get_mut(&identity)
            .map(|queue| queue.commit(prepared))
            .ok_or_else(|| DispatchError::UnknownTarget(identity.to_string()))
    }

    fn sync_selected_draft(&mut self) {
        let Some(identity) = self.selected.clone() else {
            return;
        };
        let Some(composer) = self.composers.get(&identity) else {
            return;
        };
        self.drafts
            .set(identity.selection_key().clone(), composer.text());
    }
}

impl Default for DashboardDispatch {
    fn default() -> Self {
        Self::new()
    }
}
