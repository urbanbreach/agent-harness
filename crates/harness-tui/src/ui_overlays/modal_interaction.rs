use super::*;
use crate::app::{ModalSurfaceKey, ModalTarget, ModalViewKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalHitRegion {
    pub(crate) target: ModalTarget,
    pub(crate) area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModalSurfaceModel {
    pub(crate) key: ModalSurfaceKey,
    pub(crate) popup: Rect,
    pub(crate) regions: Vec<ModalHitRegion>,
    pub(crate) visual_offset: usize,
    pub(crate) max_scroll: usize,
}

impl ModalSurfaceModel {
    pub(crate) fn hit(&self, column: u16, row: u16) -> Option<ModalTarget> {
        self.regions
            .iter()
            .find(|region| rect_contains(region.area, column, row))
            .map(|region| region.target)
    }

    pub(crate) fn contains(&self, column: u16, row: u16) -> bool {
        rect_contains(self.popup, column, row)
    }
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

pub(crate) fn modal_surface_model(app: &AppState, frame_area: Rect) -> Option<ModalSurfaceModel> {
    let plan = crate::layout::FrameLayoutPlan::for_app(app, frame_area);
    let Some(overlay) = app.overlay_stack().top() else {
        return app
            .help_is_open()
            .then(|| help_model(app, plan.root, plan.composer))
            .flatten();
    };
    match overlay {
        OverlayKind::CommandPalette if app.session_history_visible => {
            session_history_model(app, plan.palette_overlay?)
        }
        OverlayKind::CommandPalette if app.model_switcher_visible => {
            model_switcher_model(app, plan.palette_overlay?)
        }
        OverlayKind::CommandPalette => command_palette_model(app, plan.palette_overlay?),
        OverlayKind::TogglesMenu => toggles_model(app, plan.palette_overlay?),
        OverlayKind::LineageBrowser => lineage_model(app, plan.palette_overlay?),
        OverlayKind::ForkSelector => fork_model(app, plan.palette_overlay?),
        OverlayKind::ThemeDialog => theme_dialog_model(app, frame_area),
        OverlayKind::PromptStashList => prompt_stash_model(app, frame_area),
        OverlayKind::SettingsEditor => settings_model(app, frame_area),
        OverlayKind::PlanView => plan_model(app, frame_area),
        OverlayKind::MemoryBrowser => memory_model(app, frame_area),
        OverlayKind::WorktreePicker => worktree_model(app, frame_area),
        OverlayKind::ForeignImportPicker => foreign_import_model(app, frame_area),
        OverlayKind::NewWorktreeDialog => new_worktree_model(app, frame_area),
        OverlayKind::SubagentActions => subagent_model(app, frame_area),
        OverlayKind::ErrorDetails => error_details_model(app, frame_area),
        OverlayKind::DetailsDrawer
        | OverlayKind::SlashCommands
        | OverlayKind::FileMentions
        | OverlayKind::StatusDialog
        | OverlayKind::PermissionModal
        | OverlayKind::AuthDialog
        | OverlayKind::TrustFolderPrompt => None,
    }
}

fn help_model(app: &AppState, root: Rect, composer: Option<Rect>) -> Option<ModalSurfaceModel> {
    let layout = crate::ui::ui_secondary_events_tab::help_modal_rects(root, composer)?;
    let mut regions = modal_chrome_regions(layout.popup);
    let (visual_offset, max_scroll) = if app.help_detail().is_some() {
        (
            0,
            crate::ui::ui_secondary_events_tab::help_detail_max_scroll(app, layout),
        )
    } else {
        regions.push(ModalHitRegion {
            target: ModalTarget::Input,
            area: layout.search,
        });
        let rows = app.help_rows();
        let row_layout =
            crate::ui::ui_secondary_events_tab::help_row_layout(app, layout.list, &rows);
        regions.extend(
            row_layout
                .areas
                .into_iter()
                .map(|(index, area)| ModalHitRegion {
                    target: ModalTarget::Row(index),
                    area,
                }),
        );
        (row_layout.offset, row_layout.max_scroll)
    };
    Some(ModalSurfaceModel {
        key: ModalSurfaceKey::Help,
        popup: layout.popup,
        regions,
        visual_offset,
        max_scroll,
    })
}

fn lineage_model(app: &AppState, popup: Rect) -> Option<ModalSurfaceModel> {
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::LineageBrowser,
        view: ModalViewKey::Lineage,
    };
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let child_rows = if app.lineage_child_dialog_view_model().is_some() {
        3
    } else {
        0
    };
    let input = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    let list = Rect::new(
        inner.x.saturating_add(1),
        inner.y.saturating_add(3),
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(3).saturating_sub(child_rows),
    );
    let rows = app.lineage_browser_view_model().rows;
    visual_rows_model(
        app,
        key,
        popup,
        input,
        list,
        rows.len(),
        (0..rows.len()).map(|index| (index, index)),
        rows.iter().position(|row| row.selected).unwrap_or(0),
    )
}

fn fork_model(app: &AppState, popup: Rect) -> Option<ModalSurfaceModel> {
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::ForkSelector,
        view: ModalViewKey::ForkSelector,
    };
    let (_, input, list) = super::command_palette_dialog_layout(popup)?;
    let list = Rect::new(
        list.x.saturating_add(1),
        list.y,
        list.width.saturating_sub(2),
        list.height,
    );
    let rows = app.fork_selector_view_model().rows;
    visual_rows_model(
        app,
        key,
        popup,
        input,
        list,
        rows.len(),
        (0..rows.len()).map(|index| (index, index)),
        rows.iter().position(|row| row.selected).unwrap_or(0),
    )
}

fn subagent_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    app.subagent_actions_session_id.as_ref()?;
    let width = 42.min(root.width.saturating_sub(4));
    let height = 7.min(root.height.saturating_sub(4));
    if width < 28 || height < 5 {
        return None;
    }
    let popup = Rect::new(
        root.x.saturating_add(root.width.saturating_sub(width) / 2),
        root.y
            .saturating_add(root.height.saturating_sub(height) / 2),
        width,
        height,
    );
    let content = Rect::new(
        popup.x.saturating_add(3),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(6),
        popup.height.saturating_sub(2),
    );
    let mut regions = modal_chrome_regions(popup);
    regions.push(ModalHitRegion {
        target: ModalTarget::Row(0),
        area: Rect::new(content.x, content.y.saturating_add(2), content.width, 1),
    });
    Some(ModalSurfaceModel {
        key: ModalSurfaceKey::Overlay {
            kind: OverlayKind::SubagentActions,
            view: ModalViewKey::Primary,
        },
        popup,
        regions,
        visual_offset: 0,
        max_scroll: 0,
    })
}

pub(super) struct ErrorDetailsLayout {
    pub(super) popup: Rect,
    pub(super) inner: Rect,
    pub(super) close: Option<Rect>,
    pub(super) resubmit: Option<Rect>,
}

pub(super) fn error_details_layout(app: &AppState, root: Rect) -> ErrorDetailsLayout {
    let popup = centered_clamped(root, 40, 80, 8, 20);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let error_lines = app
        .activities
        .get(app.transcript_view.selected_activity_index)
        .and_then(|activity| activity.error_message.as_deref())
        .unwrap_or("No error details available")
        .lines()
        .count();
    let footer_y = inner.y.saturating_add(
        u16::try_from(error_lines)
            .unwrap_or(u16::MAX)
            .saturating_add(3),
    );
    let footer_visible = footer_y < inner.bottom();
    ErrorDetailsLayout {
        popup,
        inner,
        close: footer_visible.then_some(Rect::new(inner.x, footer_y, 9.min(inner.width), 1)),
        resubmit: footer_visible.then_some(Rect::new(
            inner.x.saturating_add(14.min(inner.width)),
            footer_y,
            10.min(inner.width.saturating_sub(14)),
            1,
        )),
    }
}

fn error_details_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    let layout = error_details_layout(app, root);
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::ErrorDetails,
        view: ModalViewKey::Primary,
    };
    let mut regions = modal_chrome_regions(layout.popup);
    regions.extend(
        [
            layout.close.map(|area| ModalHitRegion {
                target: ModalTarget::Footer(crate::app::ModalAction::Cancel),
                area,
            }),
            layout.resubmit.map(|area| ModalHitRegion {
                target: ModalTarget::Footer(crate::app::ModalAction::Resubmit),
                area,
            }),
        ]
        .into_iter()
        .flatten(),
    );
    Some(ModalSurfaceModel {
        key,
        popup: layout.popup,
        regions,
        visual_offset: 0,
        max_scroll: 0,
    })
}

fn session_history_model(app: &AppState, popup: Rect) -> Option<ModalSurfaceModel> {
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::CommandPalette,
        view: if app.session_rename_visible {
            ModalViewKey::SessionRename
        } else {
            ModalViewKey::SessionHistory
        },
    };
    if app.session_rename_visible {
        return Some(ModalSurfaceModel {
            key,
            popup,
            regions: modal_chrome_regions(popup),
            visual_offset: 0,
            max_scroll: 0,
        });
    }
    let input = Rect::new(
        popup.x.saturating_add(3),
        popup.y.saturating_add(2),
        popup.width.saturating_sub(6),
        1,
    );
    let list_y = popup.y.saturating_add(4);
    let actions_y = popup.y.saturating_add(popup.height.saturating_sub(2));
    let list = Rect::new(
        popup.x.saturating_add(1),
        list_y,
        popup.width.saturating_sub(2),
        actions_y.saturating_sub(1).saturating_sub(list_y),
    );
    let rows = super::session_history::session_history_visual_rows(app);
    let visible = usize::from(list.height);
    let selected_visual = rows
        .iter()
        .position(|row| {
            matches!(
                row,
                super::session_history::SessionHistoryVisualRow::Entry { selected: true, .. }
            )
        })
        .unwrap_or(0);
    let max_scroll = rows.len().saturating_sub(visible);
    let default = selected_visual.saturating_sub(visible.saturating_sub(1));
    let scroll = app.modal_visual_offset(key, default, max_scroll);
    let mut regions = modal_chrome_regions(popup);
    regions.push(ModalHitRegion {
        target: ModalTarget::Input,
        area: input,
    });
    let mut filtered_index = 0usize;
    for (visual_index, row) in rows.iter().enumerate() {
        let target_index = match row {
            super::session_history::SessionHistoryVisualRow::Entry { .. } => {
                let index = filtered_index;
                filtered_index = filtered_index.saturating_add(1);
                Some(index)
            }
            super::session_history::SessionHistoryVisualRow::Gap
            | super::session_history::SessionHistoryVisualRow::Header(_) => None,
        };
        if visual_index < scroll || visual_index >= scroll.saturating_add(visible) {
            continue;
        }
        if let Some(index) = target_index {
            regions.push(ModalHitRegion {
                target: ModalTarget::Row(index),
                area: Rect::new(
                    list.x,
                    list.y.saturating_add(
                        u16::try_from(visual_index.saturating_sub(scroll)).unwrap_or(u16::MAX),
                    ),
                    list.width,
                    1,
                ),
            });
        }
    }
    Some(ModalSurfaceModel {
        key,
        popup,
        regions,
        visual_offset: scroll,
        max_scroll,
    })
}

fn model_switcher_model(app: &AppState, popup: Rect) -> Option<ModalSurfaceModel> {
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::CommandPalette,
        view: ModalViewKey::ModelSwitcher,
    };
    let input = Rect::new(
        popup.x.saturating_add(4),
        popup.y.saturating_add(3),
        popup.width.saturating_sub(8),
        1,
    );
    let list = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(5),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(6),
    );
    let rows = super::model_switcher::model_switcher_rows(app);
    visual_rows_model(
        app,
        key,
        popup,
        input,
        list,
        rows.len(),
        rows.iter().enumerate().filter_map(|(visual, row)| match row {
            super::model_switcher::ModelSwitcherRow::Option { filtered_index, .. } => {
                Some((visual, *filtered_index))
            }
            super::model_switcher::ModelSwitcherRow::Spacer
            | super::model_switcher::ModelSwitcherRow::Category(_) => None,
        }),
        rows.iter()
            .position(|row| matches!(row, super::model_switcher::ModelSwitcherRow::Option { filtered_index, .. } if *filtered_index == app.model_selected))
            .unwrap_or(0),
    )
}

fn toggles_model(app: &AppState, popup: Rect) -> Option<ModalSurfaceModel> {
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::TogglesMenu,
        view: if app.toggles_yolo_confirmation_visible() {
            ModalViewKey::YoloConfirm
        } else {
            ModalViewKey::Toggles
        },
    };
    if app.toggles_yolo_confirmation_visible() {
        let layout = super::toggles_menu::yolo_warning_layout(popup)?;
        let mut regions = modal_chrome_regions(layout.popup);
        regions.push(ModalHitRegion {
            target: ModalTarget::Footer(crate::app::ModalAction::Activate),
            area: layout.confirm,
        });
        regions.push(ModalHitRegion {
            target: ModalTarget::Footer(crate::app::ModalAction::Cancel),
            area: layout.cancel,
        });
        return Some(ModalSurfaceModel {
            key,
            popup: layout.popup,
            regions,
            visual_offset: 0,
            max_scroll: 0,
        });
    }
    let (_, input, list) = super::command_palette_dialog_layout(popup)?;
    let list = Rect::new(
        list.x.saturating_add(1),
        list.y,
        list.width.saturating_sub(2),
        list.height,
    );
    let rows = super::toggles_menu::toggles_overlay_rows(app);
    let mut logical_index = 0usize;
    let targets = rows
        .iter()
        .enumerate()
        .filter_map(|(visual, row)| match row {
            super::toggles_menu::TogglesOverlayRow::Toggle(_) => {
                let index = logical_index;
                logical_index = logical_index.saturating_add(1);
                Some((visual, index))
            }
            super::toggles_menu::TogglesOverlayRow::Spacer
            | super::toggles_menu::TogglesOverlayRow::Section(_) => None,
        })
        .collect::<Vec<_>>();
    let selected_visual = targets
        .iter()
        .find_map(|(visual, logical)| (*logical == app.toggles_selected).then_some(*visual))
        .unwrap_or(0);
    visual_rows_model(
        app,
        key,
        popup,
        input,
        list,
        rows.len(),
        targets.into_iter(),
        selected_visual,
    )
}

fn visual_rows_model(
    app: &AppState,
    key: ModalSurfaceKey,
    popup: Rect,
    input: Rect,
    list: Rect,
    row_count: usize,
    targets: impl Iterator<Item = (usize, usize)>,
    selected_visual: usize,
) -> Option<ModalSurfaceModel> {
    let visible = usize::from(list.height);
    let max_scroll = row_count.saturating_sub(visible);
    let default = selected_visual.saturating_sub(visible.saturating_sub(1));
    let scroll = app.modal_visual_offset(key, default, max_scroll);
    let mut regions = modal_chrome_regions(popup);
    regions.push(ModalHitRegion {
        target: ModalTarget::Input,
        area: input,
    });
    regions.extend(targets.filter_map(|(visual, logical)| {
        if visual < scroll || visual >= scroll.saturating_add(visible) {
            return None;
        }
        Some(ModalHitRegion {
            target: ModalTarget::Row(logical),
            area: Rect::new(
                list.x,
                list.y.saturating_add(
                    u16::try_from(visual.saturating_sub(scroll)).unwrap_or(u16::MAX),
                ),
                list.width,
                1,
            ),
        })
    }));
    Some(ModalSurfaceModel {
        key,
        popup,
        regions,
        visual_offset: scroll,
        max_scroll,
    })
}

fn prompt_stash_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    uniform_list_model(
        app,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::PromptStashList,
            view: ModalViewKey::Primary,
        },
        centered_clamped(root, 40, 80, 8, 24),
        1,
        app.prompt_stash.entries.len(),
        app.prompt_stash.list_selected,
        1,
    )
}

fn settings_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    uniform_list_model(
        app,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::SettingsEditor,
            view: ModalViewKey::Primary,
        },
        centered_clamped(root, 48, 88, 10, 28),
        2,
        app.settings_editor_rows().len(),
        app.settings_editor_selected_index(),
        0,
    )
}

fn plan_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    let view = if app.plan_view_preview().is_some() {
        ModalViewKey::PlanPreview
    } else {
        ModalViewKey::Primary
    };
    let count = if view == ModalViewKey::Primary {
        app.plan_view_rows().len()
    } else {
        0
    };
    uniform_list_model(
        app,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::PlanView,
            view,
        },
        centered_clamped(root, 48, 88, 10, 28),
        2,
        count,
        app.plan_view_selected_index(),
        0,
    )
}

fn memory_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    uniform_list_model(
        app,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::MemoryBrowser,
            view: ModalViewKey::Primary,
        },
        centered_clamped(root, 40, 80, 8, 24),
        1,
        app.memory_browser.filtered_entries().len(),
        app.memory_browser.selected,
        0,
    )
}

fn worktree_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    uniform_list_model(
        app,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::WorktreePicker,
            view: ModalViewKey::Primary,
        },
        centered_clamped(root, 44, 88, 8, 24),
        1,
        app.worktree_picker.entries.len(),
        app.worktree_picker.selected,
        0,
    )
}

fn foreign_import_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    uniform_list_model(
        app,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::ForeignImportPicker,
            view: ModalViewKey::Primary,
        },
        centered_clamped(root, 44, 96, 8, 24),
        1,
        app.foreign_import_picker.candidates.len(),
        app.foreign_import_picker.selected,
        0,
    )
}

fn new_worktree_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    if root.width < 20 || root.height < 5 {
        return None;
    }
    let typed_width = unicode_width::UnicodeWidthStr::width(app.new_worktree_dialog.input.as_str());
    let desired = 50usize.max(17usize.saturating_add(typed_width).saturating_add(6));
    let width = u16::try_from(desired)
        .unwrap_or(u16::MAX)
        .min(root.width.saturating_sub(4))
        .max(1);
    let popup = crate::layout::centered_overlay_area(root, width, 5);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let content = Rect::new(
        inner.x.saturating_add(1),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::NewWorktreeDialog,
        view: ModalViewKey::Primary,
    };
    Some(ModalSurfaceModel {
        key,
        popup,
        regions: vec![
            ModalHitRegion {
                target: ModalTarget::Input,
                area: Rect::new(content.x, content.y.saturating_add(1), content.width, 1),
            },
            ModalHitRegion {
                target: ModalTarget::Footer(crate::app::ModalAction::Activate),
                area: Rect::new(
                    content.x,
                    content.y.saturating_add(2),
                    16.min(content.width),
                    1,
                ),
            },
            ModalHitRegion {
                target: ModalTarget::Footer(crate::app::ModalAction::Cancel),
                area: Rect::new(
                    content.x.saturating_add(16.min(content.width)),
                    content.y.saturating_add(2),
                    content.width.saturating_sub(16.min(content.width)),
                    1,
                ),
            },
        ],
        visual_offset: 0,
        max_scroll: 0,
    })
}

fn uniform_list_model(
    app: &AppState,
    key: ModalSurfaceKey,
    popup: Rect,
    header_rows: u16,
    count: usize,
    selected: usize,
    footer_rows: u16,
) -> Option<ModalSurfaceModel> {
    if popup.width < 3 || popup.height < 3 {
        return None;
    }
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let list = Rect::new(
        inner.x,
        inner.y.saturating_add(header_rows),
        inner.width,
        inner
            .height
            .saturating_sub(header_rows)
            .saturating_sub(footer_rows),
    );
    let visible = usize::from(list.height);
    let max_scroll = count.saturating_sub(visible);
    let default = selected
        .min(count.saturating_sub(1))
        .saturating_sub(visible.saturating_sub(1));
    let scroll = app.modal_visual_offset(key, default, max_scroll);
    let mut regions = modal_chrome_regions(popup);
    regions.extend(
        (scroll..count.min(scroll.saturating_add(visible))).map(|index| ModalHitRegion {
            target: ModalTarget::Row(index),
            area: Rect::new(
                list.x,
                list.y.saturating_add(
                    u16::try_from(index.saturating_sub(scroll)).unwrap_or(u16::MAX),
                ),
                list.width,
                1,
            ),
        }),
    );
    Some(ModalSurfaceModel {
        key,
        popup,
        regions,
        visual_offset: scroll,
        max_scroll,
    })
}

fn centered_clamped(
    root: Rect,
    min_width: u16,
    max_width: u16,
    min_height: u16,
    max_height: u16,
) -> Rect {
    let width = root.width.clamp(min_width, max_width);
    let height = root.height.clamp(min_height, max_height);
    Rect::new(
        root.x + root.width.saturating_sub(width) / 2,
        root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn command_palette_model(app: &AppState, popup: Rect) -> Option<ModalSurfaceModel> {
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::CommandPalette,
        view: ModalViewKey::Primary,
    };
    let (_, input, list) = super::command_palette_dialog_layout(popup)?;
    let list = if list.width <= 1 {
        list
    } else {
        Rect::new(
            list.x.saturating_add(1),
            list.y,
            list.width.saturating_sub(1),
            list.height,
        )
    };
    let rows = super::palette_overlay_rows(app);
    let visible = usize::from(list.height);
    let max_scroll = rows.len().saturating_sub(visible);
    let selected = app
        .palette_selected
        .min(app.palette_filtered.len().saturating_sub(1));
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, super::PaletteOverlayRow::Command { is_selected, .. } if *is_selected == selected))
        .unwrap_or(0);
    let default = selected_row.saturating_sub(visible.saturating_sub(1));
    let scroll = app.modal_visual_offset(key, default, max_scroll);
    let mut regions = modal_chrome_regions(popup);
    regions.push(ModalHitRegion {
        target: ModalTarget::Input,
        area: input,
    });
    regions.extend(
        rows.iter()
            .enumerate()
            .skip(scroll)
            .take(visible)
            .filter_map(|(visual_index, row)| {
                let super::PaletteOverlayRow::Command { is_selected, .. } = row else {
                    return None;
                };
                Some(ModalHitRegion {
                    target: ModalTarget::Row(*is_selected),
                    area: Rect::new(
                        list.x,
                        list.y.saturating_add(
                            u16::try_from(visual_index.saturating_sub(scroll)).unwrap_or(u16::MAX),
                        ),
                        list.width,
                        1,
                    ),
                })
            }),
    );
    Some(ModalSurfaceModel {
        key,
        popup,
        regions,
        visual_offset: scroll,
        max_scroll,
    })
}

fn theme_dialog_model(app: &AppState, root: Rect) -> Option<ModalSurfaceModel> {
    let popup = super::theme_dialog::theme_dialog_area(root)?;
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::ThemeDialog,
        view: ModalViewKey::Primary,
    };
    let rows = super::theme_dialog::theme_dialog_row_areas(popup);
    let mut regions = modal_chrome_regions(popup);
    regions.extend(
        rows.into_iter()
            .enumerate()
            .map(|(index, area)| ModalHitRegion {
                target: ModalTarget::Row(index),
                area,
            }),
    );
    let _ = app;
    Some(ModalSurfaceModel {
        key,
        popup,
        regions,
        visual_offset: 0,
        max_scroll: 0,
    })
}

fn modal_chrome_regions(popup: Rect) -> Vec<ModalHitRegion> {
    let close_width = 6.min(popup.width);
    vec![ModalHitRegion {
        target: ModalTarget::Close,
        area: Rect::new(
            popup.right().saturating_sub(close_width),
            popup.y,
            close_width,
            1,
        ),
    }]
}
