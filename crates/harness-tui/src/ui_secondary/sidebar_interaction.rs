use super::*;

fn build_operator_sidebar_selection_snapshot(
    app: &AppState,
    inner: Rect,
    theme: &Theme,
) -> Option<OperatorSidebarSelectionSnapshot> {
    let rail = build_operator_rail_model(app);
    let body_area = operator_sidebar_body_area(app, inner, theme, rail.title.as_ref())?;

    let body_layout = build_operator_rail_body_layout(
        &rail.body,
        theme,
        body_area.width,
        app.transcript_view.transcript_animation_phase(),
    );
    Some(OperatorSidebarSelectionSnapshot {
        viewport: body_area,
        scroll_top: usize::from(app.details_scroll),
        rows: operator_sidebar_selection_rows(&body_layout.lines, body_area.width),
    })
}

pub(super) fn operator_sidebar_selection_snapshot_in_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    chrome: OperatorSidebarChrome,
) -> Option<OperatorSidebarSelectionSnapshot> {
    let inner = operator_sidebar_inner_area(app, area, theme, chrome)?;
    build_operator_sidebar_selection_snapshot(app, inner, theme)
}

fn operator_sidebar_selection_snapshot(
    app: &AppState,
    frame_area: Rect,
) -> Option<OperatorSidebarSelectionSnapshot> {
    let plan = crate::layout::FrameLayoutPlan::for_app(app, frame_area);
    let theme = app.theme();

    plan.operator_sidebar
        .and_then(|area| {
            operator_sidebar_selection_snapshot_in_surface(
                app,
                area,
                theme,
                OperatorSidebarChrome::Persistent,
            )
        })
        .or_else(|| {
            plan.details_overlay.and_then(|area| {
                operator_sidebar_selection_snapshot_in_surface(
                    app,
                    area,
                    theme,
                    OperatorSidebarChrome::Overlay,
                )
            })
        })
}

fn operator_sidebar_selection_rows(
    lines: &[Line<'static>],
    width: u16,
) -> Vec<OperatorSidebarSelectionRow> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    for line in lines {
        rows.extend(
            operator_sidebar_selection_line_rows(line, width)
                .into_iter()
                .enumerate()
                .map(|(index, cells)| OperatorSidebarSelectionRow {
                    cells,
                    continues_previous: index > 0,
                }),
        );
    }
    rows
}

fn operator_sidebar_selection_line_rows(line: &Line<'static>, width: usize) -> Vec<Vec<String>> {
    let mut row = Vec::new();
    let mut rows = Vec::new();

    for span in &line.spans {
        for ch in span.content.chars() {
            let display = ch.to_string();
            let cell_width = display_width(&display).max(1);
            if row.len() + cell_width > width {
                row.resize(width, " ".to_string());
                rows.push(std::mem::take(&mut row));
            }

            row.push(display);
            for _ in 1..cell_width {
                if row.len() == width {
                    rows.push(std::mem::take(&mut row));
                }
                row.push(String::new());
            }

            if row.len() == width {
                rows.push(std::mem::take(&mut row));
            }
        }
    }

    if rows.is_empty() && row.is_empty() {
        row.resize(width, " ".to_string());
        rows.push(row);
        return rows;
    }

    if !row.is_empty() {
        row.resize(width, " ".to_string());
        rows.push(row);
    }

    rows
}

impl OperatorSidebarSelectionSnapshot {
    fn hit(&self, column: u16, row: u16) -> Option<OperatorSidebarSelectionCell> {
        if !rect_contains(self.viewport, column, row) {
            return None;
        }

        let selection_cell = OperatorSidebarSelectionCell {
            row: self
                .scroll_top
                .saturating_add(usize::from(row.saturating_sub(self.viewport.y))),
            column: usize::from(column.saturating_sub(self.viewport.x)),
        };
        self.selectable_cell(selection_cell)
            .then_some(selection_cell)
    }

    fn selectable_cell(&self, cell: OperatorSidebarSelectionCell) -> bool {
        let Some(row) = self.rows.get(cell.row) else {
            return false;
        };
        let Some(content_end) = operator_sidebar_selection_row_content_end(row) else {
            return false;
        };
        cell.column <= content_end
    }

    fn selection_text(&self, selection: OperatorSidebarSelection) -> Option<String> {
        let (start, end) = selection.normalized();
        if start == end || self.rows.is_empty() {
            return None;
        }

        let last_row = self.rows.len().saturating_sub(1);
        let start_row = start.row.min(last_row);
        let end_row = end.row.min(last_row);
        if start_row > end_row {
            return None;
        }

        let mut lines = Vec::new();
        for row_idx in start_row..=end_row {
            let row = self.rows.get(row_idx)?;
            let Some(content_end) = operator_sidebar_selection_row_content_end(row) else {
                if row_idx == start_row || !row.continues_previous || lines.is_empty() {
                    lines.push(String::new());
                }
                continue;
            };

            let row_start = if row_idx == start_row {
                start.column.min(row.cells.len().saturating_sub(1))
            } else {
                0
            };
            let row_end = if row_idx == end_row {
                end.column.min(content_end)
            } else {
                content_end
            };
            if row_start > row_end {
                lines.push(String::new());
                continue;
            }

            let mut text = String::new();
            for cell in row
                .cells
                .iter()
                .skip(row_start)
                .take(row_end - row_start + 1)
            {
                text.push_str(cell);
            }
            if row_idx != start_row && row.continues_previous && !lines.is_empty() {
                let continuation = text.trim_start_matches(' ');
                let current = lines.last_mut().expect("continuation has previous line");
                if !continuation.is_empty() && !current.ends_with(char::is_whitespace) {
                    current.push(' ');
                }
                current.push_str(continuation);
            } else {
                lines.push(text);
            }
        }

        Some(lines.join("\n"))
    }
}

fn operator_sidebar_selection_row_content_end(row: &OperatorSidebarSelectionRow) -> Option<usize> {
    row.cells
        .iter()
        .enumerate()
        .rev()
        .find(|(_, cell)| !cell.is_empty() && cell.as_str() != " ")
        .map(|(idx, _)| idx)
}

pub(super) fn render_operator_sidebar_selection(
    frame: &mut Frame,
    selection: Option<OperatorSidebarSelection>,
    snapshot: Option<OperatorSidebarSelectionSnapshot>,
    area: Rect,
    theme: &Theme,
) {
    let Some(selection) = selection else {
        return;
    };
    let Some(snapshot) = snapshot else {
        return;
    };
    if snapshot.rows.is_empty() {
        return;
    }

    let (start, end) = selection.normalized();
    if start == end {
        return;
    }

    let visible_height = usize::from(area.height);
    let max_row = snapshot.rows.len().saturating_sub(1);
    let start_row = start.row.min(max_row);
    let end_row = end.row.min(max_row);
    let buffer = frame.buffer_mut();

    for local_row in 0..visible_height {
        let absolute_row = snapshot.scroll_top.saturating_add(local_row);
        if absolute_row < start_row || absolute_row > end_row {
            continue;
        }

        let row = &snapshot.rows[absolute_row];
        let row_start = if absolute_row == start_row {
            start.column.min(row.cells.len().saturating_sub(1))
        } else {
            0
        };
        let row_end = if absolute_row == end_row {
            end.column.min(row.cells.len().saturating_sub(1))
        } else {
            row.cells.len().saturating_sub(1)
        };
        if row_start > row_end {
            continue;
        }

        let y = area
            .y
            .saturating_add(u16::try_from(local_row).unwrap_or(u16::MAX));
        for column in row_start..=row_end {
            let x = area
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
            if x >= area.right() || y >= area.bottom() {
                continue;
            }

            let cell = &mut buffer[(x, y)];
            cell.set_fg(theme.text.inverse);
            cell.set_bg(theme.status.info);
        }
    }
}

pub(crate) fn operator_sidebar_selection_cell(
    app: &AppState,
    frame_area: Rect,
    column: u16,
    row: u16,
) -> Option<OperatorSidebarSelectionCell> {
    operator_sidebar_selection_snapshot(app, frame_area)
        .and_then(|snapshot| snapshot.hit(column, row))
}

pub(crate) fn operator_sidebar_selection_text(
    app: &AppState,
    frame_area: Rect,
    selection: OperatorSidebarSelection,
) -> Option<String> {
    operator_sidebar_selection_snapshot(app, frame_area)
        .and_then(|snapshot| snapshot.selection_text(selection))
}

pub(crate) fn operator_sidebar_section_hit_target(
    app: &AppState,
    frame_area: Rect,
    column: u16,
    row: u16,
) -> Option<OperatorSidebarSection> {
    let plan = crate::layout::FrameLayoutPlan::for_app(app, frame_area);
    let theme = app.theme();

    plan.operator_sidebar
        .and_then(|area| {
            operator_sidebar_section_hit_target_in_surface(
                app,
                area,
                theme,
                OperatorSidebarChrome::Persistent,
                column,
                row,
            )
        })
        .or_else(|| {
            plan.details_overlay.and_then(|area| {
                operator_sidebar_section_hit_target_in_surface(
                    app,
                    area,
                    theme,
                    OperatorSidebarChrome::Overlay,
                    column,
                    row,
                )
            })
        })
}

pub(crate) fn operator_sidebar_subagent_session_hit_target(
    app: &AppState,
    frame_area: Rect,
    column: u16,
    row: u16,
) -> Option<String> {
    let plan = crate::layout::FrameLayoutPlan::for_app(app, frame_area);
    let theme = app.theme();

    plan.operator_sidebar
        .and_then(|area| {
            operator_sidebar_subagent_session_hit_target_in_surface(
                app,
                area,
                theme,
                OperatorSidebarChrome::Persistent,
                column,
                row,
            )
        })
        .or_else(|| {
            plan.details_overlay.and_then(|area| {
                operator_sidebar_subagent_session_hit_target_in_surface(
                    app,
                    area,
                    theme,
                    OperatorSidebarChrome::Overlay,
                    column,
                    row,
                )
            })
        })
}

pub(crate) fn operator_sidebar_subagent_group_hit_target(
    app: &AppState,
    frame_area: Rect,
    column: u16,
    row: u16,
) -> Option<String> {
    let plan = crate::layout::FrameLayoutPlan::for_app(app, frame_area);
    let theme = app.theme();

    plan.operator_sidebar
        .and_then(|area| {
            operator_sidebar_subagent_group_hit_target_in_surface(
                app,
                area,
                theme,
                OperatorSidebarChrome::Persistent,
                column,
                row,
            )
        })
        .or_else(|| {
            plan.details_overlay.and_then(|area| {
                operator_sidebar_subagent_group_hit_target_in_surface(
                    app,
                    area,
                    theme,
                    OperatorSidebarChrome::Overlay,
                    column,
                    row,
                )
            })
        })
}

pub(crate) fn operator_sidebar_keyboard_targets(
    app: &AppState,
    frame_area: Option<Rect>,
) -> Vec<OperatorSidebarKeyboardTarget> {
    let theme = app.theme();
    let rail = build_operator_rail_model(app);
    let width = frame_area
        .and_then(|area| {
            operator_sidebar_body_width_for_frame(app, area, theme, rail.title.as_ref())
        })
        .unwrap_or(80);
    let layout = build_operator_rail_body_layout(
        &rail.body,
        theme,
        width,
        app.transcript_view.transcript_animation_phase(),
    );

    operator_sidebar_keyboard_targets_from_layout(&layout)
}

fn operator_sidebar_body_width_for_frame(
    app: &AppState,
    frame_area: Rect,
    theme: &Theme,
    title: Option<&OperatorRailTitle>,
) -> Option<u16> {
    let plan = crate::layout::FrameLayoutPlan::for_app(app, frame_area);

    plan.operator_sidebar
        .and_then(|area| {
            operator_sidebar_inner_area(app, area, theme, OperatorSidebarChrome::Persistent)
                .and_then(|inner| operator_sidebar_body_area(app, inner, theme, title))
                .map(|body| body.width)
        })
        .or_else(|| {
            plan.details_overlay.and_then(|area| {
                operator_sidebar_inner_area(app, area, theme, OperatorSidebarChrome::Overlay)
                    .and_then(|inner| operator_sidebar_body_area(app, inner, theme, title))
                    .map(|body| body.width)
            })
        })
}

fn operator_sidebar_keyboard_targets_from_layout(
    layout: &OperatorRailBodyLayout,
) -> Vec<OperatorSidebarKeyboardTarget> {
    let mut targets = Vec::new();
    targets.extend(
        layout
            .heading_hit_regions
            .iter()
            .map(|region| OperatorSidebarKeyboardTarget {
                kind: OperatorSidebarKeyboardTargetKind::Section(region.section),
                top_row: region.top_row,
                height: region.height,
            }),
    );
    targets.extend(layout.subagent_group_hit_regions.iter().map(|region| {
        OperatorSidebarKeyboardTarget {
            kind: OperatorSidebarKeyboardTargetKind::SubagentGroup(region.agent_name.clone()),
            top_row: region.top_row,
            height: region.height,
        }
    }));
    targets.extend(layout.subagent_hit_regions.iter().map(|region| {
        OperatorSidebarKeyboardTarget {
            kind: OperatorSidebarKeyboardTargetKind::SubagentSession(region.session_id.clone()),
            top_row: region.top_row,
            height: region.height,
        }
    }));
    targets.sort_by_key(|target| target.top_row);
    targets
}

pub(super) fn render_operator_sidebar_keyboard_selection(
    frame: &mut Frame,
    app: &AppState,
    layout: &OperatorRailBodyLayout,
    area: Rect,
    theme: &Theme,
) {
    if !(app.focus == Focus::List && activity_surface_visible(app)) {
        return;
    }

    let Some(selected) = app.selected_operator_sidebar_keyboard_index() else {
        return;
    };

    let targets = operator_sidebar_keyboard_targets_from_layout(layout);
    if targets.is_empty() {
        return;
    }
    let target = &targets[selected.min(targets.len().saturating_sub(1))];
    let scroll_top = usize::from(app.details_scroll);
    let visible_height = usize::from(area.height);
    let target_bottom = target.top_row.saturating_add(target.height.max(1));
    let buffer = frame.buffer_mut();

    for local_row in 0..visible_height {
        let absolute_row = scroll_top.saturating_add(local_row);
        if absolute_row < target.top_row || absolute_row >= target_bottom {
            continue;
        }

        let y = area
            .y
            .saturating_add(u16::try_from(local_row).unwrap_or(u16::MAX));
        if y >= area.bottom() {
            continue;
        }

        for local_column in 0..area.width {
            let x = area.x.saturating_add(local_column);
            if x >= area.right() {
                continue;
            }

            let cell = &mut buffer[(x, y)];
            cell.set_fg(theme.text.inverse);
            cell.set_bg(theme.border.focus);
        }
    }
}

fn operator_sidebar_section_hit_target_in_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    chrome: OperatorSidebarChrome,
    column: u16,
    row: u16,
) -> Option<OperatorSidebarSection> {
    let inner = operator_sidebar_inner_area(app, area, theme, chrome)?;
    if !rect_contains(inner, column, row) {
        return None;
    }

    let rail = build_operator_rail_model(app);
    let body_area = operator_sidebar_body_area(app, inner, theme, rail.title.as_ref())?;
    rect_contains(body_area, column, row).then_some(())?;

    let layout = build_operator_rail_body_layout(
        &rail.body,
        theme,
        body_area.width,
        app.transcript_view.transcript_animation_phase(),
    );
    let visual_row =
        usize::from(row.saturating_sub(body_area.y)).saturating_add(app.details_scroll.into());

    layout.heading_hit_regions.into_iter().find_map(|region| {
        (visual_row >= region.top_row && visual_row < region.top_row.saturating_add(region.height))
            .then_some(region.section)
    })
}

pub(super) fn operator_sidebar_subagent_session_hit_target_in_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    chrome: OperatorSidebarChrome,
    column: u16,
    row: u16,
) -> Option<String> {
    let inner = operator_sidebar_inner_area(app, area, theme, chrome)?;
    if !rect_contains(inner, column, row) {
        return None;
    }

    let rail = build_operator_rail_model(app);
    let body_area = operator_sidebar_body_area(app, inner, theme, rail.title.as_ref())?;
    rect_contains(body_area, column, row).then_some(())?;

    let layout = build_operator_rail_body_layout(
        &rail.body,
        theme,
        body_area.width,
        app.transcript_view.transcript_animation_phase(),
    );
    let visual_row =
        usize::from(row.saturating_sub(body_area.y)).saturating_add(app.details_scroll.into());

    layout.subagent_hit_regions.into_iter().find_map(|region| {
        (visual_row >= region.top_row && visual_row < region.top_row.saturating_add(region.height))
            .then_some(region.session_id)
    })
}

pub(super) fn operator_sidebar_subagent_group_hit_target_in_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    chrome: OperatorSidebarChrome,
    column: u16,
    row: u16,
) -> Option<String> {
    let inner = operator_sidebar_inner_area(app, area, theme, chrome)?;
    if !rect_contains(inner, column, row) {
        return None;
    }

    let rail = build_operator_rail_model(app);
    let body_area = operator_sidebar_body_area(app, inner, theme, rail.title.as_ref())?;
    rect_contains(body_area, column, row).then_some(())?;

    let layout = build_operator_rail_body_layout(
        &rail.body,
        theme,
        body_area.width,
        app.transcript_view.transcript_animation_phase(),
    );
    let visual_row =
        usize::from(row.saturating_sub(body_area.y)).saturating_add(app.details_scroll.into());

    layout
        .subagent_group_hit_regions
        .into_iter()
        .find_map(|region| {
            (visual_row >= region.top_row
                && visual_row < region.top_row.saturating_add(region.height))
            .then_some(region.agent_name)
        })
}

pub(super) fn operator_sidebar_body_area(
    app: &AppState,
    inner: Rect,
    theme: &Theme,
    title: Option<&OperatorRailTitle>,
) -> Option<Rect> {
    let title_text = build_operator_rail_title_text(title, theme, inner.width);
    let title_height = title_text.lines.len().min(usize::from(u16::MAX)) as u16;
    let footer_height = operator_sidebar_footer_height(app, theme, inner.width);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(inner);
    let body_area = sections[1];
    (body_area.width > 0 && body_area.height > 0).then_some(body_area)
}

pub(super) fn operator_sidebar_footer_height(app: &AppState, theme: &Theme, width: u16) -> u16 {
    let directory_height = app
        .sidebar_directory_branch_label()
        .map(|label| {
            operator_sidebar_directory_footer_text(label, theme, width, theme.surface.panel)
                .height()
        })
        .unwrap_or(0);
    directory_height
        .saturating_add(1)
        .min(usize::from(u16::MAX)) as u16
}

pub(super) fn operator_sidebar_inner_area(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    chrome: OperatorSidebarChrome,
) -> Option<Rect> {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);
    let surface = match chrome {
        OperatorSidebarChrome::Persistent => ui_chrome::divided_shell_surface(theme),
        OperatorSidebarChrome::Overlay => ui_chrome::divided_shell_surface(theme),
    };

    let inner = match chrome {
        OperatorSidebarChrome::Persistent => inset_rect(
            area,
            theme.live_shell.rhythm.sidebar_padding_x,
            theme.live_shell.rhythm.sidebar_padding_y,
        ),
        OperatorSidebarChrome::Overlay => {
            ui_chrome::secondary_pane_block(theme, Line::default(), is_focused, surface).inner(area)
        }
    };

    (inner.width > 0 && inner.height > 0).then_some(inner)
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}
