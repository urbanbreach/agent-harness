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
    pub compact: bool,
    pub menu_items_visible: usize,
}

impl WelcomeLayout {
    pub fn compute(width: u16, height: u16) -> Self {
        let compact = width < 80 || height < 24;
        let menu_items_visible = if compact { 3 } else { 5 };
        let hero_height = 1;
        let logo_height = if compact { 2 } else { 3 };
        let menu_height = u16::try_from(menu_items_visible)
            .unwrap_or(u16::MAX)
            .saturating_add(2);
        let prompt_height = if compact { 1 } else { 3 };
        let hero_y = if compact { 0 } else { 1 };
        let hero = centered(width, hero_y, width.saturating_sub(4).min(60), hero_height);
        let logo = centered(
            width,
            hero.1.saturating_add(hero.3),
            width.saturating_sub(4).min(40),
            logo_height,
        );
        let menu = centered(
            width,
            logo.1.saturating_add(logo.3),
            width.saturating_sub(4).min(50),
            menu_height,
        );
        let prompt_y = menu.1.saturating_add(menu.3);
        let prompt = (
            2.min(width),
            prompt_y,
            width.saturating_sub(4),
            prompt_height,
        );
        let status = (0, height.saturating_sub(1), width, height.min(1));

        Self {
            width,
            height,
            hero_rect: bound(hero, width, height),
            logo_rect: bound(logo, width, height),
            menu_rect: bound(menu, width, height),
            prompt_rect: bound(prompt, width, height),
            status_rect: bound(status, width, height),
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
            (WelcomeRegion::Prompt, self.prompt_rect),
            (WelcomeRegion::StatusBar, self.status_rect),
        ]
    }
}

fn centered(width: u16, y: u16, region_width: u16, height: u16) -> (u16, u16, u16, u16) {
    (
        width.saturating_sub(region_width) / 2,
        y,
        region_width,
        height,
    )
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
