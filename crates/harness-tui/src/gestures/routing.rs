use ratatui::layout::Rect;

/// A coordinate in terminal-cell space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

impl Point {
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

/// A semantic pointer destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    None,
    Transcript,
    Terminal,
    Inspector,
    Composer,
    Overlay,
}

/// One surface in top-to-bottom z-order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitSurface {
    pub target: HitTarget,
    pub rect: Rect,
    pub active: bool,
    pub covered: bool,
}

impl HitSurface {
    pub const fn new(target: HitTarget, rect: Rect, active: bool, covered: bool) -> Self {
        Self {
            target,
            rect,
            active,
            covered,
        }
    }
}

/// Routes to the first eligible surface, stopping at an inactive/covered hit rectangle.
pub fn route_hit_target(point: Point, surfaces: &[HitSurface]) -> HitTarget {
    for surface in surfaces {
        if !contains(surface.rect, point) {
            continue;
        }
        if !surface.active || surface.covered {
            return HitTarget::None;
        }
        return surface.target;
    }
    HitTarget::None
}

fn contains(rect: Rect, point: Point) -> bool {
    let right = u32::from(rect.x) + u32::from(rect.width);
    let bottom = u32::from(rect.y) + u32::from(rect.height);
    u32::from(point.x) >= u32::from(rect.x)
        && u32::from(point.x) < right
        && u32::from(point.y) >= u32::from(rect.y)
        && u32::from(point.y) < bottom
}
