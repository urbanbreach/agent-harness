use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::interaction_reducer::{mouse_intent, MouseTarget, UiIntent};
use crate::app::Focus;
use crate::composer_atoms::AttachmentId;
use crate::design_contract::ViewportId;
use crate::shell_geometry::{layout_for, FocusTarget, HitMap, HitTarget};

use super::slice::ComposerSlice;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComposerHitTarget {
    Shell(HitTarget),
    Completion(usize),
    Attachment(AttachmentId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerHitRegion {
    pub target: ComposerHitTarget,
    pub rect: Rect,
    pub z_order: u16,
    pub focus_target: FocusTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerHitMap {
    pub shell: HitMap,
    pub composer_rect: Rect,
    pub regions: Vec<ComposerHitRegion>,
}

impl ComposerHitMap {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<ComposerHitTarget> {
        self.regions
            .iter()
            .filter(|region| contains(region.rect, x, y))
            .max_by_key(|region| region.z_order)
            .map(|region| region.target)
    }

    pub fn intent_for(&self, event: MouseEvent) -> Option<UiIntent> {
        let target = self.hit_test(event.column, event.row)?;
        let focus = match target {
            ComposerHitTarget::Shell(HitTarget::TopBar) => Focus::List,
            ComposerHitTarget::Shell(HitTarget::Transcript) => Focus::Details,
            ComposerHitTarget::Shell(HitTarget::Composer)
            | ComposerHitTarget::Completion(_)
            | ComposerHitTarget::Attachment(_) => Focus::Prompt,
            ComposerHitTarget::Shell(HitTarget::StatusFooter) => Focus::Details,
            ComposerHitTarget::Shell(HitTarget::Overlay) => Focus::Details,
            ComposerHitTarget::Shell(HitTarget::Welcome) => Focus::List,
        };
        let target = match event.kind {
            MouseEventKind::Down(_) => MouseTarget::Focus(focus),
            MouseEventKind::Up(_) => MouseTarget::Focus(focus),
            MouseEventKind::Drag(_) => MouseTarget::Focus(focus),
            MouseEventKind::ScrollUp => MouseTarget::ScrollUp,
            MouseEventKind::ScrollDown => MouseTarget::ScrollDown,
            MouseEventKind::Moved | MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                return None
            }
        };
        mouse_intent(event, target)
    }
}

pub(super) fn build(slice: &ComposerSlice, viewport: ViewportId) -> ComposerHitMap {
    let shell_state = slice.shell_state();
    let shell = layout_for(viewport, shell_state).hit_map();
    let view = slice.view_model(viewport);
    let mut regions = shell
        .regions
        .iter()
        .map(|region| ComposerHitRegion {
            target: ComposerHitTarget::Shell(region.target),
            rect: region.rect,
            z_order: region.z_order,
            focus_target: region.focus_target,
        })
        .collect::<Vec<_>>();
    if let Some(completion) = &view.completion {
        for index in 0..completion.geometry.rect.height as usize {
            let offset = u16::try_from(index).unwrap_or(u16::MAX);
            let y = completion.geometry.rect.y.saturating_add(offset);
            regions.push(ComposerHitRegion {
                target: ComposerHitTarget::Completion(index),
                rect: Rect::new(
                    completion.geometry.rect.x,
                    y,
                    completion.geometry.rect.width,
                    1,
                ),
                z_order: 200,
                focus_target: FocusTarget::Prompt,
            });
        }
    }
    for attachment in &view.attachments {
        regions.push(ComposerHitRegion {
            target: ComposerHitTarget::Attachment(attachment.attachment_id),
            rect: attachment.rect,
            z_order: 150,
            focus_target: FocusTarget::Prompt,
        });
    }
    ComposerHitMap {
        shell,
        composer_rect: view.composer,
        regions,
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
