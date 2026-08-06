use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

use super::eligibility::{self, DashboardEligibilityRules, SelectionKey, SelectionKeyError};
use super::model::{
    DashboardActivity, DashboardGroupKey, DashboardReadModel, DashboardRelationship, DashboardRow,
    DashboardStatus,
};
use super::status::derive_status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardSessionInput {
    pub catalog: SessionCatalogEntry,
    pub events: Vec<EventEnvelopeV1>,
    pub read_through_seq: u64,
    pub is_background: bool,
    pub is_foreign: bool,
}

impl DashboardSessionInput {
    pub fn new(catalog: SessionCatalogEntry, events: Vec<EventEnvelopeV1>) -> Self {
        let is_foreign = matches!(catalog.mode_source, SessionModeSource::ReplayOnly);
        Self {
            catalog,
            events,
            read_through_seq: 0,
            is_background: false,
            is_foreign,
        }
    }

    pub const fn with_read_through_seq(mut self, seq: u64) -> Self {
        self.read_through_seq = seq;
        self
    }

    pub const fn with_background(mut self, is_background: bool) -> Self {
        self.is_background = is_background;
        self
    }

    pub const fn with_foreign(mut self, is_foreign: bool) -> Self {
        self.is_foreign = is_foreign;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardReplayRegistry {
    pub sessions: Vec<DashboardSessionInput>,
}

impl DashboardReplayRegistry {
    pub fn from_sessions(sessions: Vec<DashboardSessionInput>) -> Self {
        Self { sessions }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardProjectionError {
    EmptySessionId,
    DuplicateSessionId(String),
}

impl Display for DashboardProjectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySessionId => formatter.write_str("dashboard session id must be non-empty"),
            Self::DuplicateSessionId(id) => {
                write!(formatter, "duplicate dashboard session id: {id}")
            }
        }
    }
}

impl std::error::Error for DashboardProjectionError {}

pub fn build_dashboard_read_model(
    registry: &DashboardReplayRegistry,
    rules: &DashboardEligibilityRules,
) -> Result<DashboardReadModel, DashboardProjectionError> {
    let mut keys = BTreeSet::new();
    let mut rows = Vec::with_capacity(registry.sessions.len());
    let mut inferred_parents = BTreeMap::new();
    let mut linked_children = BTreeMap::<String, BTreeSet<String>>::new();

    for session in &registry.sessions {
        let key = SelectionKey::try_new(session.catalog.run_id.clone()).map_err(map_key_error)?;
        if !keys.insert(key.clone()) {
            return Err(DashboardProjectionError::DuplicateSessionId(
                key.as_str().to_string(),
            ));
        }
        let events = sorted_events(session);
        inferred_parents.insert(
            key.clone(),
            session
                .catalog
                .parent_session_id
                .clone()
                .or_else(|| DashboardReadModel::event_parent_id(&events)),
        );
        for (parent, child) in DashboardReadModel::event_child_links(&events) {
            linked_children.entry(parent).or_default().insert(child);
        }
        rows.push(base_row(session, key, &events));
    }

    for (parent, children) in linked_children {
        for child in children {
            let child_key = SelectionKey::new(child);
            if let Some(parent_id) = inferred_parents.get_mut(&child_key) {
                if parent_id.is_none() {
                    *parent_id = Some(parent.clone());
                }
            }
        }
    }

    let known_keys = rows
        .iter()
        .map(|row| row.selection_key.clone())
        .collect::<BTreeSet<_>>();
    let mut children_by_parent = BTreeMap::<SelectionKey, Vec<SelectionKey>>::new();
    for row in &rows {
        if let Some(parent) = inferred_parents
            .get(&row.selection_key)
            .and_then(Option::as_ref)
        {
            if known_keys.contains(&SelectionKey::new(parent.clone())) {
                children_by_parent
                    .entry(SelectionKey::new(parent.clone()))
                    .or_default()
                    .push(row.selection_key.clone());
            }
        }
    }
    for children in children_by_parent.values_mut() {
        children.sort();
    }

    for row in &mut rows {
        let parent = inferred_parents
            .get(&row.selection_key)
            .cloned()
            .flatten()
            .map(SelectionKey::new);
        let is_child = parent.is_some();
        let is_parent = children_by_parent.contains_key(&row.selection_key);
        let (group, lineage_depth, parent_missing) =
            DashboardReadModel::group_for(&row.selection_key, &inferred_parents, &known_keys);
        row.relationship = DashboardRelationship {
            parent,
            children: children_by_parent
                .get(&row.selection_key)
                .cloned()
                .unwrap_or_default(),
            group,
            lineage_depth,
            parent_missing,
            is_parent,
            is_child,
            is_background: row.relationship.is_background,
            is_foreign: row.relationship.is_foreign,
        };
        row.eligibility = eligibility::evaluate(row, rules);
    }

    DashboardReadModel::sort_rows(&mut rows);
    let all_rows = rows.clone();
    let eligible_rows = rows
        .into_iter()
        .filter(|row| row.eligibility.is_eligible)
        .collect::<Vec<_>>();
    let groups = DashboardReadModel::project_groups(&all_rows);

    Ok(DashboardReadModel {
        rows: eligible_rows,
        all_rows,
        groups,
    })
}

fn map_key_error(error: SelectionKeyError) -> DashboardProjectionError {
    match error {
        SelectionKeyError::Empty => DashboardProjectionError::EmptySessionId,
    }
}

fn base_row(
    session: &DashboardSessionInput,
    key: SelectionKey,
    events: &[&EventEnvelopeV1],
) -> DashboardRow {
    let last_event = events.last();
    let creation_seq = events.first().map_or(0, |event| event.seq);
    DashboardRow {
        selection_key: key,
        title: session.catalog.run_name.clone(),
        status: derive_status(session.catalog.status, events),
        activity: DashboardActivity {
            last_event_seq: last_event.map_or(0, |event| event.seq),
            last_event_id: last_event.map(|event| event.event_id.clone()),
            unread_count: events
                .iter()
                .filter(|event| event.seq > session.read_through_seq)
                .count(),
        },
        relationship: DashboardRelationship {
            parent: None,
            children: Vec::new(),
            group: DashboardGroupKey::Root(SelectionKey::new(session.catalog.run_id.clone())),
            lineage_depth: 0,
            parent_missing: false,
            is_parent: false,
            is_child: false,
            is_background: session.is_background
                || events
                    .iter()
                    .any(|event| matches!(event.payload, EventV1::BackgroundTaskNotification(_))),
            is_foreign: session.is_foreign,
        },
        eligibility: super::eligibility::DashboardEntryEligibility {
            is_eligible: true,
            excluded_by: None,
        },
        creation_seq,
    }
}

fn sorted_events(session: &DashboardSessionInput) -> Vec<&EventEnvelopeV1> {
    let mut events = session.events.iter().collect::<Vec<_>>();
    events.sort_by(|left, right| {
        left.seq
            .cmp(&right.seq)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    events
}
