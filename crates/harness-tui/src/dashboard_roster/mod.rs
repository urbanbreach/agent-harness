#![allow(
    clippy::module_name_repetitions,
    reason = "task 31 exposes stable roster-prefixed presentation types"
)]

use ratatui::layout::Rect;

use crate::dashboard::{DashboardGroupKey, DashboardStatus, SelectionKey};
use crate::design_contract::{ColorRole, GlyphRole};

pub use responsive::RosterResponsive;

pub const ROSTER_ROW_HEIGHT: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowDirection {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusMarker {
    pub status: DashboardStatus,
    pub glyph: GlyphRole,
    pub color: ColorRole,
    pub preferred: &'static str,
    pub ascii: &'static str,
}

impl StatusMarker {
    pub const fn label(self) -> &'static str {
        match self.status {
            DashboardStatus::Running => "running",
            DashboardStatus::Queued => "queued",
            DashboardStatus::Streaming => "streaming",
            DashboardStatus::Completed => "completed",
            DashboardStatus::Failed => "failed",
            DashboardStatus::Cancelled => "cancelled",
            DashboardStatus::Stale => "stale",
        }
    }

    pub const fn condensed(self) -> &'static str {
        self.ascii
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterGroupLayout {
    pub group: DashboardGroupKey,
    pub rect: Rect,
    pub label: String,
    pub folded: bool,
    pub row_keys: Vec<SelectionKey>,
    pub visible_row_keys: Vec<SelectionKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRowLayout {
    pub selection_key: SelectionKey,
    pub group: DashboardGroupKey,
    pub rect: Rect,
    pub label: String,
    pub marker: StatusMarker,
    pub marker_text: String,
    pub lineage_depth: usize,
    pub indent: u16,
    pub pinned: bool,
    pub selected: bool,
    pub hovered: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterOverflowIndicator {
    pub direction: OverflowDirection,
    pub rect: Rect,
    pub hidden_rows: usize,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterItem {
    Group(RosterGroupLayout),
    Row(RosterRowLayout),
    Overflow(RosterOverflowIndicator),
}

impl RosterItem {
    pub const fn rect(&self) -> Rect {
        match self {
            Self::Group(item) => item.rect,
            Self::Row(item) => item.rect,
            Self::Overflow(item) => item.rect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterLayout {
    pub viewport: Rect,
    pub content: Rect,
    pub responsive: RosterResponsive,
    pub rows: Vec<RosterRowLayout>,
    pub groups: Vec<RosterGroupLayout>,
    pub overflow: Vec<RosterOverflowIndicator>,
    pub items: Vec<RosterItem>,
    pub scroll_top: usize,
    pub max_scroll: usize,
}

impl RosterLayout {
    pub fn row(&self, key: &SelectionKey) -> Option<&RosterRowLayout> {
        self.rows.iter().find(|row| row.selection_key == *key)
    }

    pub fn visible_selection_keys(&self) -> Vec<SelectionKey> {
        self.rows
            .iter()
            .map(|row| row.selection_key.clone())
            .collect()
    }
}

pub mod filter;
pub mod hit_map;
pub mod layout;
pub mod responsive;

pub use crate::design_contract::ViewportId;
pub use filter::{
    FilteredRoster, FilteredRosterGroup, LineageFilter, RosterFilter, RosterState, filter_model,
};
pub use hit_map::{RosterHitMap, RosterHitRegion, RosterHitTarget};
pub use layout::{layout_for_rect, layout_for_viewport, status_marker};
pub use responsive::{label_width, responsive_for, responsive_for_rect, truncate_label};
