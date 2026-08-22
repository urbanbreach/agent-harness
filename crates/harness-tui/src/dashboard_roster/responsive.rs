use ratatui::layout::Rect;

use crate::dashboard::{DashboardGroupKey, DashboardReadModel};
use crate::terminal::char_display_width;
use crate::theme_tokens::{ViewportId, DESIGN_TOKENS};

use super::{OverflowDirection, RosterOverflowIndicator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterResponsive {
    narrow: bool,
    ultra_compact: bool,
    padding_x: u16,
    padding_y: u16,
    indent_width: u16,
    marker_width: u16,
}

impl RosterResponsive {
    pub const fn is_narrow(self) -> bool {
        self.narrow
    }

    pub const fn is_ultra_compact(self) -> bool {
        self.ultra_compact
    }

    pub const fn condensed_markers(self) -> bool {
        self.narrow
    }

    pub const fn padding_x(self) -> u16 {
        self.padding_x
    }

    pub const fn padding_y(self) -> u16 {
        self.padding_y
    }

    pub const fn indent_width(self) -> u16 {
        self.indent_width
    }

    pub const fn marker_width(self) -> u16 {
        self.marker_width
    }

    pub fn truncate(self, label: &str, max_width: u16) -> String {
        truncate_label(label, max_width)
    }
}

pub fn responsive_for(viewport: ViewportId) -> RosterResponsive {
    let (width, _) = viewport.dimensions();
    responsive_for_width(width)
}

pub fn responsive_for_rect(viewport: Rect) -> RosterResponsive {
    responsive_for_width(viewport.width)
}

pub fn label_width(label: &str) -> usize {
    label
        .chars()
        .map(|character| usize::from(char_display_width(character)))
        .sum()
}

pub fn truncate_label(label: &str, max_width: u16) -> String {
    let budget = usize::from(max_width);
    if budget == 0 {
        return String::new();
    }
    if label_width(label) <= budget {
        return label.to_string();
    }
    if budget == 1 {
        return "…".to_string();
    }

    let mut output = String::new();
    let mut width: usize = 0;
    for character in label.chars() {
        let character_width = usize::from(char_display_width(character));
        if width.saturating_add(character_width).saturating_add(1) > budget {
            break;
        }
        output.push(character);
        width = width.saturating_add(character_width);
    }
    output.push('…');
    output
}

fn responsive_for_width(width: u16) -> RosterResponsive {
    let spacing = DESIGN_TOKENS.spacing;
    let ultra_compact = width <= 40;
    let narrow = width < 80;
    RosterResponsive {
        narrow,
        ultra_compact,
        padding_x: if ultra_compact {
            spacing.unit
        } else {
            spacing.sidebar_padding_x
        },
        padding_y: if ultra_compact {
            0
        } else {
            spacing.sidebar_padding_y
        },
        indent_width: if narrow {
            spacing.unit
        } else {
            spacing.unit.saturating_mul(2)
        },
        marker_width: if narrow { 1 } else { 2 },
    }
}

pub(super) fn group_label(
    model: &DashboardReadModel,
    key: &DashboardGroupKey,
    folded: bool,
    responsive: RosterResponsive,
) -> String {
    let name = match key {
        DashboardGroupKey::Root(root) => model
            .row(root.as_str())
            .and_then(|row| row.title.as_deref())
            .map_or(root.as_str(), |title| title),
        DashboardGroupKey::Orphaned(root) => root.as_str(),
    };
    let marker = if responsive.condensed_markers() {
        if folded {
            "+ "
        } else {
            "- "
        }
    } else if folded {
        "▸ "
    } else {
        "▾ "
    };
    format!("{marker}{name}")
}

pub(super) fn overflow_indicator(
    direction: OverflowDirection,
    rect: Rect,
    hidden_rows: usize,
    responsive: RosterResponsive,
) -> RosterOverflowIndicator {
    let arrow = match (direction, responsive.condensed_markers()) {
        (OverflowDirection::Top, true) => "^",
        (OverflowDirection::Bottom, true) => "v",
        (OverflowDirection::Top, false) => "↑",
        (OverflowDirection::Bottom, false) => "↓",
    };
    RosterOverflowIndicator {
        direction,
        rect,
        hidden_rows,
        label: format!("{arrow} {hidden_rows} more"),
    }
}
