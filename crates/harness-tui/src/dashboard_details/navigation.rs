use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::dashboard::{
    build_dashboard_read_model, DashboardEligibilityRules, DashboardProjectionError,
    DashboardReadModel, DashboardReplayRegistry, DashboardRow, SelectionKey,
};

use super::fields::{metadata_map, DetailsActions, DetailsPaneFields, SessionMetadata};
use super::layout::{self, DetailsLayout};
use super::restoration::{NavigationSnapshot, RosterState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleDirection {
    Next,
    Previous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationError {
    Projection(DashboardProjectionError),
    MissingSession(SelectionKey),
    StaleSession(SelectionKey),
    NoRelatedSession(SelectionKey),
    NoBackStack,
    MissingMetadata(SelectionKey),
}

impl Display for NavigationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Projection(error) => {
                write!(formatter, "dashboard details projection failed: {error}")
            }
            Self::MissingSession(id) => {
                write!(formatter, "session is not available: {}", id.as_str())
            }
            Self::StaleSession(id) => {
                write!(formatter, "current session disappeared: {}", id.as_str())
            }
            Self::NoRelatedSession(id) => {
                write!(
                    formatter,
                    "no related session is available for {}",
                    id.as_str()
                )
            }
            Self::NoBackStack => formatter.write_str("dashboard details back stack is empty"),
            Self::MissingMetadata(id) => {
                write!(
                    formatter,
                    "metadata is unavailable for session {}",
                    id.as_str()
                )
            }
        }
    }
}

impl std::error::Error for NavigationError {}

impl From<DashboardProjectionError> for NavigationError {
    fn from(error: DashboardProjectionError) -> Self {
        Self::Projection(error)
    }
}

#[derive(Clone, Debug)]
pub struct DashboardDetails {
    model: DashboardReadModel,
    metadata: BTreeMap<SelectionKey, SessionMetadata>,
    current_session_id: SelectionKey,
    back_stack: Vec<NavigationSnapshot>,
    roster: RosterState,
}

impl DashboardDetails {
    pub fn new(
        registry: &DashboardReplayRegistry,
        rules: &DashboardEligibilityRules,
        current_session_id: SelectionKey,
        roster: RosterState,
    ) -> Result<Self, NavigationError> {
        let model = build_dashboard_read_model(registry, rules)?;
        if model.row(current_session_id.as_str()).is_none()
            && !model
                .all_rows
                .iter()
                .any(|row| row.selection_key == current_session_id)
        {
            return Err(NavigationError::MissingSession(current_session_id));
        }
        Ok(Self {
            model,
            metadata: metadata_map(registry),
            current_session_id,
            back_stack: Vec::new(),
            roster,
        })
    }

    pub fn current_session_id(&self) -> &SelectionKey {
        &self.current_session_id
    }

    pub fn roster_state(&self) -> &RosterState {
        &self.roster
    }

    pub fn set_roster_state(&mut self, roster: RosterState) {
        self.roster = roster;
    }

    pub fn fields(&self) -> Result<DetailsPaneFields, NavigationError> {
        let row = self
            .model
            .all_rows
            .iter()
            .find(|row| row.selection_key == self.current_session_id)
            .ok_or_else(|| NavigationError::StaleSession(self.current_session_id.clone()))?;
        let metadata = self
            .metadata
            .get(&self.current_session_id)
            .cloned()
            .ok_or_else(|| NavigationError::MissingMetadata(self.current_session_id.clone()))?;
        let related = self.related_session_ids(row);
        Ok(DetailsPaneFields {
            session_id: row.selection_key.clone(),
            title: row.title.clone(),
            status: row.status,
            activity: row.activity.clone(),
            metadata,
            parent: row.relationship.parent.clone(),
            children: row.relationship.children.clone(),
            lineage_depth: row.relationship.lineage_depth,
            parent_missing: row.relationship.parent_missing,
            is_parent: row.relationship.is_parent,
            is_child: row.relationship.is_child,
            is_background: row.relationship.is_background,
            is_foreign: row.relationship.is_foreign,
            actions: DetailsActions {
                can_attach: !row.relationship.children.is_empty(),
                can_cycle: related.len() > 1,
                can_back: !self.back_stack.is_empty(),
            },
        })
    }

    pub fn attach(&mut self, target: &SelectionKey) -> Result<(), NavigationError> {
        self.ensure_available(target)?;
        if target == &self.current_session_id {
            return Ok(());
        }
        self.back_stack.push(NavigationSnapshot::new(
            self.current_session_id.clone(),
            self.roster.clone(),
        ));
        self.current_session_id = target.clone();
        Ok(())
    }

    pub fn cycle_related(&mut self, direction: CycleDirection) -> Result<(), NavigationError> {
        let row = self.current_row()?;
        let related = self.related_session_ids(row);
        if related.len() < 2 {
            return Err(NavigationError::NoRelatedSession(
                self.current_session_id.clone(),
            ));
        }
        let current_index = related
            .iter()
            .position(|id| id == &self.current_session_id)
            .ok_or_else(|| NavigationError::StaleSession(self.current_session_id.clone()))?;
        let next_index = match direction {
            CycleDirection::Next => (current_index + 1) % related.len(),
            CycleDirection::Previous => {
                if current_index == 0 {
                    related.len() - 1
                } else {
                    current_index - 1
                }
            }
        };
        let target = related
            .get(next_index)
            .ok_or_else(|| NavigationError::StaleSession(self.current_session_id.clone()))?;
        self.ensure_available(target)?;
        self.current_session_id = target.clone();
        Ok(())
    }

    pub fn back(&mut self) -> Result<(), NavigationError> {
        let snapshot = self.back_stack.pop().ok_or(NavigationError::NoBackStack)?;
        self.current_session_id = snapshot.session_id;
        self.roster = snapshot.roster;
        Ok(())
    }

    pub fn refresh(
        &mut self,
        registry: &DashboardReplayRegistry,
        rules: &DashboardEligibilityRules,
    ) -> Result<(), NavigationError> {
        self.model = build_dashboard_read_model(registry, rules)?;
        self.metadata = metadata_map(registry);
        Ok(())
    }

    pub fn layout_for(&self, viewport: ratatui::layout::Rect) -> DetailsLayout {
        layout::layout_for(viewport)
    }

    fn current_row(&self) -> Result<&DashboardRow, NavigationError> {
        self.model
            .all_rows
            .iter()
            .find(|row| row.selection_key == self.current_session_id)
            .ok_or_else(|| NavigationError::StaleSession(self.current_session_id.clone()))
    }

    fn ensure_available(&self, target: &SelectionKey) -> Result<(), NavigationError> {
        if self
            .model
            .all_rows
            .iter()
            .any(|row| row.selection_key == *target)
        {
            Ok(())
        } else {
            Err(NavigationError::MissingSession(target.clone()))
        }
    }

    fn related_session_ids(&self, row: &DashboardRow) -> Vec<SelectionKey> {
        let candidate_ids = row
            .relationship
            .parent
            .as_ref()
            .and_then(|parent| {
                self.model
                    .all_rows
                    .iter()
                    .find(|candidate| candidate.selection_key == *parent)
            })
            .map(|parent| parent.relationship.children.clone())
            .unwrap_or_else(|| row.relationship.children.clone());
        candidate_ids
            .into_iter()
            .filter(|id| {
                self.model
                    .all_rows
                    .iter()
                    .any(|candidate| candidate.selection_key == *id)
            })
            .collect()
    }
}
