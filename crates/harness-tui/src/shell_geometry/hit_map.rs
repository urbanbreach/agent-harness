use ratatui::layout::Rect;

use super::regions::ShellRegions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitTarget {
    TopBar,
    Transcript,
    Composer,
    StatusFooter,
    Overlay,
    Welcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusTarget {
    Shell,
    Scrollback,
    Prompt,
    Status,
    Modal,
    Welcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitRegionState {
    Active,
    Covered,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitRegion {
    pub target: HitTarget,
    pub rect: Rect,
    pub z_order: u16,
    pub focus_target: FocusTarget,
    pub active: bool,
    pub covered: bool,
}

impl HitRegion {
    pub const fn state(self) -> HitRegionState {
        if self.covered {
            HitRegionState::Covered
        } else if self.active {
            HitRegionState::Active
        } else {
            HitRegionState::Inactive
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitMap {
    pub regions: Vec<HitRegion>,
}

impl HitMap {
    pub(crate) fn from_regions(regions: &ShellRegions) -> Self {
        let overlay = match regions.overlays.first() {
            Some(rect) => *rect,
            None => Rect::default(),
        };
        let overlay_active = !is_empty(overlay);
        let entries = [
            (HitTarget::TopBar, regions.top_bar, 10, FocusTarget::Shell),
            (
                HitTarget::Transcript,
                regions.transcript_viewport,
                20,
                FocusTarget::Scrollback,
            ),
            (
                HitTarget::Welcome,
                regions.welcome,
                25,
                FocusTarget::Welcome,
            ),
            (
                HitTarget::Composer,
                regions.composer,
                30,
                FocusTarget::Prompt,
            ),
            (
                HitTarget::StatusFooter,
                regions.status_footer,
                40,
                FocusTarget::Status,
            ),
            (HitTarget::Overlay, overlay, 100, FocusTarget::Modal),
        ];
        let regions = entries
            .into_iter()
            .map(|(target, rect, z_order, focus_target)| {
                let covered = overlay_active && target != HitTarget::Overlay;
                let active = if target == HitTarget::Overlay {
                    overlay_active
                } else {
                    !is_empty(rect) && !covered
                };
                HitRegion {
                    target,
                    rect,
                    z_order,
                    focus_target,
                    active,
                    covered,
                }
            })
            .collect();
        Self { regions }
    }

    pub fn hit_test(&self, x: u16, y: u16) -> Option<HitTarget> {
        self.hit_region_at(x, y).map(|region| region.target)
    }

    pub fn hit_region_at(&self, x: u16, y: u16) -> Option<&HitRegion> {
        self.regions
            .iter()
            .filter(|region| region.active && contains(region.rect, x, y))
            .max_by_key(|region| region.z_order)
    }
}

fn is_empty(rect: Rect) -> bool {
    rect.width == 0 || rect.height == 0
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    !is_empty(rect) && x >= rect.x && y >= rect.y && x < rect.right() && y < rect.bottom()
}
