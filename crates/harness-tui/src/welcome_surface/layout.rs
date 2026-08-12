const TWO_COLUMN_MIN_WIDTH: u16 = 90;
const WIDE_ACTION_MARKER_OFFSET: u16 = 17;
const WIDE_PANEL_MIN_HEIGHT: u16 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WelcomeRegion {
    Hero,
    Logo,
    Menu,
    Prompt,
    StatusBar,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeLayout {
    pub width: u16,
    pub height: u16,
    pub hero_rect: (u16, u16, u16, u16),
    pub logo_rect: (u16, u16, u16, u16),
    pub menu_rect: (u16, u16, u16, u16),
    pub prompt_rect: (u16, u16, u16, u16),
    pub status_rect: (u16, u16, u16, u16),
    pub panel_rect: Option<(u16, u16, u16, u16)>,
    pub content_rect: (u16, u16, u16, u16),
    pub action_rects: [(u16, u16, u16, u16); 3],
    pub compact: bool,
    pub menu_items_visible: usize,
}

impl WelcomeLayout {
    pub fn compute(width: u16, height: u16) -> Self {
        Self::for_area((0, 0, width, height), false)
    }

    pub fn for_area(
        (origin_x, origin_y, width, height): (u16, u16, u16, u16),
        clipboard_warning_visible: bool,
    ) -> Self {
        let wide = width >= TWO_COLUMN_MIN_WIDTH;
        let menu_items_visible = 3;
        let terminal_height = height.saturating_add(5);
        let base_top: u16 = if wide && terminal_height <= 30 { 3 } else { 4 };
        let top = base_top
            .saturating_add(terminal_height.saturating_sub(30) / 3)
            .saturating_add(u16::from(clipboard_warning_visible) * 3)
            .min(height.saturating_sub(8));
        let compact = !wide || height.saturating_sub(top) < WIDE_PANEL_MIN_HEIGHT;
        let panel_width = width.saturating_sub(6).clamp(20, 120);
        let panel_height = 16.min(height.saturating_sub(top));
        let panel_x = origin_x.saturating_add(width.saturating_sub(panel_width) / 2);
        let panel_y = origin_y.saturating_add(top);
        let panel_rect =
            (!compact && panel_height > 0).then_some((panel_x, panel_y, panel_width, panel_height));
        let content = if let Some(panel) = panel_rect {
            (
                panel.0.saturating_add(2),
                panel.1.saturating_add(1),
                panel.2.saturating_sub(4),
                panel.3.saturating_sub(2),
            )
        } else {
            let inset = if width <= 51 {
                5.min(width.saturating_sub(8) / 2)
            } else {
                width
                    .saturating_sub(51)
                    .div_ceil(2)
                    .min(width.saturating_sub(8) / 2)
            };
            let content_width = 51.min(width.saturating_sub(inset));
            let compact_top = if width <= 60 { 8 } else { 7 }.min(height.saturating_sub(4));
            (
                origin_x.saturating_add(inset),
                origin_y.saturating_add(compact_top),
                content_width,
                height.saturating_sub(compact_top).max(1),
            )
        };
        let action_start = if panel_rect.is_some() {
            content.1.saturating_add(9)
        } else {
            content.1
        };
        let action_x = if panel_rect.is_some() {
            content
                .0
                .saturating_add(WIDE_ACTION_MARKER_OFFSET.min(content.2))
        } else {
            content.0
        };
        let action_width = content.0.saturating_add(content.2).saturating_sub(action_x);
        let action_rects = [0, 1, 2].map(|offset| {
            bound(
                (
                    action_x,
                    action_start.saturating_add(offset),
                    action_width,
                    1,
                ),
                origin_x.saturating_add(width),
                origin_y.saturating_add(height),
            )
        });
        let hero = (content.0, content.1, content.2, 1.min(content.3));
        let logo = (
            content.0,
            content.1.saturating_add(1),
            15.min(content.2),
            7.min(content.3),
        );
        let menu = (
            content.0,
            action_start.saturating_sub(1),
            content.2,
            5.min(content.3),
        );
        let prompt = (
            origin_x.saturating_add(2.min(width)),
            origin_y.saturating_add(height.saturating_sub(3)),
            width.saturating_sub(4),
            3.min(height),
        );
        let status = (
            origin_x,
            origin_y.saturating_add(height.saturating_sub(1)),
            width,
            height.min(1),
        );

        Self {
            width,
            height,
            hero_rect: bound(
                hero,
                origin_x.saturating_add(width),
                origin_y.saturating_add(height),
            ),
            logo_rect: bound(
                logo,
                origin_x.saturating_add(width),
                origin_y.saturating_add(height),
            ),
            menu_rect: bound(
                menu,
                origin_x.saturating_add(width),
                origin_y.saturating_add(height),
            ),
            prompt_rect: bound(
                prompt,
                origin_x.saturating_add(width),
                origin_y.saturating_add(height),
            ),
            status_rect: bound(
                status,
                origin_x.saturating_add(width),
                origin_y.saturating_add(height),
            ),
            panel_rect,
            content_rect: content,
            action_rects,
            compact,
            menu_items_visible,
        }
    }

    pub fn region_at(&self, col: u16, row: u16) -> WelcomeRegion {
        self.all_regions()
            .into_iter()
            .find(|(_, rect)| contains(*rect, col, row))
            .map_or(WelcomeRegion::None, |(region, _)| region)
    }

    pub fn all_regions(&self) -> [(WelcomeRegion, (u16, u16, u16, u16)); 5] {
        [
            (WelcomeRegion::Hero, self.hero_rect),
            (WelcomeRegion::Logo, self.logo_rect),
            (WelcomeRegion::Menu, self.menu_rect),
            (WelcomeRegion::StatusBar, self.status_rect),
            (WelcomeRegion::Prompt, self.prompt_rect),
        ]
    }
}

fn bound((x, y, w, h): (u16, u16, u16, u16), width: u16, height: u16) -> (u16, u16, u16, u16) {
    let x = x.min(width);
    let y = y.min(height);
    (
        x,
        y,
        w.min(width.saturating_sub(x)),
        h.min(height.saturating_sub(y)),
    )
}

fn contains((x, y, width, height): (u16, u16, u16, u16), col: u16, row: u16) -> bool {
    col >= x && col < x.saturating_add(width) && row >= y && row < y.saturating_add(height)
}
