// allow: SIZE_OK — TUI chrome rendering (indivisible view model)
use super::*;

use ratatui::widgets::Padding;

use crate::layout::{ControlDockLayout, FrameLayoutPlan, SessionFooterMode, SessionHeaderMode};
use crate::text::has_trimmed_content;
use crate::theme::{ChromeMode, DividerIntensity};

#[cfg(test)]
#[path = "ui_chrome_exact_tests.rs"]
mod ui_chrome_exact_tests;
#[cfg(test)]
#[path = "ui_subagent_footer_entry_body_tests.rs"]
mod ui_subagent_footer_entry_body_tests;
#[cfg(test)]
#[path = "ui_subagent_footer_exact_tests.rs"]
mod ui_subagent_footer_exact_tests;
#[cfg(test)]
pub(crate) use ui_chrome_exact_tests::{
    exact_test_composer_viewport_wraps_at_word_boundaries,
    exact_test_composer_viewport_wraps_by_display_width,
    exact_test_footer_status_cluster_empty_when_no_activity,
    exact_test_footer_status_cluster_shows_pending_permission_count,
    exact_test_live_composer_disclosure_none_context_shows_est_zero,
    exact_test_live_composer_disclosure_none_context_shows_percent_when_limit_known,
    exact_test_live_composer_disclosure_summarizes_compaction_metrics,
    exact_test_live_composer_metadata_omits_success_without_variant,
    exact_test_live_composer_reserves_right_gap,
    exact_test_live_control_dock_collapses_disclosure_before_status,
    exact_test_live_control_dock_renders_shared_surface,
    exact_test_retry_summary_segment_prioritizes_retry_indicator,
    exact_test_startup_disclosure_matches_harness_hint_row,
    exact_test_subagent_footer_matches_harness_layout,
    exact_test_subagent_replay_suppresses_parent_replay_dock,
    exact_test_tool_status_summary_uses_effective_tool_identity,
};
#[cfg(test)]
pub(crate) use ui_subagent_footer_exact_tests::{
    exact_test_subagent_footer_body_keeps_ordered_transcript_tool_rows,
    exact_test_subagent_footer_status_uses_running_and_cancelled_icons,
};

#[path = "ui_subagent_footer.rs"]
mod ui_subagent_footer;
#[path = "ui_subagent_footer_navigation.rs"]
mod ui_subagent_footer_navigation;
use self::ui_subagent_footer::render_subagent_footer;
pub(crate) use self::ui_subagent_footer_navigation::{
    subagent_footer_target_at, SubagentFooterTarget,
};
#[path = "ui_permission_dock.rs"]
mod ui_permission_dock;
use self::ui_permission_dock::render_inline_permission_dock;
pub(super) use self::ui_permission_dock::{question_prompt_accent, question_prompt_secondary};
#[path = "ui_control_dock_disclosure.rs"]
mod ui_control_dock_disclosure;
use self::ui_control_dock_disclosure::{
    completed_session_status_summary, composer_shortcut_hints, render_control_dock_disclosure,
    render_replay_read_only_composer_content, replay_read_only_shortcut_hints, status_context,
};
#[path = "ui_composer.rs"]
mod ui_composer;
use self::ui_composer::render_document_composer_content;
use self::ui_composer::COMPOSER_PROMPT_GLYPH;
#[cfg(test)]
use self::ui_composer::{
    composer_line_with_file_tags, composer_metadata_candidates, composer_viewport,
};
#[cfg(test)]
use self::ui_control_dock_disclosure::{
    composer_context_summary_candidates, startup_disclosure_candidates,
};

struct DocumentComposerRenderContext<'a> {
    dock: &'a crate::view_model::ControlDockViewModel,
    composer_lines: u16,
    disclosure_visible: bool,
}

const QUIET_SURFACE_PADDING_X: u16 = 1;
const QUIET_SURFACE_PADDING_TOP: u16 = 1;
pub(super) const fn composer_input_surface(theme: &Theme) -> Color {
    theme.reference_terminal.canvas
}

pub(super) const fn composer_input_text(theme: &Theme) -> Color {
    theme.reference_terminal.primary
}

pub(super) const fn composer_input_muted(theme: &Theme) -> Color {
    theme.text.secondary
}

pub(super) const fn composer_input_accent(theme: &Theme) -> Color {
    theme.text.accent
}

pub(super) fn composer_agent_accent(theme: &Theme, app: &AppState) -> Color {
    theme.agent_accent(app.active_profile())
}

pub(super) const fn command_palette_surface(theme: &Theme) -> Color {
    theme.surface.canvas
}

pub(super) const fn slash_command_surface(theme: &Theme) -> Color {
    theme.surface.panel_elevated
}

pub(super) const fn slash_command_selection_bg(theme: &Theme) -> Color {
    theme.text.accent
}

pub(super) const fn slash_command_selection_fg(theme: &Theme) -> Color {
    theme.text.inverse
}

pub(super) const fn command_palette_title(theme: &Theme) -> Color {
    theme.text.primary
}

pub(super) const fn command_palette_muted(theme: &Theme) -> Color {
    theme.text.tertiary
}

pub(super) const fn command_palette_section(theme: &Theme) -> Color {
    theme.reference_terminal.palette_section
}

pub(super) const fn command_palette_selection_bg(theme: &Theme) -> Color {
    theme.text.accent
}

pub(super) const fn command_palette_selection_fg(theme: &Theme) -> Color {
    theme.text.inverse
}

pub(super) const fn command_palette_cursor(theme: &Theme) -> Color {
    theme.text.primary
}

pub(super) const fn fork_selector_selection_bg(theme: &Theme) -> Color {
    theme.reference_terminal.fork_accent
}

pub(super) const fn fork_selector_selection_fg(theme: &Theme) -> Color {
    theme.text.inverse
}

pub(super) const fn fork_selector_cursor(theme: &Theme) -> Color {
    theme.reference_terminal.fork_accent
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChromeFrame {
    Frame,
}

pub(super) fn render_header(
    frame: &mut Frame,
    app: &AppState,
    plan: &FrameLayoutPlan,
    theme: &Theme,
) {
    let area = plan.header;
    let text_area = plan.header_text;
    if area.height == 0 || text_area.height == 0 {
        return;
    }

    if startup_shell_visible(app) || live_empty_state_visible(app) {
        return;
    }

    let header_text = header_identity_text(app, plan.session_contract.header_mode);
    let header_text = truncate_plain_text(&header_text, usize::from(text_area.width));

    if app.replay_mode {
        let style = Style::default()
            .fg(theme.text.secondary)
            .bg(theme.surface.shell);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(Paragraph::new(header_text).style(style), text_area);
    } else {
        frame.render_widget(
            Paragraph::new(header_text)
                .style(Style::default().fg(composer_agent_accent(theme, app))),
            text_area,
        );
    }
}

pub(super) fn render_footer(
    frame: &mut Frame,
    app: &AppState,
    plan: &FrameLayoutPlan,
    theme: &Theme,
) {
    let area = plan.footer;
    let text_area = plan.footer_text;
    if area.height == 0 || text_area.height == 0 {
        return;
    }
    if app.review_surface().is_none() && app.current_subagent_session_present() {
        if let Some(info) = app.current_subagent_session_info() {
            render_subagent_footer(frame, app, area, text_area, theme, &info);
        }
        return;
    }
    if app.replay_mode && app.review_surface().is_none() {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface.shell)),
            area,
        );
        return;
    }

    if footer_suppressed_by_overlay(app) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface.canvas)),
            area,
        );
        return;
    }

    if app.startup_shell_visible() {
        render_startup_reference_footer(frame, app, text_area, theme);
        return;
    }

    let mut footer_hints = app.footer_hints_view_model();
    // Reference idle footer shows VariantCycle/:mode and Help/:shortcuts when
    // the prompt buffer is empty (matching the pinned reference freeze).
    if !app.replay_mode
        && !app.startup_shell_visible()
        && app.composer.prompt_buffer.is_empty()
        && !app.completed_session_shell_active()
    {
        footer_hints.hints = vec![
            crate::view_model::FooterHint {
                action: crate::keybindings::Action::VariantCycle,
                label: ":mode",
            },
            crate::view_model::FooterHint {
                action: crate::keybindings::Action::Help,
                label: ":shortcuts",
            },
        ];
    } else if !app.replay_mode
        && !app.startup_shell_visible()
        && !app.composer.prompt_buffer.is_empty()
    {
        let active_turn = app.has_live_turn_activity()
            || matches!(
                app.runtime_state().kind,
                crate::app::RuntimeStateKind::Sending | crate::app::RuntimeStateKind::Streaming
            );
        footer_hints.hints = vec![
            crate::view_model::FooterHint {
                action: crate::keybindings::Action::SubmitPrompt,
                label: if active_turn { ":queue" } else { ":send" },
            },
            crate::view_model::FooterHint {
                action: crate::keybindings::Action::VariantCycle,
                label: ":mode",
            },
        ];
        if active_turn {
            footer_hints.hints.insert(
                1,
                crate::view_model::FooterHint {
                    action: crate::keybindings::Action::InsertNewline,
                    label: ":newline",
                },
            );
            footer_hints.hints.extend([crate::view_model::FooterHint {
                action: crate::keybindings::Action::DismissModal,
                label: ":cancel",
            }]);
        }
        footer_hints.hints.push(crate::view_model::FooterHint {
            action: crate::keybindings::Action::Help,
            label: ":shortcuts",
        });
    }
    match plan.session_contract.footer_mode {
        SessionFooterMode::Standard => {}
        SessionFooterMode::Reduced => {
            footer_hints.hints = compact_footer_hints(&footer_hints.hints, 4);
        }
        SessionFooterMode::Minimal => {
            footer_hints.hints = compact_footer_hints(&footer_hints.hints, 2);
            footer_hints.prefix = None;
        }
    }
    let key_style = Style::default()
        .fg(theme.reference_terminal.primary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default()
        .fg(theme.reference_terminal.secondary)
        .bg(theme.surface.canvas);
    let dim_style = Style::default()
        .fg(theme.reference_terminal.secondary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::DIM);

    let mut hint_spans: Vec<Span<'static>> = Vec::new();
    for (i, hint) in footer_hints.hints.iter().enumerate() {
        if i > 0 {
            hint_spans.push(Span::styled("  │  ", dim_style));
        }
        let key_str = app.keymap.get_binding_str(hint.action);
        if key_str != "-" {
            hint_spans.push(Span::styled(key_str, key_style));
            hint_spans.push(Span::styled(hint.label.to_string(), label_style));
        }
    }
    let hint_width: usize = hint_spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum();
    if hint_width > usize::from(text_area.width) {
        let joined: String = hint_spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        let truncated = truncate_plain_text(&joined, usize::from(text_area.width));
        hint_spans = vec![Span::styled(truncated, label_style)];
    }

    let style = label_style;

    if app.replay_mode {
        let replay_style = style.bg(theme.surface.shell);
        frame.render_widget(Block::default().style(replay_style), area);
        frame.render_widget(
            Paragraph::new(Line::from(hint_spans.clone())).style(replay_style),
            text_area,
        );
    } else {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface.canvas)),
            area,
        );
        let mut status_candidates =
            live_footer_status_candidates(app, usize::from(text_area.width), theme);
        let cluster_spans = footer_status_cluster_text(app, theme);

        if cluster_spans.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(hint_spans)).style(style),
                text_area,
            );
        } else {
            let cluster_text = cluster_spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            status_candidates.insert(0, cluster_text);
            render_live_footer_row(
                frame,
                text_area,
                style,
                status_candidates,
                Line::from(hint_spans),
            );
        }
    }
}

pub(super) fn footer_suppressed_by_overlay(app: &AppState) -> bool {
    app.review_surface().is_some()
        || app.overlay_state().command_palette_channel_visible()
        || app.overlay_state().permission_pending
}

fn render_startup_reference_footer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let area = Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(2).max(1),
        height: area.height,
    };
    let bold = Style::default()
        .fg(theme.reference_terminal.primary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::BOLD);
    let normal = Style::default()
        .fg(theme.reference_terminal.secondary)
        .bg(theme.surface.canvas);
    let dim = normal.add_modifier(Modifier::DIM);
    if !app.composer.prompt_buffer.is_empty() || app.welcome_dismissed() {
        let line = Line::from(vec![
            Span::styled("  ", normal),
            Span::styled("Enter", bold),
            Span::styled(":send", normal),
            Span::styled("  │  ", dim),
            Span::styled("Shift+Tab", bold),
            Span::styled(":mode", normal),
            Span::styled("  │  ", dim),
            Span::styled("Ctrl+x", bold),
            Span::styled(":shortcuts", normal),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let mode = app
        .launch_mode_label()
        .map(str::trim)
        .filter(|label| {
            !label.is_empty()
                && !label.eq_ignore_ascii_case("live")
                && !label.eq_ignore_ascii_case("demo")
                && !label.eq_ignore_ascii_case("mock")
        })
        .unwrap_or("Beta");
    let auth_summary = if !app.launch_metadata().has_provider() {
        "Provider not connected"
    } else if app.launch_metadata().uses_oauth_authentication() {
        "OAuth provider configured"
    } else {
        "Provider configured"
    };
    let line = Line::from(vec![
        Span::styled(auth_summary, normal),
        Span::styled("  │  ", dim),
        Span::styled(mode.to_string(), normal),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .style(Style::default().bg(theme.surface.canvas))
            .alignment(Alignment::Right),
        area,
    );
}

fn render_live_footer_row(
    frame: &mut Frame,
    area: Rect,
    style: Style,
    status_candidates: Vec<String>,
    hint_line: Line<'static>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_width = usize::from(area.width);
    let hint_width = hint_line.width();
    let status_gap = usize::from(hint_width > 0 && !status_candidates.is_empty()) * 2;
    let status_width = max_width.saturating_sub(hint_width.saturating_add(status_gap));
    let status_text = if status_width == 0 {
        String::new()
    } else {
        let first_candidate = status_candidates.first().cloned();
        status_candidates
            .into_iter()
            .find(|text| text.chars().count() <= status_width)
            .or_else(|| first_candidate.map(|text| truncate_plain_text(&text, status_width)))
            .unwrap_or_default()
    };

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(u16::try_from(hint_width.min(max_width)).unwrap_or(u16::MAX)),
            Constraint::Min(0),
        ])
        .split(area);

    if hint_width > 0 && columns[0].width > 0 {
        frame.render_widget(Paragraph::new(Text::from(hint_line)), columns[0]);
    }
    if !status_text.is_empty() && columns[1].width > 0 {
        frame.render_widget(
            Paragraph::new(status_text)
                .style(style)
                .alignment(Alignment::Right),
            columns[1],
        );
    }
}

fn compact_footer_hints(
    hints: &[crate::view_model::FooterHint],
    max_hints: usize,
) -> Vec<crate::view_model::FooterHint> {
    if hints.len() <= max_hints {
        return hints.to_vec();
    }

    if max_hints == 2 {
        let mut compact = hints
            .iter()
            .find(|hint| hint.action == Action::Palette)
            .copied()
            .or_else(|| hints.first().copied())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(last) = hints.last().copied() {
            if !compact.contains(&last) {
                compact.push(last);
            }
        }
        compact.truncate(max_hints);
        return compact;
    }

    let keep_head = max_hints.saturating_sub(1).max(1);
    let mut compact = hints.iter().take(keep_head).copied().collect::<Vec<_>>();
    if let Some(last) = hints.last().copied() {
        if !compact.contains(&last) {
            compact.push(last);
        }
    }
    compact.truncate(max_hints);
    compact
}

fn live_footer_status_candidates(app: &AppState, max_width: usize, theme: &Theme) -> Vec<String> {
    if app.replay_mode || app.startup_shell_visible() {
        return Vec::new();
    }

    let runtime_state = app.runtime_state();
    let runtime_summary = completed_session_status_summary(app, &runtime_state)
        .unwrap_or_else(|| runtime_state.summary.clone());
    let mut options = vec![
        format!("{}  ·  {}", runtime_state.kind.label(), runtime_summary),
        runtime_summary.clone(),
        runtime_state.kind.label().to_string(),
    ];
    if let Some(segment) = control_dock_summary_segment(app) {
        let segment_text = control_dock_summary_segment_text_for_width(
            &segment,
            u16::try_from(max_width).unwrap_or(u16::MAX),
            theme,
        );
        options.insert(
            0,
            format!(
                "{}  ·  {}  ·  {}",
                runtime_state.kind.label(),
                runtime_summary,
                segment_text
            ),
        );
    }

    options
}

fn footer_status_cluster_text(app: &AppState, theme: &Theme) -> Vec<Span<'static>> {
    if app.replay_mode || app.startup_shell_visible() {
        return Vec::new();
    }

    let data = crate::ui::ui_secondary::footer_status_cluster_data(app);
    let mut spans: Vec<Span<'static>> = Vec::new();

    let mut text_items: Vec<String> = Vec::new();

    if data.pending_permissions > 0 {
        text_items.push(format!("△{}", data.pending_permissions));
    }

    if data.lsp_count > 0 {
        let _dot_color = if data.lsp_has_error {
            theme.status.error
        } else if data.lsp_count > 1 {
            theme.status.warning
        } else {
            theme.status.success
        };
        text_items.push(format!("•{}", data.lsp_count));
    }

    if data.mcp_count > 0 {
        text_items.push(format!("⊙{}", data.mcp_count));
    }

    if !text_items.is_empty() {
        text_items.push("/status".to_string());
    }

    if !text_items.is_empty() {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            text_items.join(" "),
            Style::default().fg(theme.text.tertiary),
        ));
    }

    spans
}

fn header_identity_text(app: &AppState, header_mode: SessionHeaderMode) -> String {
    let run_id = app.run_id().unwrap_or("unknown");

    if app.replay_mode {
        if let Some(info) = app.current_subagent_session_info() {
            let mut title = format!("{} / {}", info.parent_label, info.title);
            if title.chars().count() > 0 {
                title = format!("{} · {title}", info.label);
            }
            return title;
        }
        let replay_identity = format!(
            "Replay · read-only · run {run_id} · {} ev",
            app.events.len()
        );
        return if app.review_surface().is_some() {
            let session_path = app
                .session_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "Replay · read-only · run {run_id} · {session_path} · {} ev",
                app.events.len()
            )
        } else {
            replay_identity
        };
    }

    if let Some(surface) = app.review_surface() {
        return format!("{} · {run_id}", surface.status_label());
    }

    let run_identity = format!("run {run_id}");
    match header_mode {
        SessionHeaderMode::Hidden => {
            let identity = format!(
                "{run_identity} · {}/{} · {}",
                app.active_profile(),
                app.active_provider(),
                app.current_model_label()
            );
            if app.continued_live_run() {
                format!("continued · {identity}")
            } else {
                identity
            }
        }
    }
}

fn control_dock_summary_segment_text_for_width(
    segment: &crate::view_model::ControlDockSummarySegment,
    width: u16,
    theme: &Theme,
) -> String {
    if width < primary_shell_context_width(theme)
        && matches!(
            segment.kind,
            crate::view_model::ControlDockSummarySegmentKind::Orchestration
        )
        && matches!(
            segment.tone,
            crate::view_model::ControlDockSummaryTone::Warning
        )
    {
        return segment
            .text
            .split(" · warn ")
            .next()
            .unwrap_or(&segment.text)
            .to_string();
    }

    segment.text.clone()
}

pub(super) fn render_unified_bottom_dock(
    frame: &mut Frame,
    app: &AppState,
    dock_layout: ControlDockLayout,
    theme: &Theme,
) {
    if dock_layout.shell.width == 0 || dock_layout.shell.height == 0 {
        return;
    }

    let dock = build_control_dock_view_model(app, theme);
    if dock.variant != crate::view_model::ControlDockVariant::Startup {
        frame.render_widget(control_dock_section(theme, dock.variant), dock_layout.shell);
        render_control_dock_top_divider(
            frame,
            Rect::new(
                dock_layout.shell.x,
                dock_layout.shell.y,
                dock_layout.shell.width,
                1,
            ),
            theme,
            dock.variant,
        );
    }

    if dock.variant == crate::view_model::ControlDockVariant::ReplayReadOnly {
        render_replay_read_only_composer_content(frame, dock_layout.composer, theme, &dock);
        return;
    }

    let active_permission = app.active_permission_view();
    if let Some(status_area) = dock_layout.status {
        if let Some(permission) = active_permission.as_ref() {
            if permission.question_prompts.is_some() {
                render_question_permission_with_shell_footer(
                    frame,
                    app,
                    status_area,
                    theme,
                    permission,
                );
            } else {
                render_inline_permission_dock(frame, app, status_area, theme, permission);
            }
        } else {
            super::ui_live_turn_status::render_live_turn_status(frame, app, status_area, theme);
        }
    }

    if let Some(disclosure_area) = dock_layout.disclosure {
        render_control_dock_disclosure(frame, disclosure_area, app, theme, &dock);
    }

    let composer_text = app.composer_render_text();
    let composer_lines = if dock.variant == crate::view_model::ControlDockVariant::Startup {
        startup_composer_input_height(
            &composer_text,
            dock_layout.composer.width,
            frame.area().height,
        )
    } else {
        composer_input_height(&composer_text, dock_layout.composer.width)
    };
    render_document_composer_content(
        frame,
        app,
        dock_layout.composer,
        theme,
        DocumentComposerRenderContext {
            dock: &dock,
            composer_lines,
            disclosure_visible: dock_layout.disclosure.is_some(),
        },
    );
}

fn render_question_permission_with_shell_footer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    permission: &crate::app::ActivePermissionView,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Reference question state: dock (11) + blank + Esc footer + trailing blank.
    const FOOTER_ROWS: u16 = 3;
    if area.height <= FOOTER_ROWS {
        render_inline_permission_dock(frame, app, area, theme, permission);
        return;
    }

    let dock_height = area.height.saturating_sub(FOOTER_ROWS);
    let dock_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: dock_height,
    };
    let footer_area = Rect {
        x: area.x,
        y: area.y.saturating_add(dock_height).saturating_add(1),
        width: area.width,
        height: 1,
    };

    render_inline_permission_dock(frame, app, dock_area, theme, permission);

    if footer_area.width == 0 || footer_area.height == 0 {
        return;
    }

    let surface = live_control_dock_surface(theme);
    let bold = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let normal = Style::default().fg(theme.text.primary).bg(surface);
    let dim = Style::default()
        .fg(theme.text.tertiary)
        .bg(surface)
        .add_modifier(Modifier::DIM);

    // Reference question-state outer shell:
    //   Esc:unselect │ Tab:scrollback │ Shift+x:dismiss
    // Harness product-honest keys (no invented Shift+x):
    //   Esc / Ctrl+c both DismissModal; Tab is FocusNext ("scrollback" elsewhere).
    let esc = preferred_binding(app, crate::keybindings::Action::DismissModal, "Esc");
    let tab = preferred_binding(app, crate::keybindings::Action::FocusNext, "Tab");
    let dismiss = preferred_binding(app, crate::keybindings::Action::DismissModal, "Ctrl+c");

    let spans = vec![
        Span::styled(esc, bold),
        Span::styled(":unselect", normal),
        Span::styled("  │  ", dim),
        Span::styled(tab, bold),
        Span::styled(":scrollback", normal),
        Span::styled("  │  ", dim),
        Span::styled(dismiss, bold),
        Span::styled(":dismiss", normal),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(surface)),
        footer_area,
    );
}

fn preferred_binding(
    app: &AppState,
    action: crate::keybindings::Action,
    preferred: &str,
) -> String {
    let bindings = app.keymap.get_binding_strs(action);
    if bindings.iter().any(|binding| binding == preferred) {
        return preferred.to_string();
    }
    bindings
        .into_iter()
        .next()
        .filter(|binding| binding != "-")
        .unwrap_or_else(|| preferred.to_string())
}

pub(super) fn panel_style(surface: Color, foreground: Color) -> Style {
    Style::default().fg(foreground).bg(surface)
}

fn framed_surface_block<'a>(
    title: impl Into<Line<'a>>,
    surface: Color,
    _border: Color,
    title_color: Color,
    _borders: Borders,
) -> Block<'a> {
    Block::default()
        .style(Style::default().bg(surface))
        .padding(Padding::new(
            QUIET_SURFACE_PADDING_X,
            QUIET_SURFACE_PADDING_X,
            QUIET_SURFACE_PADDING_TOP,
            0,
        ))
        .title(title)
        .title_style(panel_style(surface, title_color))
}

fn modal_card_surface_block<'a>(
    title: impl Into<Line<'a>>,
    surface: Color,
    border: Color,
    title_color: Color,
    _frame: ChromeFrame,
) -> Block<'a> {
    framed_surface_block(title, surface, border, title_color, Borders::NONE)
}

fn divided_surface_block<'a>(
    _theme: &Theme,
    title: impl Into<Line<'a>>,
    _divider: DividerIntensity,
    _frame: ChromeFrame,
    surface: Color,
    title_color: Color,
) -> Block<'a> {
    Block::default()
        .style(Style::default().bg(surface))
        .padding(Padding::new(
            QUIET_SURFACE_PADDING_X,
            QUIET_SURFACE_PADDING_X,
            QUIET_SURFACE_PADDING_TOP,
            0,
        ))
        .title(title)
        .title_style(panel_style(surface, title_color))
}

pub(super) fn live_transcript_shell_surface(theme: &Theme) -> Color {
    theme.surface.canvas
}

pub(super) fn live_anchor_panel_surface(theme: &Theme) -> Color {
    semantic_surface(theme, ChromeMode::Divided)
}

pub(super) fn divided_shell_surface(theme: &Theme) -> Color {
    live_anchor_panel_surface(theme)
}

pub(super) fn live_control_dock_surface(theme: &Theme) -> Color {
    theme.surface.canvas
}

pub(super) fn control_dock_surface(
    theme: &Theme,
    _variant: crate::view_model::ControlDockVariant,
) -> Color {
    theme.surface.canvas
}

pub(super) fn elevated_card_surface(theme: &Theme) -> Color {
    semantic_surface(theme, ChromeMode::Card)
}

pub(super) fn overlay_focus_row_style(_theme: &Theme) -> Style {
    Style::default()
        .fg(command_palette_selection_fg(_theme))
        .bg(command_palette_selection_bg(_theme))
}

pub(super) fn slash_command_row_style(theme: &Theme, is_selected: bool) -> Style {
    let surface = slash_command_surface(theme);
    if is_selected {
        Style::default()
            .fg(slash_command_selection_fg(theme))
            .bg(slash_command_selection_bg(theme))
    } else {
        Style::default().bg(surface)
    }
}

pub(super) fn open_canvas(surface: Color) -> Block<'static> {
    Block::default().style(Style::default().bg(surface))
}

pub(super) fn quiet_rail<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
    surface: Color,
) -> Block<'a> {
    divided_surface_block(
        theme,
        title,
        if is_focused {
            DividerIntensity::Focus
        } else {
            DividerIntensity::Subtle
        },
        ChromeFrame::Frame,
        surface,
        theme.text.secondary,
    )
}

pub(super) fn message_surface<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
    surface: Color,
) -> Block<'a> {
    divided_surface_block(
        theme,
        title,
        if is_focused {
            DividerIntensity::Focus
        } else {
            DividerIntensity::Subtle
        },
        ChromeFrame::Frame,
        surface,
        theme.text.secondary,
    )
}

pub(super) fn unified_bottom_dock(
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) -> Block<'static> {
    Block::default().style(Style::default().bg(control_dock_surface(theme, variant)))
}

pub(super) fn modal_card<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    border: Color,
    title_color: Color,
    frame: ChromeFrame,
) -> Block<'a> {
    modal_card_surface_block(
        title,
        elevated_card_surface(theme),
        border,
        title_color,
        frame,
    )
}

pub(super) fn live_transcript_shell_section(surface: Color) -> Block<'static> {
    open_canvas(surface)
}

pub(super) fn control_dock_section(
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) -> Block<'static> {
    unified_bottom_dock(theme, variant)
}

pub(super) fn secondary_pane_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
    surface: Color,
) -> Block<'a> {
    quiet_rail(theme, title, is_focused, surface)
}

pub(super) fn interruptive_modal_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    border: Color,
    title_color: Color,
    frame: ChromeFrame,
) -> Block<'a> {
    modal_card(theme, title, border, title_color, frame)
}

pub(super) fn panel_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
    surface: Color,
) -> Block<'a> {
    message_surface(theme, title, is_focused, surface)
}

fn semantic_surface(theme: &Theme, mode: ChromeMode) -> Color {
    let chrome = theme.token_families().semantic.chrome;
    match mode {
        ChromeMode::Chromeless => chrome.chromeless.surface,
        ChromeMode::Divided => chrome.divided.surface,
        ChromeMode::Card => chrome.card.surface,
    }
}

pub(super) fn muted_meta_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.secondary)
}

pub(super) fn transcript_prefix_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.tertiary)
}

pub(super) fn status_badge(label: impl Into<String>, color: Color, theme: &Theme) -> Span<'static> {
    Span::styled(
        format!(" {} ", label.into()),
        Style::default()
            .fg(theme.text.inverse)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn tool_status_summary(
    app: &AppState,
) -> Option<(String, crate::view_model::ControlDockSummaryTone)> {
    let activity = app
        .activities
        .get(app.transcript_view.selected_activity_index)?;
    let tool_calls = &activity.tool_calls;
    if tool_calls.is_empty() {
        return None;
    }

    if tool_calls.len() == 1 {
        let tool_call = &tool_calls[0];
        let (_, _, label, _) = tool_status_tokens(tool_call.status, app.theme());
        let tone = match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => {
                crate::view_model::ControlDockSummaryTone::Warning
            }
            ToolCallDisplayStatus::Queued => crate::view_model::ControlDockSummaryTone::Secondary,
            ToolCallDisplayStatus::Running => crate::view_model::ControlDockSummaryTone::Accent,
            ToolCallDisplayStatus::Succeeded => crate::view_model::ControlDockSummaryTone::Success,
            ToolCallDisplayStatus::Failed => crate::view_model::ControlDockSummaryTone::Error,
        };
        return Some((
            format!("tool {} {label}", tool_call.effective_tool_id()),
            tone,
        ));
    }

    let mut pending = 0usize;
    let mut queued = 0usize;
    let mut running = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for tool_call in tool_calls {
        match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => pending += 1,
            ToolCallDisplayStatus::Queued => queued += 1,
            ToolCallDisplayStatus::Running => running += 1,
            ToolCallDisplayStatus::Succeeded => succeeded += 1,
            ToolCallDisplayStatus::Failed => failed += 1,
        }
    }

    let mut segments = vec!["tools".to_string()];
    if running > 0 {
        segments.push(format!("{running} running"));
    }
    if pending > 0 {
        segments.push(format!("{pending} approval"));
    }
    if queued > 0 {
        segments.push(format!("{queued} queued"));
    }
    if failed > 0 {
        segments.push(format!("{failed} failed"));
    }
    if succeeded > 0 {
        segments.push(format!("{succeeded} done"));
    }

    let tone = if failed > 0 {
        crate::view_model::ControlDockSummaryTone::Error
    } else if pending > 0 {
        crate::view_model::ControlDockSummaryTone::Warning
    } else if running > 0 {
        crate::view_model::ControlDockSummaryTone::Accent
    } else if queued > 0 {
        crate::view_model::ControlDockSummaryTone::Secondary
    } else {
        crate::view_model::ControlDockSummaryTone::Success
    };

    Some((segments.join(" · "), tone))
}

pub(super) fn compact_inline_payload(payload: &str, max_chars: usize) -> Option<String> {
    crate::text::compact_payload(payload, 4, max_chars)
}

pub(super) fn tool_status_tokens(
    status: ToolCallDisplayStatus,
    theme: &Theme,
) -> (&'static str, Color, &'static str, bool) {
    match status {
        ToolCallDisplayStatus::PendingPermission => (
            theme.live_shell.glyphs.pending_permission,
            theme.status.warning,
            "pending permission",
            false,
        ),
        ToolCallDisplayStatus::Queued => (
            theme.live_shell.glyphs.queued,
            theme.text.secondary,
            "queued",
            false,
        ),
        ToolCallDisplayStatus::Running => (
            theme.live_shell.glyphs.running,
            theme.text.accent,
            "running",
            false,
        ),
        ToolCallDisplayStatus::Succeeded => (
            theme.live_shell.glyphs.succeeded,
            theme.status.success,
            "succeeded",
            true,
        ),
        ToolCallDisplayStatus::Failed => (
            theme.live_shell.glyphs.failed,
            theme.status.error,
            "failed",
            true,
        ),
    }
}

pub(super) fn runtime_state_color(kind: RuntimeStateKind, theme: &Theme) -> Color {
    match kind {
        RuntimeStateKind::Ready => theme.status.info,
        RuntimeStateKind::Success => theme.status.success,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming => theme.status.info,
        RuntimeStateKind::Cancelled
        | RuntimeStateKind::PermissionBlocked
        | RuntimeStateKind::PermissionPending
        | RuntimeStateKind::Degraded => theme.status.warning,
        RuntimeStateKind::Failure | RuntimeStateKind::Disconnected => theme.status.error,
    }
}

pub(super) fn truncate_plain_text(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let text_width = display_width(text);
    if text_width <= max_width {
        return text.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = display_width(ellipsis);
    if max_width <= ellipsis_width {
        return "…".to_string();
    }

    let truncated = take_width_prefix(text, max_width.saturating_sub(ellipsis_width));
    format!("{truncated}{ellipsis}")
}

pub(super) fn display_width(text: &str) -> usize {
    Line::from(text.to_string()).width()
}

pub(super) fn take_width_prefix(text: &str, max_width: usize) -> &str {
    if max_width == 0 {
        return "";
    }

    let mut used = 0usize;
    let mut split_at = text.len();
    for (index, ch) in text.char_indices() {
        let ch_width = Line::from(ch.to_string()).width();
        if used.saturating_add(ch_width) > max_width {
            split_at = index;
            break;
        }
        used = used.saturating_add(ch_width);
    }

    &text[..split_at]
}

fn primary_shell_context_width(theme: &Theme) -> u16 {
    theme.live_shell.breakpoints.primary.width
}

fn build_control_dock_view_model(
    app: &AppState,
    theme: &Theme,
) -> crate::view_model::ControlDockViewModel {
    if app.startup_shell_visible() {
        let mut dock = app.control_dock_view_model();
        dock.composer_disclosure = composer_shortcut_hints(app, dock.composer_disabled);
        return dock;
    }

    if app.replay_mode {
        let mut dock = app.control_dock_view_model();
        dock.composer_disclosure = replay_read_only_shortcut_hints(app);
        return dock;
    }

    if app.completed_session_shell_active() {
        let runtime_state = app.runtime_state();
        let runtime_context = Some(status_context(app, theme, runtime_state.kind).0.to_string());
        let primary_summary = completed_session_status_summary(app, &runtime_state)
            .unwrap_or_else(|| runtime_state.summary.clone());

        return crate::view_model::control_dock_view_model(
            crate::view_model::ControlDockInput::Live {
                runtime_context,
                runtime_state,
                primary_summary,
                summary_segment: control_dock_summary_segment(app),
                composer_body: String::new(),
                composer_disclosure: String::new(),
                composer_focused: app.focus == Focus::Prompt,
            },
        );
    }

    let mut dock = app.control_dock_view_model();
    dock.composer_disclosure = composer_shortcut_hints(app, dock.composer_disabled);
    dock
}

fn control_dock_summary_segment(
    app: &AppState,
) -> Option<crate::view_model::ControlDockSummarySegment> {
    if let Some(retry) = app.active_retry_metadata() {
        let text = format!("retry {}/{}", retry.attempt, retry.max_attempts);
        return Some(crate::view_model::ControlDockSummarySegment {
            kind: crate::view_model::ControlDockSummarySegmentKind::Retry,
            text,
            tone: crate::view_model::ControlDockSummaryTone::Warning,
        });
    }
    if let Some((text, tone)) = tool_status_summary(app) {
        return Some(crate::view_model::ControlDockSummarySegment {
            kind: crate::view_model::ControlDockSummarySegmentKind::Tool,
            text,
            tone,
        });
    }
    if let Some(cache_segment) = app.cache_status_summary_segment() {
        return Some(cache_segment);
    }

    let summary = app.orchestration_summary();
    let latest_warning = app.orchestration_latest_warning();
    if latest_warning.is_none()
        && summary.active_agents == 0
        && summary.queued == 0
        && summary.running == 0
        && summary.stale == 0
    {
        return None;
    }

    let mut text = format!(
        "orch {}a {}q {}r {}s",
        summary.active_agents, summary.queued, summary.running, summary.stale
    );
    let tone = if let Some(latest_warning) = latest_warning {
        text.push_str(&format!(" · warn {latest_warning}"));
        crate::view_model::ControlDockSummaryTone::Warning
    } else {
        crate::view_model::ControlDockSummaryTone::Secondary
    };
    Some(crate::view_model::ControlDockSummarySegment {
        kind: crate::view_model::ControlDockSummarySegmentKind::Orchestration,
        text,
        tone,
    })
}

fn render_control_dock_top_divider(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) {
    let surface = control_dock_surface(theme, variant);
    let _ = variant;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
}

#[cfg(test)]
mod surface_tests {
    use super::{control_dock_surface, live_control_dock_surface, live_transcript_shell_surface};
    use crate::theme::Theme;
    use crate::view_model::ControlDockVariant;

    #[test]
    fn live_control_dock_uses_themed_canvas() {
        let theme = Theme::harness_chat();

        assert_eq!(live_control_dock_surface(&theme), theme.surface.canvas);
        assert_eq!(
            live_control_dock_surface(&Theme::terminal_native()),
            ratatui::style::Color::Reset
        );
        for variant in [
            ControlDockVariant::Startup,
            ControlDockVariant::Live,
            ControlDockVariant::ReplayReadOnly,
        ] {
            assert_eq!(control_dock_surface(&theme, variant), theme.surface.canvas);
            assert_eq!(
                control_dock_surface(&Theme::terminal_native(), variant),
                ratatui::style::Color::Reset
            );
        }
        assert_eq!(live_transcript_shell_surface(&theme), theme.surface.canvas);
    }
}
