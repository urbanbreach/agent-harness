use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::proj::SessionCatalogEntry;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionLineageTree {
    pub roots: Vec<SessionLineageNode>,
}

impl SessionLineageTree {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.roots.iter().map(SessionLineageNode::subtree_len).sum()
    }

    pub fn flatten(&self) -> Vec<SessionLineageRow<'_>> {
        let mut rows = Vec::new();
        for root in &self.roots {
            root.flatten_into(0, &mut rows);
        }
        rows
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineageNode {
    pub entry: SessionCatalogEntry,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SessionLineageNode>,
}

impl SessionLineageNode {
    fn subtree_len(&self) -> usize {
        1 + self.children.iter().map(Self::subtree_len).sum::<usize>()
    }

    fn flatten_into<'a>(&'a self, depth: usize, rows: &mut Vec<SessionLineageRow<'a>>) {
        rows.push(SessionLineageRow {
            depth,
            entry: &self.entry,
        });
        for child in &self.children {
            child.flatten_into(depth + 1, rows);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLineageRow<'a> {
    pub depth: usize,
    pub entry: &'a SessionCatalogEntry,
}

/// Project a deterministic, read-only lineage tree over session catalog entries.
///
/// Entries whose `parent_session_id` is missing, blank, unknown, self-referential, or cyclic are
/// treated as roots so legacy or partially migrated catalogs remain browseable.
pub fn project_lineage_tree(
    entries: impl IntoIterator<Item = SessionCatalogEntry>,
) -> SessionLineageTree {
    let entries = entries
        .into_iter()
        .map(|entry| (entry.run_id.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let parent_by_id = entries
        .iter()
        .map(|(run_id, entry)| {
            (
                run_id.clone(),
                normalized_parent_id(entry.parent_session_id.as_deref()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut children_by_parent = BTreeMap::<String, Vec<String>>::new();
    let mut roots = Vec::<String>::new();

    for run_id in sorted_ids(&entries) {
        let parent_id = parent_by_id
            .get(&run_id)
            .and_then(|parent_id| parent_id.as_deref());

        match parent_id {
            Some(parent_id)
                if entries.contains_key(parent_id)
                    && parent_id != run_id
                    && !parent_chain_reaches(&parent_by_id, parent_id, &run_id) =>
            {
                children_by_parent
                    .entry(parent_id.to_string())
                    .or_default()
                    .push(run_id);
            }
            _ => roots.push(run_id),
        }
    }

    for children in children_by_parent.values_mut() {
        sort_ids_by_entry_order(&entries, children);
    }
    sort_ids_by_entry_order(&entries, &mut roots);

    SessionLineageTree {
        roots: roots
            .into_iter()
            .map(|run_id| build_node(&entries, &children_by_parent, &run_id))
            .collect(),
    }
}

fn normalized_parent_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parent_chain_reaches(
    parent_by_id: &BTreeMap<String, Option<String>>,
    start_parent_id: &str,
    target_id: &str,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut current = Some(start_parent_id);

    while let Some(run_id) = current {
        if run_id == target_id {
            return true;
        }
        if !seen.insert(run_id.to_string()) {
            return false;
        }
        current = parent_by_id
            .get(run_id)
            .and_then(|parent| parent.as_deref());
    }

    false
}

fn sorted_ids(entries: &BTreeMap<String, SessionCatalogEntry>) -> Vec<String> {
    let mut ids = entries.keys().cloned().collect::<Vec<_>>();
    sort_ids_by_entry_order(entries, &mut ids);
    ids
}

fn sort_ids_by_entry_order(entries: &BTreeMap<String, SessionCatalogEntry>, ids: &mut [String]) {
    ids.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));
}

fn compare_entries(left: &SessionCatalogEntry, right: &SessionCatalogEntry) -> Ordering {
    match (&left.last_updated_at, &right.last_updated_at) {
        (Some(left_updated), Some(right_updated)) => right_updated
            .cmp(left_updated)
            .then_with(|| left.run_id.cmp(&right.run_id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.run_id.cmp(&right.run_id),
    }
}

fn build_node(
    entries: &BTreeMap<String, SessionCatalogEntry>,
    children_by_parent: &BTreeMap<String, Vec<String>>,
    run_id: &str,
) -> SessionLineageNode {
    SessionLineageNode {
        entry: entries[run_id].clone(),
        children: children_by_parent
            .get(run_id)
            .into_iter()
            .flatten()
            .map(|child_id| build_node(entries, children_by_parent, child_id))
            .collect(),
    }
}
