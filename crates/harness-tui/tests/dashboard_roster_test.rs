use std::collections::{BTreeMap, BTreeSet};

use harness_tui::dashboard::{
    DashboardActivity, DashboardEntryEligibility, DashboardGroup, DashboardGroupKey,
    DashboardReadModel, DashboardRelationship, DashboardRow, DashboardStatus, SelectionKey,
};
use harness_tui::dashboard_roster::{
    layout_for_rect, layout_for_viewport, responsive_for_rect, status_marker, truncate_label,
    LineageFilter, RosterFilter, RosterHitMap, RosterHitTarget, RosterState, ViewportId,
};
use ratatui::layout::Rect;

fn row(
    id: &str,
    title: &str,
    status: DashboardStatus,
    depth: usize,
    parent: Option<&str>,
) -> DashboardRow {
    let group = parent.map_or(id, |parent| parent);
    let is_parent = id == "parent";
    DashboardRow {
        selection_key: SelectionKey::new(id),
        title: Some(title.to_string()),
        status,
        activity: DashboardActivity {
            last_event_seq: 1,
            last_event_id: Some(format!("event-{id}")),
            unread_count: 0,
        },
        relationship: DashboardRelationship {
            parent: parent.map(SelectionKey::new),
            children: Vec::new(),
            group: DashboardGroupKey::Root(SelectionKey::new(group)),
            lineage_depth: depth,
            parent_missing: false,
            is_parent,
            is_child: parent.is_some(),
            is_background: false,
            is_foreign: false,
        },
        eligibility: DashboardEntryEligibility {
            is_eligible: true,
            excluded_by: None,
        },
        creation_seq: 1,
    }
}

fn model() -> DashboardReadModel {
    let rows = [
        ("parent", "Build parent", DashboardStatus::Running, 0, None),
        (
            "child",
            "Stream child",
            DashboardStatus::Streaming,
            1,
            Some("parent"),
        ),
        (
            "queued",
            "Queued task",
            DashboardStatus::Queued,
            1,
            Some("parent"),
        ),
        (
            "done",
            "Finished task",
            DashboardStatus::Completed,
            1,
            Some("parent"),
        ),
        (
            "failed",
            "Failed task",
            DashboardStatus::Failed,
            1,
            Some("parent"),
        ),
        (
            "cancelled",
            "Cancelled task",
            DashboardStatus::Cancelled,
            1,
            Some("parent"),
        ),
        (
            "stale",
            "Stale task",
            DashboardStatus::Stale,
            1,
            Some("parent"),
        ),
        ("other", "Other root", DashboardStatus::Completed, 0, None),
    ]
    .into_iter()
    .map(|(id, title, status, depth, parent)| row(id, title, status, depth, parent))
    .collect::<Vec<_>>();
    let mut grouped = BTreeMap::<DashboardGroupKey, Vec<SelectionKey>>::new();
    for item in &rows {
        grouped
            .entry(item.relationship.group.clone())
            .or_default()
            .push(item.selection_key.clone());
    }
    let groups = grouped
        .into_iter()
        .map(|(key, row_keys)| DashboardGroup { key, row_keys })
        .collect();
    DashboardReadModel {
        all_rows: rows.clone(),
        rows,
        groups,
    }
}

#[test]
fn status_markers_cover_every_dashboard_status() {
    for status in [
        DashboardStatus::Running,
        DashboardStatus::Queued,
        DashboardStatus::Streaming,
        DashboardStatus::Completed,
        DashboardStatus::Failed,
        DashboardStatus::Cancelled,
        DashboardStatus::Stale,
    ] {
        let marker = status_marker(status);
        assert_eq!(marker.status, status);
        assert!(!marker.preferred.is_empty());
        assert!(!marker.ascii.is_empty());
    }
}

#[test]
fn filter_matches_name_status_and_lineage_without_reordering_ids() {
    let source = model();
    let by_name = RosterFilter::new().with_query("stream child");
    assert_eq!(
        by_name.matching_keys(&source),
        vec![SelectionKey::new("child")]
    );
    let by_status = RosterFilter::new().with_status(DashboardStatus::Failed);
    assert_eq!(
        by_status.matching_keys(&source),
        vec![SelectionKey::new("failed")]
    );
    let by_lineage = RosterFilter::new().with_lineage(LineageFilter::Child);
    assert_eq!(by_lineage.matching_keys(&source).len(), 6);
}

#[test]
fn pinned_rows_lead_their_group_and_fold_preserves_stable_selection() {
    let source = model();
    let mut state = RosterState::default();
    state.toggle_pin(SelectionKey::new("stale"));
    state.set_selected(Some(SelectionKey::new("stale")));
    let expanded = layout_for_rect(Rect::new(0, 0, 80, 24), &source, &state);
    let parent_group = DashboardGroupKey::Root(SelectionKey::new("parent"));
    assert_eq!(
        expanded
            .rows
            .iter()
            .find(|row| row.group == parent_group)
            .map(|row| row.selection_key.as_str()),
        Some("stale")
    );
    state.toggle_fold(DashboardGroupKey::Root(SelectionKey::new("parent")));
    let folded = layout_for_rect(Rect::new(0, 0, 80, 24), &source, &state);
    assert!(folded
        .rows
        .iter()
        .all(|row| row.group != DashboardGroupKey::Root(SelectionKey::new("parent"))));
    assert_eq!(
        state.selected_key().map(SelectionKey::as_str),
        Some("stale")
    );
}

#[test]
fn narrow_layout_condenses_markers_and_clips_wide_labels() {
    let responsive = responsive_for_rect(Rect::new(0, 0, 40, 10));
    assert!(responsive.is_narrow() && responsive.condensed_markers());
    assert_eq!(truncate_label("成功 session", 5), "成功…");
    let layout = layout_for_rect(Rect::new(0, 0, 40, 10), &model(), &RosterState::default());
    assert!(layout.rows.iter().all(|row| row.marker_text.len() <= 1));
}

#[test]
fn overflow_and_hit_map_cover_every_rendered_roster_region() {
    let state = RosterState {
        scroll_top: 2,
        ..RosterState::default()
    };
    let layout = layout_for_rect(Rect::new(0, 0, 32, 5), &model(), &state);
    assert!(!layout.overflow.is_empty());
    let hit_map = RosterHitMap::from_layout(&layout);
    assert!(hit_map.regions.iter().any(|region| {
        matches!(region.target, RosterHitTarget::Row(ref key) if key.as_str() == "child")
    }));
    for region in &hit_map.regions {
        assert_eq!(
            hit_map.hit_test(region.rect.x, region.rect.y),
            Some(region.target.clone())
        );
        assert_eq!(
            hit_map.hit_test(region.rect.right().saturating_sub(1), region.rect.y),
            Some(region.target.clone())
        );
        assert_eq!(hit_map.hit_test(region.rect.right(), region.rect.y), None);
    }
}

#[test]
fn viewport_matrix_preserves_group_filter_pin_and_hit_contracts() {
    let source = model();
    let filters = [
        RosterFilter::default(),
        RosterFilter::new().with_status(DashboardStatus::Completed),
        RosterFilter::new().with_lineage(LineageFilter::Child),
    ];
    for filter in filters {
        for viewport in ViewportId::ALL {
            for folded in [false, true] {
                let folded_groups = if folded {
                    BTreeSet::from([DashboardGroupKey::Root(SelectionKey::new("parent"))])
                } else {
                    BTreeSet::new()
                };
                let state = RosterState {
                    filter: filter.clone(),
                    folded_groups,
                    pinned_rows: BTreeSet::from([SelectionKey::new("stale")]),
                    ..RosterState::default()
                };
                let layout = layout_for_viewport(viewport, &source, &state);
                let hit_map = RosterHitMap::from_layout(&layout);
                assert!(hit_map.regions.iter().all(|region| {
                    hit_map.hit_test(region.rect.x, region.rect.y) == Some(region.target.clone())
                }));
                let Some(row) = layout.rows.first() else {
                    continue;
                };
                let mut selection = state.clone();
                selection.select_at(&hit_map, row.rect.x, row.rect.y);
                selection.hover_at(&hit_map, row.rect.x, row.rect.y);
                assert_eq!(selection.selected_key(), Some(&row.selection_key));
                assert_eq!(selection.hovered_key(), Some(&row.selection_key));
            }
        }
    }
}
