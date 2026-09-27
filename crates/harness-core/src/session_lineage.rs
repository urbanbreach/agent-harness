//! Branches copy validated history into a new directory; source sessions are read-only.
use crate::{
    event::*,
    proj::{RunStatus, SessionCatalogEntry},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod artifacts;
mod identities;
pub(crate) use artifacts::copy as copy_session_artifacts;
mod materialize;
pub use materialize::*;

#[derive(Debug, thiserror::Error)]
pub enum SessionLineageError {
    #[error(transparent)]
    Projection(#[from] crate::proj::ProjectionError),
    #[error(transparent)]
    Store(#[from] crate::store::EventStoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}
pub type ChildSessionMaterializationError = SessionLineageError;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableSessionPrefix {
    pub cutoff_seq: u64,
    pub event_count: usize,
    pub run_id: Option<String>,
    pub status: Option<RunStatus>,
}
pub fn validate_fork_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate_stable_prefix(events, cutoff)
}
pub fn validate_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate(events, cutoff, false)
}
pub fn validate_tui_fork_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate(events, cutoff, true)
}
fn validate(
    events: &[EventEnvelopeV1],
    cutoff: u64,
    live: bool,
) -> Result<StableSessionPrefix, SessionLineageError> {
    crate::proj::checked_history(events)?;
    let count = usize::try_from(cutoff)
        .ok()
        .filter(|n| *n <= events.len())
        .ok_or_else(|| SessionLineageError::Invalid("fork cutoff is outside the journal".into()))?;
    let mut state = crate::proj::InFlight::default();
    for event in &events[..count] {
        state.apply(event);
    }
    if !live && !state.stable() {
        return Err(SessionLineageError::Invalid(
            "session prefix still has active work or an open lifecycle".into(),
        ));
    }
    Ok(prefix(events, count, state.status))
}
pub fn latest_clone_stable_prefix(
    events: &[EventEnvelopeV1],
) -> Result<StableSessionPrefix, SessionLineageError> {
    crate::proj::checked_history(events)?;
    let mut state = crate::proj::InFlight::default();
    let mut latest = events.is_empty().then(|| prefix(events, 0, None));
    for (index, event) in events.iter().enumerate() {
        state.apply(event);
        if state.stable() {
            latest = Some(prefix(events, index + 1, state.status));
        }
    }
    latest.ok_or_else(|| {
        SessionLineageError::Invalid("no completed stable prefix is available".into())
    })
}
fn prefix(
    events: &[EventEnvelopeV1],
    count: usize,
    status: Option<RunStatus>,
) -> StableSessionPrefix {
    StableSessionPrefix {
        cutoff_seq: count as u64,
        event_count: count,
        run_id: events
            .first()
            .filter(|_| count > 0)
            .map(|e| e.run_id.to_string()),
        status: status.filter(|s| *s != RunStatus::Running),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineageTree {
    pub roots: Vec<SessionLineageNode>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineageNode {
    pub entry: SessionCatalogEntry,
    pub children: Vec<SessionLineageNode>,
}

pub fn project_lineage_tree(
    entries: impl IntoIterator<Item = SessionCatalogEntry>,
) -> SessionLineageTree {
    let mut entries: BTreeMap<_, _> = entries.into_iter().map(|e| (e.run_id.clone(), e)).collect();
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut roots = Vec::new();
    for (id, entry) in &entries {
        if let Some(parent) = entry
            .parent_session_id
            .as_ref()
            .filter(|p| *p != id && entries.contains_key(*p))
        {
            edges.entry(parent.clone()).or_default().push(id.clone());
        } else {
            roots.push(id.clone());
        }
    }
    // Remove entries as they are visited: orphan roots and cycles each appear exactly once.
    let mut forest = SessionLineageTree::default();
    for root in roots
        .into_iter()
        .chain(entries.keys().cloned().collect::<Vec<_>>())
    {
        let Some(entry) = entries.remove(&root) else {
            continue;
        };
        let mut stack = vec![(
            SessionLineageNode {
                entry,
                children: Vec::new(),
            },
            edges.remove(&root).unwrap_or_default().into_iter(),
        )];
        while let Some((_, children)) = stack.last_mut() {
            if let Some(id) = children.next() {
                if let Some(entry) = entries.remove(&id) {
                    stack.push((
                        SessionLineageNode {
                            entry,
                            children: Vec::new(),
                        },
                        edges.remove(&id).unwrap_or_default().into_iter(),
                    ));
                }
            } else if let Some((mut node, _)) = stack.pop() {
                sort(&mut node.children);
                if let Some((parent, _)) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    forest.roots.push(node);
                }
            }
        }
    }
    sort(&mut forest.roots);
    forest
}
fn sort(nodes: &mut [SessionLineageNode]) {
    nodes.sort_by(|a, b| {
        b.entry
            .last_updated_at
            .cmp(&a.entry.last_updated_at)
            .then_with(|| a.entry.run_id.cmp(&b.entry.run_id))
    });
}
