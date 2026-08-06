use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use harness_core::event::{EventEnvelopeV1, EventV1};

use super::eligibility::{DashboardEntryEligibility, SelectionKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardStatus {
    Running,
    Queued,
    Streaming,
    Completed,
    Failed,
    Cancelled,
    Stale,
}

impl DashboardStatus {
    pub(crate) const fn sort_rank(self) -> u8 {
        match self {
            Self::Running | Self::Streaming => 0,
            Self::Queued => 1,
            Self::Completed => 2,
            Self::Failed | Self::Cancelled => 3,
            Self::Stale => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardActivity {
    pub last_event_seq: u64,
    pub last_event_id: Option<String>,
    pub unread_count: usize,
}

impl DashboardActivity {
    pub const fn is_unread(&self) -> bool {
        self.unread_count > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DashboardGroupKey {
    Root(SelectionKey),
    Orphaned(SelectionKey),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardRelationship {
    pub parent: Option<SelectionKey>,
    pub children: Vec<SelectionKey>,
    pub group: DashboardGroupKey,
    pub lineage_depth: usize,
    pub parent_missing: bool,
    pub is_parent: bool,
    pub is_child: bool,
    pub is_background: bool,
    pub is_foreign: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardRow {
    pub selection_key: SelectionKey,
    pub title: Option<String>,
    pub status: DashboardStatus,
    pub activity: DashboardActivity,
    pub relationship: DashboardRelationship,
    pub eligibility: DashboardEntryEligibility,
    pub creation_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardGroup {
    pub key: DashboardGroupKey,
    pub row_keys: Vec<SelectionKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardReadModel {
    /// Eligible rows in deterministic display order.
    pub rows: Vec<DashboardRow>,
    /// All replay-derived rows, including rows excluded by configured rules.
    pub all_rows: Vec<DashboardRow>,
    pub groups: Vec<DashboardGroup>,
}

impl DashboardReadModel {
    pub fn row(&self, selection_key: &str) -> Option<&DashboardRow> {
        self.all_rows
            .iter()
            .find(|row| row.selection_key.as_str() == selection_key)
    }

    pub fn eligible_selection_keys(&self) -> Vec<SelectionKey> {
        self.rows
            .iter()
            .map(|row| row.selection_key.clone())
            .collect()
    }

    pub fn fallback_selection(&self, requested: Option<&SelectionKey>) -> Option<SelectionKey> {
        requested
            .filter(|key| self.rows.iter().any(|row| row.selection_key == **key))
            .cloned()
            .or_else(|| self.rows.first().map(|row| row.selection_key.clone()))
    }

    /// Sort by active, queued, completed, failed/cancelled, then stale state.
    /// Ties use newest creation sequence, shallower lineage, and stable ID.
    pub(crate) fn sort_rows(rows: &mut [DashboardRow]) {
        rows.sort_by(|left, right| {
            left.status
                .sort_rank()
                .cmp(&right.status.sort_rank())
                .then_with(|| right.creation_seq.cmp(&left.creation_seq))
                .then_with(|| {
                    left.relationship
                        .lineage_depth
                        .cmp(&right.relationship.lineage_depth)
                })
                .then_with(|| left.selection_key.cmp(&right.selection_key))
        });
    }

    pub(crate) fn compare_group_keys(
        left: &DashboardGroupKey,
        right: &DashboardGroupKey,
    ) -> Ordering {
        left.cmp(right)
    }

    pub(crate) fn event_parent_id(events: &[&EventEnvelopeV1]) -> Option<String> {
        events.iter().find_map(|event| match &event.payload {
            EventV1::BackgroundTaskNotification(notification) => {
                Some(notification.parent_session_id.to_string())
            }
            _ => event.lineage_parent_session_id().map(str::to_string),
        })
    }

    pub(crate) fn event_child_links(events: &[&EventEnvelopeV1]) -> Vec<(String, String)> {
        events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::BackgroundTaskNotification(notification) => Some((
                    notification.parent_session_id.to_string(),
                    notification.child_session_id.to_string(),
                )),
                EventV1::ToolCallRequested(payload) => payload
                    .metadata
                    .as_ref()?
                    .lineage
                    .as_ref()
                    .and_then(|lineage| {
                        Some((
                            lineage
                                .parent_session_id
                                .clone()
                                .unwrap_or_else(|| event.run_id.to_string()),
                            lineage.child_session_id.clone()?,
                        ))
                    }),
                EventV1::ToolCallFinished(payload) => payload
                    .metadata
                    .as_ref()?
                    .lineage
                    .as_ref()
                    .and_then(|lineage| {
                        Some((
                            lineage
                                .parent_session_id
                                .clone()
                                .unwrap_or_else(|| event.run_id.to_string()),
                            lineage.child_session_id.clone()?,
                        ))
                    }),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn group_for(
        key: &SelectionKey,
        parents: &BTreeMap<SelectionKey, Option<String>>,
        known_keys: &BTreeSet<SelectionKey>,
    ) -> (DashboardGroupKey, usize, bool) {
        let mut current = key.clone();
        let mut seen = BTreeSet::new();
        let mut depth = 0;
        loop {
            if !seen.insert(current.clone()) {
                let cycle_root = seen.into_iter().min().unwrap_or_else(|| key.clone());
                return (DashboardGroupKey::Root(cycle_root), depth, false);
            }
            let Some(parent) = parents.get(&current).and_then(Option::as_ref) else {
                return (DashboardGroupKey::Root(current), depth, false);
            };
            let parent_key = SelectionKey::new(parent.clone());
            if !known_keys.contains(&parent_key) {
                return (DashboardGroupKey::Orphaned(parent_key), depth, true);
            }
            current = parent_key;
            depth += 1;
        }
    }

    pub(crate) fn project_groups(rows: &[DashboardRow]) -> Vec<DashboardGroup> {
        let mut grouped = BTreeMap::<DashboardGroupKey, Vec<SelectionKey>>::new();
        for row in rows {
            grouped
                .entry(row.relationship.group.clone())
                .or_default()
                .push(row.selection_key.clone());
        }
        let mut groups = grouped
            .into_iter()
            .map(|(key, row_keys)| DashboardGroup { key, row_keys })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| Self::compare_group_keys(&left.key, &right.key));
        groups
    }
}
