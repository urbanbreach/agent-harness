use ratatui::layout::Rect;

use crate::dashboard::{DashboardReadModel, DashboardRow, DashboardStatus};
use crate::design_contract::{ColorRole, DESIGN_TOKENS, GlyphRole, ViewportId};

use super::filter::{RosterState, filter_model};
use super::responsive::{
    RosterResponsive, group_label, label_width, overflow_indicator, responsive_for_rect,
    truncate_label,
};
pub use super::{
    OverflowDirection, ROSTER_ROW_HEIGHT, RosterGroupLayout, RosterItem, RosterLayout,
    RosterOverflowIndicator, RosterRowLayout, StatusMarker,
};

pub fn status_marker(status: DashboardStatus) -> StatusMarker {
    let (glyph, color, ascii) = match status {
        DashboardStatus::Running => (GlyphRole::Running, ColorRole::StatusInfo, "o"),
        DashboardStatus::Queued => (GlyphRole::Queued, ColorRole::TextSecondary, "."),
        DashboardStatus::Streaming => (GlyphRole::Streaming, ColorRole::StatusInfo, "o"),
        DashboardStatus::Completed => (GlyphRole::Succeeded, ColorRole::StatusSuccess, "*"),
        DashboardStatus::Failed => (GlyphRole::Failed, ColorRole::StatusError, "x"),
        DashboardStatus::Cancelled => (GlyphRole::Error, ColorRole::StatusWarning, "!"),
        DashboardStatus::Stale => (GlyphRole::Error, ColorRole::StatusWarning, "!"),
    };
    let preferred = DESIGN_TOKENS
        .glyph_roles
        .all
        .iter()
        .find(|token| token.role == glyph)
        .map_or(ascii, |token| token.preferred);
    StatusMarker {
        status,
        glyph,
        color,
        preferred,
        ascii,
    }
}

pub fn layout_for_viewport(
    viewport: ViewportId,
    model: &DashboardReadModel,
    state: &RosterState,
) -> RosterLayout {
    let (width, height) = viewport.dimensions();
    layout_for_rect(Rect::new(0, 0, width, height), model, state)
}

pub fn layout_for_rect(
    viewport: Rect,
    model: &DashboardReadModel,
    state: &RosterState,
) -> RosterLayout {
    let responsive = responsive_for_rect(viewport);
    let content = content_rect(viewport, responsive);
    let mut groups = filter_model(model, &state.filter).groups;
    for group in &mut groups {
        group.folded = state.is_folded(&group.key);
        group
            .rows
            .sort_by_key(|row| !state.is_pinned(&row.selection_key));
    }
    let mut group_layouts = groups
        .iter()
        .map(|group| RosterGroupLayout {
            group: group.key.clone(),
            rect: Rect::default(),
            label: truncate_label(
                &group_label(model, &group.key, group.folded, responsive),
                content.width,
            ),
            folded: group.folded,
            row_keys: group
                .rows
                .iter()
                .map(|row| row.selection_key.clone())
                .collect(),
            visible_row_keys: if group.folded {
                Vec::new()
            } else {
                group
                    .rows
                    .iter()
                    .map(|row| row.selection_key.clone())
                    .collect()
            },
        })
        .collect::<Vec<_>>();
    let mut logical = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        logical.push(LogicalItem::Group(index));
        if !group.folded {
            logical.extend(group.rows.iter().cloned().map(LogicalItem::Row));
        }
    }
    let capacity = usize::from(content.height);
    let overflowing = logical.len() > capacity;
    let max_scroll = if overflowing {
        logical.len().saturating_sub(capacity.saturating_sub(1))
    } else {
        0
    };
    let scroll_top = state.scroll_top.min(max_scroll);
    let top_overflow = overflowing && scroll_top > 0 && capacity > 0;
    let top_units = if top_overflow { 1 } else { 0 };
    let bottom_possible_end = scroll_top.saturating_add(capacity.saturating_sub(top_units));
    let bottom_overflow =
        overflowing && bottom_possible_end < logical.len() && capacity > top_units;
    let bottom_units = if bottom_overflow { 1 } else { 0 };
    let available = capacity
        .saturating_sub(top_units)
        .saturating_sub(bottom_units);
    let end = logical.len().min(scroll_top.saturating_add(available));
    let mut items = Vec::new();
    let mut overflow = Vec::new();
    let mut cursor_y = content.y;
    if top_overflow {
        let indicator = overflow_indicator(
            OverflowDirection::Top,
            Rect::new(content.x, cursor_y, content.width, ROSTER_ROW_HEIGHT),
            scroll_top,
            responsive,
        );
        cursor_y = cursor_y.saturating_add(ROSTER_ROW_HEIGHT);
        items.push(RosterItem::Overflow(indicator.clone()));
        overflow.push(indicator);
    }
    let mut rows = Vec::new();
    for logical_item in &logical[scroll_top..end] {
        match logical_item {
            LogicalItem::Group(index) => {
                let mut header = group_layouts[*index].clone();
                header.rect = Rect::new(content.x, cursor_y, content.width, ROSTER_ROW_HEIGHT);
                group_layouts[*index] = header.clone();
                cursor_y = cursor_y.saturating_add(ROSTER_ROW_HEIGHT);
                items.push(RosterItem::Group(header));
            }
            LogicalItem::Row(row) => {
                let rendered = row_layout(row, content, cursor_y, responsive, state);
                cursor_y = cursor_y.saturating_add(ROSTER_ROW_HEIGHT);
                items.push(RosterItem::Row(rendered.clone()));
                rows.push(rendered);
            }
        }
    }
    if bottom_overflow {
        let indicator = overflow_indicator(
            OverflowDirection::Bottom,
            Rect::new(
                content.x,
                content.bottom().saturating_sub(ROSTER_ROW_HEIGHT),
                content.width,
                ROSTER_ROW_HEIGHT,
            ),
            logical.len().saturating_sub(end),
            responsive,
        );
        items.push(RosterItem::Overflow(indicator.clone()));
        overflow.push(indicator);
    }
    RosterLayout {
        viewport,
        content,
        responsive,
        rows,
        groups: group_layouts,
        overflow,
        items,
        scroll_top,
        max_scroll,
    }
}

#[derive(Debug, Clone)]
enum LogicalItem {
    Group(usize),
    Row(DashboardRow),
}

fn content_rect(viewport: Rect, responsive: RosterResponsive) -> Rect {
    let horizontal = responsive.padding_x().min(viewport.width / 2);
    let vertical = responsive.padding_y().min(viewport.height / 2);
    Rect::new(
        viewport.x.saturating_add(horizontal),
        viewport.y.saturating_add(vertical),
        viewport.width.saturating_sub(horizontal.saturating_mul(2)),
        viewport.height.saturating_sub(vertical.saturating_mul(2)),
    )
}

fn row_layout(
    row: &DashboardRow,
    content: Rect,
    y: u16,
    responsive: RosterResponsive,
    state: &RosterState,
) -> RosterRowLayout {
    let marker = status_marker(row.status);
    let marker_text = if responsive.condensed_markers() {
        marker.condensed().to_string()
    } else {
        marker.preferred.to_string()
    };
    let indent = responsive.indent_width().saturating_mul(
        u16::try_from(row.relationship.lineage_depth).map_or(u16::MAX, |value| value),
    );
    let pin_width = if state.is_pinned(&row.selection_key) {
        2
    } else {
        0
    };
    let reserved = responsive
        .marker_width()
        .saturating_add(indent)
        .saturating_add(pin_width)
        .saturating_add(2);
    let budget = content.width.saturating_sub(reserved);
    let source = row
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map_or(row.selection_key.as_str(), |title| title);
    let label = truncate_label(source, budget);
    RosterRowLayout {
        selection_key: row.selection_key.clone(),
        group: row.relationship.group.clone(),
        rect: Rect::new(content.x, y, content.width, ROSTER_ROW_HEIGHT),
        label,
        marker,
        marker_text,
        lineage_depth: row.relationship.lineage_depth,
        indent,
        pinned: state.is_pinned(&row.selection_key),
        selected: state.selected_key() == Some(&row.selection_key),
        hovered: state.hovered_key() == Some(&row.selection_key),
        truncated: label_width(source) > usize::from(budget),
    }
}
