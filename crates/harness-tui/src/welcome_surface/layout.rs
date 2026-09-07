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
    pub action_rects: [(u16, u16, u16, u16); 4],
    pub changelog_header_rect: Option<(u16, u16, u16, u16)>,
    pub identity_rect: (u16, u16, u16, u16),
    pub notes_rect: (u16, u16, u16, u16),
    pub notices_rect: (u16, u16, u16, u16),
    pub compact: bool,
    pub menu_items_visible: usize,
}

impl WelcomeLayout {
    pub fn compute(width: u16, height: u16) -> Self {
        Self::for_area((0, 0, width, height), false)
    }

    pub(crate) fn for_startup_area(
        area: (u16, u16, u16, u16),
        clipboard_warning_visible: bool,
        expanded: bool,
    ) -> Self {
        Self::for_area_state(area, clipboard_warning_visible, expanded)
    }

    pub fn for_area(
        (origin_x, origin_y, width, height): (u16, u16, u16, u16),
        clipboard_warning_visible: bool,
    ) -> Self {
        Self::for_area_state(
            (origin_x, origin_y, width, height),
            clipboard_warning_visible,
            true,
        )
    }

    fn for_area_state(
        area: (u16, u16, u16, u16),
        _clipboard_warning_visible: bool,
        _expanded: bool,
    ) -> Self {
        Self::with_content(area, &crate::release_notes::CURRENT, "", true)
    }

    pub(crate) fn with_content(
        (x, y, width, height): (u16, u16, u16, u16),
        notes: &[&str],
        notice: &str,
        glyphs: bool,
    ) -> Self {
        let full_logo = crate::startup_logo::full_logo(glyphs);
        let full_width = full_logo.map_or(0, |logo| cells(logo.width()));
        let panel_width = width.saturating_sub(6).min(120);
        let copy_width = panel_width.saturating_sub(4 + full_width + 3);
        let measure = |text: &str, columns: u16| -> u16 {
            if text.is_empty() {
                0
            } else {
                cells(crate::ui::wrap_completion_text(text, usize::from(columns.max(1))).len())
            }
        };
        let note_rows = notes
            .iter()
            .map(|note| measure(&format!("• {note}"), copy_width))
            .fold(0u16, u16::saturating_add);
        let notice_rows = measure(notice, copy_width);
        let copy_height =
            1 + 2 + note_rows + 2 + 4 + if notice_rows > 0 { notice_rows + 1 } else { 0 };
        let panel_height = copy_height.max(full_logo.map_or(0, |logo| cells(logo.height()))) + 2;
        let wide = width >= 90 && copy_width >= 60 && panel_height <= height.saturating_sub(3);
        let (
            content,
            panel,
            logo,
            identity,
            notes_rect,
            action_y,
            action_x,
            action_width,
            notices_rect,
        ) = if wide {
            let panel_x = x + (width - panel_width) / 2;
            let panel_y = y + 3 + height.saturating_sub(3 + panel_height) / 3;
            let content = (
                panel_x + 2,
                panel_y + 1,
                panel_width.saturating_sub(4),
                panel_height.saturating_sub(2),
            );
            let copy_x = content.0 + full_width + 3;
            let action_y = content.1 + 3 + note_rows + 2;
            (
                content,
                Some((panel_x, panel_y, panel_width, panel_height)),
                (
                    content.0,
                    content.1,
                    full_width,
                    full_logo.map_or(0, |logo| cells(logo.height())),
                ),
                (copy_x, content.1, copy_width, 1),
                (copy_x, content.1 + 3, copy_width, note_rows),
                action_y,
                copy_x,
                copy_width,
                (copy_x, action_y + 5, copy_width, notice_rows),
            )
        } else {
            let columns = width.saturating_sub(4).min(60);
            let content_x = x + width.saturating_sub(columns) / 2;
            let available = height.saturating_sub(3);
            let stacked_note_rows = notes
                .iter()
                .map(|note| measure(&format!("• {note}"), columns))
                .fold(0u16, u16::saturating_add);
            let stacked_notice_rows = measure(notice, columns);
            let copy_rows = 8
                + stacked_note_rows
                + if stacked_notice_rows > 0 {
                    stacked_notice_rows + 1
                } else {
                    0
                };
            let logo = crate::startup_logo::for_height(height.saturating_add(5), glyphs)
                .filter(|logo| cells(logo.height()) + 1 + copy_rows <= available);
            let logo_height = logo.map_or(0, |logo| cells(logo.height()));
            let logo_width = logo.map_or(0, |logo| cells(logo.width()));
            let identity_y = y + 3 + logo_height + u16::from(logo.is_some());
            let action_y = identity_y + 2;
            let notes_y = action_y + 6;
            let note_rows = notes
                .iter()
                .map(|note| measure(&format!("• {note}"), columns))
                .fold(0u16, u16::saturating_add);
            let notice_reserve = if stacked_notice_rows > 0 {
                stacked_notice_rows + 1
            } else {
                0
            };
            let note_rows = if notes_y + note_rows + notice_reserve <= y + height {
                note_rows
            } else {
                0
            };
            let notice_y = if note_rows > 0 {
                notes_y + note_rows + 1
            } else {
                action_y + 5
            };
            let notice_rows = measure(notice, columns).min((y + height).saturating_sub(notice_y));
            (
                (content_x, y + 3, columns, available),
                None,
                (
                    content_x + columns.saturating_sub(logo_width) / 2,
                    y + 3,
                    logo_width,
                    logo_height,
                ),
                (content_x, identity_y, columns, 1),
                (content_x, notes_y, columns, note_rows),
                action_y,
                content_x,
                columns,
                (content_x, notice_y, columns, notice_rows),
            )
        };
        let clamp = |rect| bound(rect, x + width, y + height);
        let action_rects =
            [0, 1, 2, 3].map(|row| clamp((action_x, action_y + row, action_width, 1)));
        let menu_items_visible = action_rects
            .iter()
            .filter(|rect| rect.2 > 0 && rect.3 > 0)
            .count();
        let notes_rect = clamp(notes_rect);
        Self {
            width,
            height,
            hero_rect: clamp(identity),
            logo_rect: clamp(logo),
            menu_rect: clamp((action_x, action_y, action_width, 4)),
            prompt_rect: (x, y + height, 0, 0),
            status_rect: (x, y + height, 0, 0),
            panel_rect: panel,
            content_rect: clamp(content),
            action_rects,
            changelog_header_rect: (notes_rect.3 > 0).then(|| {
                clamp((
                    notes_rect.0,
                    notes_rect.1.saturating_sub(1),
                    notes_rect.2,
                    1,
                ))
            }),
            identity_rect: clamp(identity),
            notes_rect,
            notices_rect: clamp(notices_rect),
            compact: !wide,
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

fn cells(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}
