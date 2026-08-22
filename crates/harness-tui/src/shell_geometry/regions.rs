use ratatui::layout::Rect;

use crate::terminal::char_display_width;
use crate::theme_tokens::ViewportId;

use super::hit_map::HitMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellState {
    Idle,
    Drafting,
    Streaming,
    Permission,
    Question,
    Queued,
    Cancelling,
    Failed,
    Completed,
}

pub const ALL_SHELL_STATES: [ShellState; 9] = [
    ShellState::Idle,
    ShellState::Drafting,
    ShellState::Streaming,
    ShellState::Permission,
    ShellState::Question,
    ShellState::Queued,
    ShellState::Cancelling,
    ShellState::Failed,
    ShellState::Completed,
];

impl ShellState {
    pub const fn is_overlay(self) -> bool {
        matches!(self, Self::Permission | Self::Question)
    }

    pub const fn is_editable(self) -> bool {
        matches!(self, Self::Idle | Self::Drafting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellRegions {
    pub viewport: Rect,
    pub state: ShellState,
    pub top_bar: Rect,
    pub transcript_viewport: Rect,
    pub composer: Rect,
    pub status_footer: Rect,
    pub overlays: Vec<Rect>,
    pub welcome: Rect,
}

impl ShellRegions {
    pub fn contains_all_regions(&self) -> bool {
        let base = [
            self.top_bar,
            self.transcript_viewport,
            self.composer,
            self.status_footer,
            self.welcome,
        ];
        base.into_iter()
            .all(|rect| rect_is_inside(self.viewport, rect))
            && self
                .overlays
                .iter()
                .copied()
                .all(|rect| rect_is_inside(self.viewport, rect))
    }

    pub fn hit_map(&self) -> HitMap {
        HitMap::from_regions(&self)
    }

    pub const fn viewport_id(&self) -> Option<ViewportId> {
        match self.viewport.width {
            40 if self.viewport.height == 10 => Some(ViewportId::Compact40x10),
            60 if self.viewport.height == 15 => Some(ViewportId::Dense60x15),
            80 if self.viewport.height == 24 => Some(ViewportId::Default80x24),
            100 if self.viewport.height == 30 => Some(ViewportId::Standard100x30),
            132 if self.viewport.height == 40 => Some(ViewportId::Wide132x40),
            160 if self.viewport.height == 50 => Some(ViewportId::Large160x50),
            200 if self.viewport.height == 60 => Some(ViewportId::Maximum200x60),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityCopy<'a> {
    pub product: &'a str,
    pub logo: &'a str,
    pub version: &'a str,
    pub auth: &'a str,
    pub model: &'a str,
    pub workspace: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityRectangles {
    pub product: Rect,
    pub logo: Rect,
    pub version: Rect,
    pub auth: Rect,
    pub model: Rect,
    pub workspace: Rect,
}

impl IdentityRectangles {
    pub const fn height(self) -> u16 {
        self.product.height
    }
}

pub fn identity_rectangles(viewport: ViewportId, copy: &IdentityCopy<'_>) -> IdentityRectangles {
    let (width, _) = viewport.dimensions();
    let line = Rect::new(0, 0, width, 1);
    let mut x = line.x.saturating_add(2);
    let product = identity_rect(&mut x, line, copy.product);
    let logo = identity_rect(&mut x, line, copy.logo);
    let version = identity_rect(&mut x, line, copy.version);
    let auth = identity_rect(&mut x, line, copy.auth);
    let model = identity_rect(&mut x, line, copy.model);
    let workspace = identity_rect(&mut x, line, copy.workspace);
    IdentityRectangles {
        product,
        logo,
        version,
        auth,
        model,
        workspace,
    }
}

fn identity_rect(cursor: &mut u16, line: Rect, text: &str) -> Rect {
    let available = line.right().saturating_sub(*cursor);
    let text_width = display_width(text).min(usize::from(available));
    let width = u16::try_from(text_width).map_or(u16::MAX, |value| value);
    let rect = Rect::new(*cursor, line.y, width, line.height);
    *cursor = cursor.saturating_add(width).saturating_add(1);
    rect
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_display_width(character)))
        .sum()
}

fn rect_is_inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && u32::from(inner.right()) <= u32::from(outer.right())
        && u32::from(inner.bottom()) <= u32::from(outer.bottom())
}
