use ratatui::layout::Rect;

use crate::dashboard::{DashboardReadModel, DashboardRow, DashboardStatus};
use crate::theme_tokens::{ColorRole, GlyphRole, ViewportId, DESIGN_TOKENS};

use super::filter::{filter_model, RosterState};
use super::responsive::{
    group_label, label_width, overflow_indicator, responsive_for_rect, truncate_label,
    RosterResponsive,
};
pub use super::{
    OverflowDirection, RosterGroupLayout, RosterItem, RosterLayout, RosterOverflowIndicator,
    RosterRowLayout, StatusMarker, ROSTER_ROW_HEIGHT,
};

pub fn status_marker(status: DashboardStatus) -> StatusMarker {
    let (glyph, color, ascii) = match status {
        DashboardStatus::AwaitingInput => (GlyphRole::Queued, ColorRole::StatusWarning, "?"),
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
                &format!(
                    "{} {}",
                    group_label(model, &group.key, group.folded, responsive),
                    group
                        .rows
                        .iter()
                        .filter(|row| !row.relationship.is_child)
                        .count()
                ),
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
    let capacity = content.height;
    let total_height = logical
        .iter()
        .fold(0_u16, |height, item| height.saturating_add(item.height()));
    let overflowing = total_height > capacity;
    let mut max_scroll = 0;
    let mut remaining = total_height;
    while remaining > capacity.saturating_sub(1) && max_scroll + 1 < logical.len() {
        remaining = remaining.saturating_sub(logical[max_scroll].height());
        max_scroll += 1;
    }
    if !overflowing {
        max_scroll = 0;
    }
    let scroll_top = state.scroll_top.min(max_scroll);
    let mut items = Vec::new();
    let mut overflow = Vec::new();
    let mut cursor_y = content.y;
    if scroll_top > 0 && capacity > 0 {
        let indicator = overflow_indicator(
            OverflowDirection::Top,
            Rect::new(content.x, cursor_y, content.width, 1),
            scroll_top,
            responsive,
        );
        cursor_y = cursor_y.saturating_add(1);
        items.push(RosterItem::Overflow(indicator.clone()));
        overflow.push(indicator);
    }
    let mut rows = Vec::new();
    let mut end = scroll_top;
    for (index, logical_item) in logical.iter().enumerate().skip(scroll_top) {
        let reserve = u16::from(index + 1 < logical.len());
        let available = content
            .bottom()
            .saturating_sub(cursor_y)
            .saturating_sub(reserve);
        if available < logical_item.height() {
            break;
        }
        match logical_item {
            LogicalItem::Group(index) => {
                let mut header = group_layouts[*index].clone();
                header.rect = Rect::new(content.x, cursor_y, content.width, 2);
                group_layouts[*index] = header.clone();
                items.push(RosterItem::Group(header));
            }
            LogicalItem::Row(row) => {
                let mut rendered = row_layout(row, content, cursor_y, responsive, state);
                rendered.group = super::filter::presentation_group(model, row);
                items.push(RosterItem::Row(rendered.clone()));
                rows.push(rendered);
            }
        }
        cursor_y = cursor_y.saturating_add(logical_item.height());
        end = index + 1;
    }
    if end < logical.len() && capacity > 0 {
        let indicator = overflow_indicator(
            OverflowDirection::Bottom,
            Rect::new(
                content.x,
                content.bottom().saturating_sub(1),
                content.width,
                1,
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

impl LogicalItem {
    const fn height(&self) -> u16 {
        match self {
            Self::Group(_) => 2,
            Self::Row(_) => ROSTER_ROW_HEIGHT,
        }
    }
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
    let indent = responsive
        .indent_width()
        .saturating_mul(u16::try_from(row.relationship.lineage_depth).unwrap_or(u16::MAX));
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
