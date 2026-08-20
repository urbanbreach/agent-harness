use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::AppState;
use crate::overlay::OverlayKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalViewKey {
    Primary,
    SessionHistory,
    SessionRename,
    ModelSwitcher,
    Toggles,
    YoloConfirm,
    Lineage,
    ForkSelector,
    PlanPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalSurfaceKey {
    Overlay {
        kind: OverlayKind,
        view: ModalViewKey,
    },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalAction {
    Activate,
    Cancel,
    Resubmit,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalTarget {
    Close,
    Input,
    Row(usize),
    Scrollbar,
    Footer(ModalAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalPressLocation {
    Target(ModalTarget),
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModalScrollbarPress {
    owner: ModalSurfaceKey,
    anchor_row: u16,
    anchor_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalPress {
    Target {
        owner: ModalSurfaceKey,
        location: ModalPressLocation,
    },
    Scrollbar(ModalScrollbarPress),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ModalInteractionState {
    owner: Option<ModalSurfaceKey>,
    hovered: Option<ModalTarget>,
    pressed: Option<ModalPress>,
    visual_offset: usize,
}

impl ModalInteractionState {
    fn bind(&mut self, owner: ModalSurfaceKey, visual_offset: usize) -> bool {
        if self.owner == Some(owner) {
            return false;
        }
        self.owner = Some(owner);
        self.hovered = None;
        self.pressed = None;
        self.visual_offset = visual_offset;
        true
    }

    pub(crate) fn invalidate(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn hovered(&self, owner: ModalSurfaceKey) -> Option<ModalTarget> {
        (self.owner == Some(owner))
            .then_some(self.hovered)
            .flatten()
    }

    pub(crate) fn visual_offset(
        &self,
        owner: ModalSurfaceKey,
        default: usize,
        max: usize,
    ) -> usize {
        if self.owner == Some(owner) {
            self.visual_offset.min(max)
        } else {
            default.min(max)
        }
    }
}

impl AppState {
    pub(crate) fn modal_visual_offset(
        &self,
        owner: ModalSurfaceKey,
        default: usize,
        max: usize,
    ) -> usize {
        self.modal_interaction.visual_offset(owner, default, max)
    }

    pub(crate) fn modal_target_hovered(&self, owner: ModalSurfaceKey, target: ModalTarget) -> bool {
        self.modal_interaction.hovered(owner) == Some(target)
    }

    pub(crate) fn handle_top_modal_mouse(
        &mut self,
        mouse: MouseEvent,
        frame_area: ratatui::layout::Rect,
    ) -> Option<bool> {
        let model = crate::ui::ui_overlays::modal_surface_model(self, frame_area)?;
        let owner_changed = self.modal_interaction.bind(model.key, model.visual_offset);
        let target = model.hit(mouse.column, mouse.row);

        if mouse.modifiers.contains(KeyModifiers::CONTROL) {
            let press_invalidated = self.modal_interaction.pressed.take().is_some();
            return Some(owner_changed || press_invalidated);
        }

        match mouse.kind {
            MouseEventKind::Moved => {
                let hover_changed = self.modal_interaction.hovered != target;
                self.modal_interaction.hovered = target;
                let selection_changed = match (model.key, target) {
                    (ModalSurfaceKey::Help, _) => false,
                    (_, Some(ModalTarget::Row(index))) => self.select_modal_row(model.key, index),
                    (_, Some(ModalTarget::Close | ModalTarget::Input | ModalTarget::Scrollbar))
                    | (_, Some(ModalTarget::Footer(_)))
                    | (_, None) => false,
                };
                Some(owner_changed || hover_changed || selection_changed)
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if target.is_none()
                    && !model.contains(mouse.column, mouse.row)
                    && model.outside_dismiss_policy
                        == crate::ui::ui_overlays::OutsideDismissPolicy::ImmediateDown
                {
                    self.dismiss_modal(model.key);
                    return Some(true);
                }
                self.modal_interaction.pressed = match target {
                    Some(ModalTarget::Scrollbar) => {
                        Some(ModalPress::Scrollbar(ModalScrollbarPress {
                            owner: model.key,
                            anchor_row: mouse.row,
                            anchor_offset: model.visual_offset,
                        }))
                    }
                    Some(target) => Some(ModalPress::Target {
                        owner: model.key,
                        location: ModalPressLocation::Target(target),
                    }),
                    None if !model.contains(mouse.column, mouse.row) => Some(ModalPress::Target {
                        owner: model.key,
                        location: ModalPressLocation::Outside,
                    }),
                    None => None,
                };
                Some(true)
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let pressed = self.modal_interaction.pressed.take();
                let released = if model.contains(mouse.column, mouse.row) {
                    target.map(ModalPressLocation::Target)
                } else {
                    Some(ModalPressLocation::Outside)
                };
                let activated = match (pressed, released) {
                    (Some(ModalPress::Target { owner, location }), Some(released_location))
                        if owner == model.key && location == released_location =>
                    {
                        self.activate_modal_location(model.key, location)
                    }
                    (Some(ModalPress::Target { .. } | ModalPress::Scrollbar(_)), _) | (None, _) => {
                        false
                    }
                };
                Some(pressed.is_some() || activated)
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(pressed) = self.modal_interaction.pressed else {
                    return Some(owner_changed);
                };
                match pressed {
                    ModalPress::Target { .. } => {
                        self.modal_interaction.pressed = None;
                        Some(true)
                    }
                    ModalPress::Scrollbar(press) => {
                        let Some(scrollbar) = model.scrollbar else {
                            self.modal_interaction.pressed = None;
                            return Some(true);
                        };
                        if press.owner != model.key {
                            self.modal_interaction.pressed = None;
                            return Some(true);
                        }
                        let previous = self.modal_interaction.visual_offset;
                        self.modal_interaction.visual_offset = scrollbar.drag_offset(
                            press.anchor_row,
                            press.anchor_offset,
                            mouse.row,
                            model.max_scroll,
                        );
                        self.modal_interaction.hovered = None;
                        Some(owner_changed || self.modal_interaction.visual_offset != previous)
                    }
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let press_invalidated = self.modal_interaction.pressed.take().is_some();
                if !model.contains(mouse.column, mouse.row) {
                    return Some(owner_changed || press_invalidated);
                }
                if model.key == ModalSurfaceKey::Help {
                    let changed = self.help_browser.scroll_by(
                        mouse.kind == MouseEventKind::ScrollDown,
                        model.visual_offset,
                        model.max_scroll,
                    );
                    self.modal_interaction.hovered = None;
                    return Some(owner_changed || press_invalidated || changed);
                }
                let previous = self.modal_interaction.visual_offset;
                self.modal_interaction.visual_offset = match mouse.kind {
                    MouseEventKind::ScrollDown => previous.saturating_add(3).min(model.max_scroll),
                    MouseEventKind::ScrollUp => previous.saturating_sub(3),
                    MouseEventKind::Down(_)
                    | MouseEventKind::Up(_)
                    | MouseEventKind::Drag(_)
                    | MouseEventKind::Moved
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight => previous,
                };
                self.modal_interaction.hovered = None;
                Some(
                    owner_changed
                        || press_invalidated
                        || self.modal_interaction.visual_offset != previous,
                )
            }
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                let press_invalidated = self.modal_interaction.pressed.take().is_some();
                Some(owner_changed || press_invalidated)
            }
        }
    }

    fn activate_modal_location(
        &mut self,
        owner: ModalSurfaceKey,
        location: ModalPressLocation,
    ) -> bool {
        match location {
            ModalPressLocation::Outside => {
                self.dismiss_modal(owner);
                true
            }
            ModalPressLocation::Target(ModalTarget::Close)
            | ModalPressLocation::Target(ModalTarget::Footer(ModalAction::Cancel)) => {
                self.dismiss_modal(owner);
                true
            }
            ModalPressLocation::Target(ModalTarget::Row(index)) => {
                self.select_modal_row(owner, index);
                self.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                true
            }
            ModalPressLocation::Target(ModalTarget::Footer(action)) => {
                let key = match action {
                    ModalAction::Activate => KeyCode::Enter,
                    ModalAction::Cancel => KeyCode::Esc,
                    ModalAction::Resubmit | ModalAction::Reset => KeyCode::Char('r'),
                };
                self.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
                true
            }
            ModalPressLocation::Target(ModalTarget::Input) if owner == ModalSurfaceKey::Help => {
                self.help_browser.activate_search()
            }
            ModalPressLocation::Target(ModalTarget::Input | ModalTarget::Scrollbar) => false,
        }
    }

    fn dismiss_modal(&mut self, owner: ModalSurfaceKey) {
        if owner
            == (ModalSurfaceKey::Overlay {
                kind: OverlayKind::ReleaseNotes,
                view: ModalViewKey::Primary,
            })
        {
            self.close_release_notes();
        } else if owner == ModalSurfaceKey::Help {
            self.close_review_surface();
        } else {
            self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        self.modal_interaction.invalidate();
    }

    fn select_modal_row(&mut self, owner: ModalSurfaceKey, index: usize) -> bool {
        let previous = match owner {
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::CommandPalette,
                view: ModalViewKey::Primary,
            } => std::mem::replace(&mut self.palette_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::CommandPalette,
                view: ModalViewKey::SessionHistory,
            } => std::mem::replace(&mut self.session_history_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::CommandPalette,
                view: ModalViewKey::ModelSwitcher,
            } => std::mem::replace(&mut self.model_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::TogglesMenu,
                view: ModalViewKey::Toggles,
            } => std::mem::replace(&mut self.toggles_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::ThemeDialog,
                ..
            } => std::mem::replace(&mut self.theme_dialog_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::PromptStashList,
                ..
            } => std::mem::replace(&mut self.prompt_stash.list_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::SettingsEditor,
                ..
            } => std::mem::replace(&mut self.settings_editor_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::PlanView,
                view: ModalViewKey::Primary,
            } => std::mem::replace(&mut self.plan_view_selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::MemoryBrowser,
                ..
            } => std::mem::replace(&mut self.memory_browser.selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::WorktreePicker,
                ..
            } => std::mem::replace(&mut self.worktree_picker.selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::ForeignImportPicker,
                ..
            } => std::mem::replace(&mut self.foreign_import_picker.selected, index),
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::LineageBrowser,
                ..
            } => {
                let current = self
                    .lineage_browser_view_model()
                    .rows
                    .iter()
                    .position(|row| row.selected)
                    .unwrap_or(0);
                if current == index {
                    current
                } else {
                    self.lineage_browser
                        .move_selection(selection_delta(current, index));
                    current
                }
            }
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::ForkSelector,
                ..
            } => {
                let current = self
                    .fork_selector_view_model()
                    .rows
                    .iter()
                    .position(|row| row.selected)
                    .unwrap_or(0);
                if current == index {
                    current
                } else {
                    self.fork_selector
                        .move_selection(selection_delta(current, index));
                    current
                }
            }
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::SubagentActions,
                ..
            } => return index == 0,
            ModalSurfaceKey::Help => {
                let rows = self.help_rows();
                return self.help_browser.select(index, &rows);
            }
            ModalSurfaceKey::Overlay { .. } => return false,
        };
        previous != index
    }

    #[cfg(test)]
    pub(crate) fn modal_close_hovered_for_test(&self) -> bool {
        matches!(self.modal_interaction.hovered, Some(ModalTarget::Close))
    }
}

fn selection_delta(current: usize, target: usize) -> isize {
    if target >= current {
        isize::try_from(target - current).unwrap_or(isize::MAX)
    } else {
        -isize::try_from(current - target).unwrap_or(isize::MAX)
    }
}
