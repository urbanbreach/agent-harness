use ratatui::layout::Rect;

use crate::transcript_identity::TurnId;

use super::geometry::TimelineGeometry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineHitRegion {
    pub turn_id: TurnId,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimelineHitMap {
    pub regions: Vec<TimelineHitRegion>,
}

impl TimelineHitMap {
    pub fn from_geometry(geometry: &TimelineGeometry) -> Self {
        Self {
            regions: geometry
                .marker_rects
                .iter()
                .map(|marker| TimelineHitRegion {
                    turn_id: marker.turn_id,
                    rect: marker.rect,
                })
                .collect(),
        }
    }

    pub fn hit_test(&self, x: u16, y: u16) -> Option<TurnId> {
        self.regions
            .iter()
            .find(|region| contains(region.rect, x, y))
            .map(|region| region.turn_id)
    }

    pub fn region_at(&self, x: u16, y: u16) -> Option<TimelineHitRegion> {
        self.regions
            .iter()
            .copied()
            .find(|region| contains(region.rect, x, y))
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
