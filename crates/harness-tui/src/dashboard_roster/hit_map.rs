use ratatui::layout::Rect;

use crate::dashboard::{DashboardGroupKey, SelectionKey};

use super::layout::{OverflowDirection, RosterItem, RosterLayout};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterHitTarget {
    Row(SelectionKey),
    Group(DashboardGroupKey),
    Overflow(OverflowDirection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterHitRegion {
    pub target: RosterHitTarget,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RosterHitMap {
    pub regions: Vec<RosterHitRegion>,
}

impl RosterHitMap {
    pub fn from_layout(layout: &RosterLayout) -> Self {
        let regions = layout
            .items
            .iter()
            .map(|item| match item {
                RosterItem::Group(group) => RosterHitRegion {
                    target: RosterHitTarget::Group(group.group.clone()),
                    rect: group.rect,
                },
                RosterItem::Row(row) => RosterHitRegion {
                    target: RosterHitTarget::Row(row.selection_key.clone()),
                    rect: row.rect,
                },
                RosterItem::Overflow(indicator) => RosterHitRegion {
                    target: RosterHitTarget::Overflow(indicator.direction),
                    rect: indicator.rect,
                },
            })
            .filter(|region| region.rect.width > 0 && region.rect.height > 0)
            .collect();
        Self { regions }
    }

    pub fn hit_test(&self, x: u16, y: u16) -> Option<RosterHitTarget> {
        self.region_at(x, y).map(|region| region.target.clone())
    }

    pub fn region_at(&self, x: u16, y: u16) -> Option<&RosterHitRegion> {
        self.regions
            .iter()
            .find(|region| contains(region.rect, x, y))
    }

    pub fn selection_at(&self, x: u16, y: u16) -> Option<SelectionKey> {
        match self.hit_test(x, y) {
            Some(RosterHitTarget::Row(key)) => Some(key),
            Some(RosterHitTarget::Group(_)) | Some(RosterHitTarget::Overflow(_)) | None => None,
        }
    }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && x >= rect.x
        && y >= rect.y
        && x < rect.right()
        && y < rect.bottom()
}
