use super::layout::{WelcomeLayout, WelcomeRegion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WelcomeHit {
    pub region: WelcomeRegion,
    pub item_index: Option<usize>,
}

pub struct WelcomeHitMap {
    layout: WelcomeLayout,
    menu_item_rects: Vec<(usize, (u16, u16, u16, u16))>,
}

impl WelcomeHitMap {
    pub fn new(layout: WelcomeLayout, menu_labels: &[&str]) -> Self {
        let item_width = layout.menu_rect.2.saturating_sub(2);
        let menu_item_rects = menu_labels
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let y = layout
                    .menu_rect
                    .1
                    .saturating_add(1)
                    .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
                (
                    index,
                    (layout.menu_rect.0.saturating_add(1), y, item_width, 1),
                )
            })
            .collect();
        Self {
            layout,
            menu_item_rects,
        }
    }

    pub fn hit(&self, col: u16, row: u16) -> Option<WelcomeHit> {
        self.menu_item_rects
            .iter()
            .rev()
            .find_map(|(index, rect)| {
                contains(*rect, col, row).then_some(WelcomeHit {
                    region: WelcomeRegion::Menu,
                    item_index: Some(*index),
                })
            })
            .or_else(|| match self.layout.region_at(col, row) {
                WelcomeRegion::None => None,
                region => Some(WelcomeHit {
                    region,
                    item_index: None,
                }),
            })
    }

    pub fn layout(&self) -> &WelcomeLayout {
        &self.layout
    }
}

fn contains((x, y, width, height): (u16, u16, u16, u16), col: u16, row: u16) -> bool {
    col >= x && col < x.saturating_add(width) && row >= y && row < y.saturating_add(height)
}
