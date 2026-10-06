use ratatui::layout::Rect;

use super::{
    composer_footer_spacer_rows, composer_input_height, inset_composer_width,
    permission_dock_measure, question_dock_measure, session_operator_overlay,
    startup_composer_input_height, surfaces, ControlDockLayout, FrameLayoutPlan, WheelHitAreas,
    QUESTION_OUTER_FOOTER_ROWS,
};
use crate::app::AppState;
use crate::theme::LiveShellLayout;

/// Project panes and the control dock into the same plan used for paint and input.
pub(super) fn project(
    app: &AppState,
    plan: &mut FrameLayoutPlan,
    layout: LiveShellLayout,
    child_footer: bool,
) {
    let area = plan.shell;
    let terminal_height = plan.root.height;
    let tokens = app.theme().token_families().live_shell;
    let gap = tokens.spacing.rhythm.surface_gap / 2;
    let startup = app.startup_shell_visible();
    let mut column = area;
    if app.replay_mode && !startup && !app.current_subagent_session_present() {
        let width = layout
            .details_sidebar_width
            .min(area.width.saturating_sub(1))
            .max(1);
        if area.width
            >= layout
                .transcript_min_width
                .max(48)
                .saturating_add(gap)
                .saturating_add(width)
        {
            column = Rect::new(
                area.x,
                area.y,
                area.width.saturating_sub(width).saturating_sub(gap),
                area.height,
            );
            plan.operator_sidebar = Some(Rect::new(
                column.right().saturating_add(gap),
                area.y,
                width,
                area.height,
            ));
        }
    }

    let permission = app.active_permission_view().filter(|_| !app.replay_mode);
    let question_inset = 2.min(column.width / 2);
    let question_width = column.width.saturating_sub(question_inset * 2);
    let mut dock = if let Some(permission) = permission.as_ref() {
        let height = if permission.question_prompts.is_some() {
            question_dock_measure(
                app,
                question_width,
                Rect::new(0, 0, area.width, terminal_height),
                permission,
            )
            .status_height
        } else {
            permission_dock_measure(app, question_width, terminal_height, permission)
                .height
                .saturating_add(QUESTION_OUTER_FOOTER_ROWS)
                .min(column.height)
        };
        let [_, body, shell] = surfaces::split_rows(column, 0, height);
        plan.transcript = Some(body);
        let status = Rect {
            x: shell.x.saturating_add(question_inset),
            width: question_width,
            ..shell
        };
        ControlDockLayout {
            shell: status,
            status: Some(status),
            composer: Rect::new(status.x, status.bottom(), status.width, 0),
            disclosure: None,
        }
    } else if app.replay_mode {
        let height = if child_footer {
            0
        } else {
            tokens.spacing.heights.prompt_block()
        };
        let [_, body, shell] = surfaces::split_rows(column, 0, height);
        plan.transcript = Some(body);
        ControlDockLayout {
            shell,
            status: None,
            composer: shell,
            disclosure: None,
        }
    } else {
        let composer_height = prompt_height(app, column, terminal_height, startup);
        let spacer = composer_footer_spacer_rows(terminal_height);
        let status = if app.live_turn_status_visible() || app.model_prompt_notice.is_some() {
            tokens.spacing.heights.status
        } else {
            0
        };
        let status_gap = if status > 0 || app.activities.is_empty() {
            spacer
        } else {
            0
        };
        let disclosure = u16::from(!startup && app.review_surface().is_none());
        let height = if child_footer {
            0
        } else if startup {
            composer_height
                .saturating_add(status)
                .saturating_add(if status > 0 { status_gap } else { 0 })
        } else {
            composer_height
                .saturating_add(status_gap)
                .saturating_add(spacer)
                .saturating_add(status)
                .saturating_add(spacer)
                .saturating_add(disclosure)
        };
        let [_, body, shell] = surfaces::split_rows(column, 0, height);
        plan.transcript = Some(body);
        if startup {
            let [status, _, composer] = surfaces::split_rows(shell, status, composer_height);
            ControlDockLayout {
                shell,
                status: (status.height > 0).then_some(status),
                composer: Rect {
                    height: composer
                        .height
                        .saturating_sub(1)
                        .max(3)
                        .min(composer.height),
                    ..composer
                },
                disclosure: None,
            }
        } else {
            live_dock(
                shell,
                composer_height,
                status,
                status_gap,
                spacer,
                disclosure,
            )
        }
    };
    if plan.operator_sidebar.is_some() {
        dock = surfaces::reserve_sidebar_gap(dock, tokens.spacing.rhythm.transcript_gutter_x);
    }
    if startup {
        dock = surfaces::inset_dock(dock, area, true);
    } else if let Some(body) = plan.transcript {
        let task_height = app
            .task_pane_height(terminal_height)
            .min(body.height.saturating_sub(5));
        let body = if task_height > 0 {
            plan.tasks = Some(Rect::new(
                body.x.saturating_add(5),
                body.y.saturating_add(3),
                body.width.saturating_sub(9),
                task_height,
            ));
            Rect::new(
                body.x,
                body.y.saturating_add(task_height + 1),
                body.width,
                body.height.saturating_sub(task_height + 1),
            )
        } else {
            body
        };
        let (body, todo) = todo_pane(app, body, terminal_height);
        plan.todo = todo;
        let (transcript, terminal) = terminal_panel(app, body, gap);
        plan.transcript = Some(transcript);
        plan.terminal_panel = terminal;
        if plan.operator_sidebar.is_none() && app.details_drawer_open() {
            // The details overlay covers both transcript and terminal, below the todo reserve.
            plan.details_overlay = session_operator_overlay(body, plan.session_contract);
        }
        if permission.is_none() {
            dock = surfaces::inset_dock(dock, column, false);
        }
    }
    let empty_sidebar = plan.operator_sidebar.is_some() && !app.operator_rail_has_sections();
    let inspector = if empty_sidebar {
        None
    } else {
        plan.operator_sidebar.or(plan.details_overlay)
    };
    plan.wheel_hit_areas = WheelHitAreas {
        transcript: plan.transcript,
        terminal_panel: plan.terminal_panel,
        overlay: inspector,
        inspector,
    };
    plan.status = dock.status;
    plan.composer = Some(dock.composer);
    plan.disclosure = dock.disclosure;
    plan.dock = Some(dock);
}

fn prompt_height(app: &AppState, area: Rect, terminal_height: u16, startup: bool) -> u16 {
    if let Some(state) = app.rewind.state.as_ref() {
        return crate::rewind_view::rewind_overlay_height(&state.phase, terminal_height)
            .min(area.height);
    }
    let width = inset_composer_width(area.width);
    let input = if startup {
        startup_composer_input_height(&app.composer.prompt_buffer, width, terminal_height)
    } else {
        composer_input_height(&app.composer.prompt_buffer, width)
    };
    let chrome = if startup { 3 } else { 2 };
    input
        .max(1)
        .saturating_add(chrome)
        .min(area.height)
        .max(chrome + 1)
}

fn live_dock(
    shell: Rect,
    composer_height: u16,
    status: u16,
    status_gap: u16,
    spacer: u16,
    disclosure: u16,
) -> ControlDockLayout {
    let disclosure = disclosure.min(shell.height.saturating_sub(1));
    let composer_height = composer_height.min(shell.height);
    let status = status.min(shell.height.saturating_sub(composer_height));
    let composer_gap = spacer.min(shell.height.saturating_sub(status));
    let status_gap = status_gap.min(shell.height.saturating_sub(status));
    let bottom_margin = spacer.min(
        shell.height.saturating_sub(
            status
                .saturating_add(status_gap)
                .saturating_add(composer_height)
                .saturating_add(composer_gap)
                .saturating_add(disclosure),
        ),
    );
    let disclosure_y = shell
        .bottom()
        .saturating_sub(bottom_margin)
        .saturating_sub(disclosure);
    let composer_y = disclosure_y
        .saturating_sub(composer_gap)
        .saturating_sub(composer_height);
    let status_y = composer_y.saturating_sub(status_gap).saturating_sub(status);
    ControlDockLayout {
        shell,
        status: (status > 0).then_some(Rect::new(shell.x, status_y, shell.width, status)),
        composer: Rect::new(shell.x, composer_y, shell.width, composer_height),
        disclosure: (disclosure > 0).then_some(Rect::new(
            shell.x,
            disclosure_y,
            shell.width,
            disclosure,
        )),
    }
}

fn todo_pane(app: &AppState, body: Rect, terminal_height: u16) -> (Rect, Option<Rect>) {
    if !app.todo_pane.visible
        || app.transcript_viewer().is_some()
        || body.width < 6
        || body.height < 6
    {
        return (body, None);
    }
    let height = app
        .todo_pane
        .desired_height(terminal_height)
        .min(body.height.saturating_sub(5));
    let todo = Rect::new(
        body.x.saturating_add(2),
        body.y.saturating_add(3),
        body.width.saturating_sub(4),
        height,
    );
    let reserve = height.saturating_add(2);
    (
        Rect::new(
            body.x,
            body.y.saturating_add(reserve),
            body.width,
            body.height.saturating_sub(reserve),
        ),
        Some(todo),
    )
}

fn terminal_panel(app: &AppState, body: Rect, gap: u16) -> (Rect, Option<Rect>) {
    if !app.terminal_panel_visible() || body.width == 0 {
        return (body, None);
    }
    let gap = gap.min(body.height.saturating_sub(2));
    if body.height < 7_u16.saturating_add(gap).saturating_add(5) {
        return (body, None);
    }
    let height = (body.height / 3)
        .clamp(5, 12)
        .min(body.height.saturating_sub(gap).saturating_sub(7));
    let transcript_height = body.height.saturating_sub(gap).saturating_sub(height);
    (
        Rect::new(body.x, body.y, body.width, transcript_height),
        Some(Rect::new(
            body.x,
            body.y.saturating_add(transcript_height).saturating_add(gap),
            body.width,
            height,
        )),
    )
}
