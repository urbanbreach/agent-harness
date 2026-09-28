
/// Fixed header/footer rows around the remaining body; the footer wins at tiny heights.
/// Preserve raw horizontal fields, as vertical Layout does for saturated rectangles.
pub(super) fn split_rows(area: Rect, top: u16, bottom: u16) -> [Rect; 3] {
    let height = area.bottom().saturating_sub(area.y);
    let bottom = bottom.min(height);
    let top = top.min(height.saturating_sub(bottom));
    let middle = height.saturating_sub(top).saturating_sub(bottom);
    [
        Rect { height: top, ..area },
        Rect { y: area.y.saturating_add(top), height: middle, ..area },
        Rect { y: area.bottom().saturating_sub(bottom), height: bottom, ..area },
    ]
}

pub(super) fn reserve_sidebar_gap(dock: ControlDockLayout, gap: u16) -> ControlDockLayout {
    let shrink = |area: Rect| Rect { width: area.width.saturating_sub(gap), ..area };
    ControlDockLayout { status: dock.status.map(shrink), composer: shrink(dock.composer), disclosure: dock.disclosure.map(shrink), ..dock }
}

pub(super) fn inset_dock(dock: ControlDockLayout, area: Rect, startup: bool) -> ControlDockLayout {
    if dock.shell.width == 0 || dock.shell.height == 0 { return dock; }
    let inset = if startup && area.width <= super::DENSE_SESSION_MAX_WIDTH { 0 } else { super::composer_horizontal_inset(area.width) };
    if !startup && inset == 0 { return dock; }
    let width = if startup { area.width } else { dock.shell.width }.saturating_sub(inset.saturating_mul(2)).max(1);
    let x = if startup { area.x } else { dock.shell.x }.saturating_add(inset);
    let y = area.y.saturating_add(area.height.saturating_sub(dock.shell.height));
    let map = |band: Rect| if startup {
        Rect::new(x, y.saturating_add(band.y.saturating_sub(dock.shell.y)), width, band.height)
    } else { Rect { x, width, ..band } };
    ControlDockLayout { shell: map(dock.shell), status: dock.status.map(map), composer: map(dock.composer), disclosure: dock.disclosure.map(map) }
}
