use std::collections::{BTreeMap, BTreeSet};

use crate::dashboard::{
    DashboardGroupKey, DashboardReadModel, DashboardRow, DashboardStatus, SelectionKey,
};

use super::hit_map::{RosterHitMap, RosterHitTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineageFilter {
    #[default]
    Any,
    Root,
    Parent,
    Child,
    Orphaned,
    Background,
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RosterFilter {
    pub query: String,
    pub status: Option<DashboardStatus>,
    pub lineage: LineageFilter,
}

impl RosterFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    pub fn with_search(self, query: impl Into<String>) -> Self {
        self.with_query(query)
    }

    pub const fn with_status(mut self, status: DashboardStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub const fn with_lineage(mut self, lineage: LineageFilter) -> Self {
        self.lineage = lineage;
        self
    }

    pub fn matches(&self, row: &DashboardRow) -> bool {
        self.status.is_none_or(|status| status == row.status)
            && matches_lineage(self.lineage, row)
            && self
                .query
                .split_whitespace()
                .all(|term| query_term_matches(term, row))
    }

    pub fn matching_keys(&self, model: &DashboardReadModel) -> Vec<SelectionKey> {
        model
            .rows
            .iter()
            .filter(|row| self.matches(row))
            .map(|row| row.selection_key.clone())
            .collect()
    }

    pub fn matching_rows(&self, model: &DashboardReadModel) -> Vec<DashboardRow> {
        model
            .rows
            .iter()
            .filter(|row| self.matches(row))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredRosterGroup {
    pub key: DashboardGroupKey,
    pub rows: Vec<DashboardRow>,
    pub folded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredRoster {
    pub rows: Vec<DashboardRow>,
    pub groups: Vec<FilteredRosterGroup>,
}

pub fn filter_model(model: &DashboardReadModel, filter: &RosterFilter) -> FilteredRoster {
    let rows = filter.matching_rows(model);
    let mut grouped = BTreeMap::<DashboardGroupKey, Vec<DashboardRow>>::new();
    for row in &rows {
        grouped
            .entry(row.relationship.group.clone())
            .or_default()
            .push(row.clone());
    }
    let groups = grouped
        .into_iter()
        .map(|(key, rows)| FilteredRosterGroup {
            key,
            rows,
            folded: false,
        })
        .collect();
    FilteredRoster { rows, groups }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RosterState {
    pub filter: RosterFilter,
    pub folded_groups: BTreeSet<DashboardGroupKey>,
    pub pinned_rows: BTreeSet<SelectionKey>,
    pub selected: Option<SelectionKey>,
    pub hovered: Option<SelectionKey>,
    pub scroll_top: usize,
}

impl RosterState {
    pub fn set_filter(&mut self, filter: RosterFilter) {
        self.filter = filter;
        self.scroll_top = 0;
    }

    pub fn toggle_fold(&mut self, group: DashboardGroupKey) {
        if !self.folded_groups.insert(group.clone()) {
            self.folded_groups.remove(&group);
        }
    }

    pub fn is_folded(&self, group: &DashboardGroupKey) -> bool {
        self.folded_groups.contains(group)
    }

    pub fn toggle_pin(&mut self, key: SelectionKey) {
        if !self.pinned_rows.insert(key.clone()) {
            self.pinned_rows.remove(&key);
        }
    }

    pub fn is_pinned(&self, key: &SelectionKey) -> bool {
        self.pinned_rows.contains(key)
    }

    pub fn set_selected(&mut self, key: Option<SelectionKey>) {
        self.selected = key;
    }

    pub fn set_hovered(&mut self, key: Option<SelectionKey>) {
        self.hovered = key;
    }

    pub fn selected_key(&self) -> Option<&SelectionKey> {
        self.selected.as_ref()
    }

    pub fn hovered_key(&self) -> Option<&SelectionKey> {
        self.hovered.as_ref()
    }

    pub fn select_at(&mut self, hit_map: &RosterHitMap, x: u16, y: u16) -> Option<SelectionKey> {
        let target = hit_map.hit_test(x, y);
        if let Some(RosterHitTarget::Row(key)) = target {
            self.selected = Some(key.clone());
            return Some(key);
        }
        None
    }

    pub fn hover_at(&mut self, hit_map: &RosterHitMap, x: u16, y: u16) -> Option<SelectionKey> {
        let target = hit_map.hit_test(x, y);
        if let Some(RosterHitTarget::Row(key)) = target {
            self.hovered = Some(key.clone());
            return Some(key);
        }
        self.hovered = None;
        None
    }

    pub fn reconcile(&mut self, model: &DashboardReadModel) {
        self.pinned_rows
            .retain(|key| model.all_rows.iter().any(|row| row.selection_key == *key));
        self.selected = self
            .selected
            .take()
            .filter(|key| model.all_rows.iter().any(|row| row.selection_key == *key));
        self.hovered = self
            .hovered
            .take()
            .filter(|key| model.all_rows.iter().any(|row| row.selection_key == *key));
    }
}

fn matches_lineage(filter: LineageFilter, row: &DashboardRow) -> bool {
    match filter {
        LineageFilter::Any => true,
        LineageFilter::Root => row.relationship.parent.is_none(),
        LineageFilter::Parent => row.relationship.is_parent,
        LineageFilter::Child => row.relationship.is_child,
        LineageFilter::Orphaned => row.relationship.parent_missing,
        LineageFilter::Background => row.relationship.is_background,
        LineageFilter::Foreign => row.relationship.is_foreign,
    }
}

fn query_term_matches(term: &str, row: &DashboardRow) -> bool {
    let term = term.to_lowercase();
    if let Some(value) = term.strip_prefix("status:") {
        return status_name(row.status) == value;
    }
    if let Some(value) = term.strip_prefix("lineage:") {
        return lineage_name(value, row);
    }
    let title = row
        .title
        .as_deref()
        .map_or_else(String::new, str::to_lowercase);
    let parent = row
        .relationship
        .parent
        .as_ref()
        .map_or_else(String::new, |key| key.as_str().to_lowercase());
    let group = match &row.relationship.group {
        DashboardGroupKey::Root(key) | DashboardGroupKey::Orphaned(key) => {
            key.as_str().to_lowercase()
        }
    };
    title.contains(&term)
        || row.selection_key.as_str().to_lowercase().contains(&term)
        || status_name(row.status).contains(&term)
        || parent.contains(&term)
        || group.contains(&term)
        || lineage_name(&term, row)
}

fn lineage_name(value: &str, row: &DashboardRow) -> bool {
    match value {
        "root" => row.relationship.parent.is_none(),
        "parent" => row.relationship.is_parent,
        "child" => row.relationship.is_child,
        "orphaned" => row.relationship.parent_missing,
        "background" => row.relationship.is_background,
        "foreign" => row.relationship.is_foreign,
        _ => false,
    }
}

const fn status_name(status: DashboardStatus) -> &'static str {
    match status {
        DashboardStatus::Running => "running",
        DashboardStatus::Queued => "queued",
        DashboardStatus::Streaming => "streaming",
        DashboardStatus::Completed => "completed",
        DashboardStatus::Failed => "failed",
        DashboardStatus::Cancelled => "cancelled",
        DashboardStatus::Stale => "stale",
    }
}
