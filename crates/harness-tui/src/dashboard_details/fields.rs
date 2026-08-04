use std::collections::BTreeMap;

use harness_core::proj::SessionModeSource;

use crate::dashboard::{DashboardActivity, DashboardReplayRegistry, DashboardStatus, SelectionKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMetadata {
    pub run_name: Option<String>,
    pub workspace_root: Option<String>,
    pub profile_preset: Option<String>,
    pub provider_model: Option<String>,
    pub mode_source: SessionModeSource,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
    pub artifact_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailsAction {
    Attach,
    CycleNext,
    CyclePrevious,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetailsActions {
    pub can_attach: bool,
    pub can_cycle: bool,
    pub can_back: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailsPaneFields {
    pub session_id: SelectionKey,
    pub title: Option<String>,
    pub status: DashboardStatus,
    pub activity: DashboardActivity,
    pub metadata: SessionMetadata,
    pub parent: Option<SelectionKey>,
    pub children: Vec<SelectionKey>,
    pub lineage_depth: usize,
    pub parent_missing: bool,
    pub is_parent: bool,
    pub is_child: bool,
    pub is_background: bool,
    pub is_foreign: bool,
    pub actions: DetailsActions,
}

pub(crate) fn metadata_map(
    registry: &DashboardReplayRegistry,
) -> BTreeMap<SelectionKey, SessionMetadata> {
    registry
        .sessions
        .iter()
        .map(|session| {
            let catalog = &session.catalog;
            (
                SelectionKey::new(catalog.run_id.clone()),
                SessionMetadata {
                    run_name: catalog.run_name.clone(),
                    workspace_root: catalog.workspace_root.clone(),
                    profile_preset: catalog.profile_preset.clone(),
                    provider_model: catalog.provider_model.clone(),
                    mode_source: catalog.mode_source,
                    is_resumable: catalog.is_resumable,
                    resume_disabled_reason: catalog.resume_disabled_reason.clone(),
                    artifact_count: catalog.artifact_count,
                },
            )
        })
        .collect()
}
