use super::*;

use ratatui::widgets::Padding;

use crate::layout::{ControlDockLayout, FrameLayoutPlan, SessionFooterMode, SessionHeaderMode};
use crate::theme::{ChromeMode, DividerIntensity};

struct DocumentComposerRenderContext<'a> {
    dock: &'a crate::view_model::ControlDockViewModel,
    composer_lines: u16,
}

const QUIET_SURFACE_PADDING_X: u16 = 1;
const QUIET_SURFACE_PADDING_TOP: u16 = 1;

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
            Paragraph::new(header_text).style(Style::default().fg(theme.text.tertiary)),
            text_area,
        );
    }
}

pub(super) fn render_live_shell_anchor(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = theme.surface.panel;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let horizontal_inset = u16::from(area.width > 2);
    let content_area = Rect::new(
        area.x.saturating_add(horizontal_inset),
        area.y,
        area.width
            .saturating_sub(horizontal_inset.saturating_mul(2)),
        1,
    );
    if content_area.width == 0 {
        return;
    }

    let dock = build_control_dock_view_model(app, theme);
    let (fallback_context, context_color) = status_context(app, theme, dock.runtime_kind);
    let run_identity = truncate_plain_text(
        &format!("run {}", app.run_id().unwrap_or("unknown")),
        usize::from(content_area.width),
    );
    let context_label = truncate_plain_text(
        dock.runtime_context.as_deref().unwrap_or(fallback_context),
        usize::from(content_area.width),
    );
    let left_width = u16::try_from(run_identity.chars().count())
        .unwrap_or(content_area.width)
        .min(content_area.width);
    let right_width = u16::try_from(context_label.chars().count())
        .unwrap_or(content_area.width)
        .min(content_area.width.saturating_sub(left_width));
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_width),
            Constraint::Min(0),
            Constraint::Length(right_width),
        ])
        .split(content_area);

    frame.render_widget(
        Paragraph::new(run_identity).style(
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );

    if sections[1].width > 0 {
        frame.render_widget(
            Paragraph::new(live_shell_anchor_metadata_text(
                app,
                usize::from(sections[1].width),
            ))
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
            sections[1],
        );
    }

    frame.render_widget(
        Paragraph::new(context_label)
            .style(
                Style::default()
                    .fg(context_color)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Right),
        sections[2],
    );
}

pub(super) fn render_footer(
    frame: &mut Frame,
    app: &AppState,
    plan: &FrameLayoutPlan,
    theme: &Theme,
) {
    let area = plan.footer;
    let text_area = plan.footer_text;
    if app.replay_mode && app.review_surface().is_none() {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface.shell)),
            area,
        );
        return;
    }

    let separator = " ".repeat(theme.live_shell.rhythm.status_separator as usize);
    let mut footer_hints = app.footer_hints_view_model();
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
    let hint_text = footer_hints
        .hints
        .iter()
        .map(|hint| app.keymap.get_binding_label(hint.action, hint.label))
        .collect::<Vec<_>>()
        .join(&separator);

    let style = Style::default().fg(theme.text.tertiary);
    let hint_text = truncate_plain_text(&hint_text, usize::from(text_area.width));

    if app.replay_mode {
        let replay_style = style.bg(theme.surface.shell);
        frame.render_widget(Block::default().style(replay_style), area);
        frame.render_widget(Paragraph::new(hint_text).style(replay_style), text_area);
    } else {
        frame.render_widget(Paragraph::new(hint_text).style(style), text_area);
    }
}

fn compact_footer_hints(
    hints: &[crate::view_model::FooterHint],
    max_hints: usize,
) -> Vec<crate::view_model::FooterHint> {
    if hints.len() <= max_hints {
        return hints.to_vec();
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

fn header_identity_text(app: &AppState, header_mode: SessionHeaderMode) -> String {
    let run_id = app.run_id().unwrap_or("unknown");

    if app.replay_mode {
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
        SessionHeaderMode::Minimal => run_identity,
        SessionHeaderMode::Compact => {
            format!(
                "{run_identity} · {}/{}",
                app.active_provider(),
                app.current_model_label()
            )
        }
        SessionHeaderMode::Hidden | SessionHeaderMode::Standard => {
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

fn live_shell_anchor_metadata_text(app: &AppState, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let variants = [
        format!(
            "{}/{}/{}",
            app.active_profile(),
            app.active_provider(),
            app.current_model_label()
        ),
        format!("{}/{}", app.active_provider(), app.current_model_label()),
        app.current_model_label().to_string(),
    ];

    variants
        .into_iter()
        .find(|variant| variant.chars().count() <= max_width)
        .unwrap_or_else(|| truncate_plain_text(app.current_model_label(), max_width))
}

fn render_control_dock_status_band(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
) {
    let (_, context_color) = status_context(app, theme, dock.runtime_kind);
    let surface = control_dock_status_surface(theme, dock.variant);
    let base_style = Style::default().fg(theme.text.secondary).bg(surface);
    frame.render_widget(control_dock_status_section(theme, dock.variant), area);
    let outcome_badge = quiet_status_badge(
        dock.runtime_badge.clone(),
        runtime_state_color(dock.runtime_kind, theme),
        surface,
    );
    let mut spans: Vec<Span<'static>> = Vec::new();

    if dock.variant == crate::view_model::ControlDockVariant::Startup {
        spans.push(Span::styled("  ", base_style));
        spans.push(Span::styled(dock.primary_summary.clone(), base_style));
        let status_line = Line::from(spans);
        frame.render_widget(Paragraph::new(status_line).style(base_style), area);
        return;
    }

    if area.width >= primary_shell_context_width(theme) {
        if let Some(context_label) = dock.runtime_context.as_deref() {
            spans.push(quiet_status_badge(context_label, context_color, surface));
            spans.push(Span::styled("  ", base_style));
        }
    }

    spans.push(outcome_badge);
    spans.push(Span::styled("  ", base_style));

    spans.push(Span::styled(dock.primary_summary.clone(), base_style));

    append_control_dock_summary_segment(&mut spans, area.width, &dock, surface, theme);

    let status_line = Line::from(spans);

    frame.render_widget(Paragraph::new(status_line).style(base_style), area);
}

fn append_control_dock_summary_segment(
    spans: &mut Vec<Span<'static>>,
    width: u16,
    dock: &crate::view_model::ControlDockViewModel,
    surface: Color,
    theme: &Theme,
) {
    let Some(summary_segment) = dock.summary_segment.as_ref() else {
        return;
    };

    let separator = "  ·  ";
    let summary_text = control_dock_summary_segment_text_for_width(summary_segment, width, theme);
    let available = usize::from(width)
        .saturating_sub(status_strip_width(spans))
        .saturating_sub(separator.chars().count());
    if available <= 10 {
        return;
    }

    spans.push(Span::styled(
        separator,
        Style::default().fg(theme.text.secondary).bg(surface),
    ));
    spans.push(Span::styled(
        truncate_plain_text(&summary_text, available),
        control_dock_summary_segment_style(summary_segment, surface, theme),
    ));
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

fn control_dock_summary_segment_style(
    segment: &crate::view_model::ControlDockSummarySegment,
    surface: Color,
    theme: &Theme,
) -> Style {
    let foreground = match segment.tone {
        crate::view_model::ControlDockSummaryTone::Secondary => theme.text.secondary,
        crate::view_model::ControlDockSummaryTone::Accent => theme.text.accent,
        crate::view_model::ControlDockSummaryTone::Success => theme.status.success,
        crate::view_model::ControlDockSummaryTone::Warning => theme.status.warning,
        crate::view_model::ControlDockSummaryTone::Error => theme.status.error,
    };
    let style = Style::default().fg(foreground).bg(surface);
    if matches!(
        segment.tone,
        crate::view_model::ControlDockSummaryTone::Success
            | crate::view_model::ControlDockSummaryTone::Warning
            | crate::view_model::ControlDockSummaryTone::Error
    ) {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn status_strip_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
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

    if let Some(status_area) = dock_layout.status {
        render_control_dock_status_band(frame, app, status_area, theme, &dock);
    }

    if dock.variant == crate::view_model::ControlDockVariant::ReplayReadOnly {
        render_replay_read_only_composer_content(frame, dock_layout.composer, theme, &dock);
        return;
    }

    let composer_lines = composer_input_height(&app.prompt_buffer, dock_layout.composer.width);
    render_document_composer_content(
        frame,
        app,
        dock_layout.composer,
        theme,
        DocumentComposerRenderContext {
            dock: &dock,
            composer_lines,
        },
    );
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
    semantic_surface(theme, ChromeMode::Chromeless)
}

pub(super) fn chromeless_shell_surface(theme: &Theme) -> Color {
    live_transcript_shell_surface(theme)
}

pub(super) fn live_anchor_panel_surface(theme: &Theme) -> Color {
    semantic_surface(theme, ChromeMode::Divided)
}

pub(super) fn divided_shell_surface(theme: &Theme) -> Color {
    live_anchor_panel_surface(theme)
}

pub(super) fn live_control_dock_surface(theme: &Theme) -> Color {
    semantic_surface(theme, ChromeMode::Divided)
}

pub(super) fn control_dock_surface(
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) -> Color {
    match variant {
        crate::view_model::ControlDockVariant::Startup => chromeless_shell_surface(theme),
        crate::view_model::ControlDockVariant::Live => live_control_dock_surface(theme),
        crate::view_model::ControlDockVariant::ReplayReadOnly => live_control_dock_surface(theme),
    }
}

pub(super) fn elevated_card_surface(theme: &Theme) -> Color {
    semantic_surface(theme, ChromeMode::Card)
}

pub(super) fn quiet_modal_backdrop_surface(theme: &Theme) -> Color {
    chromeless_shell_surface(theme)
}

pub(super) fn overlay_focus_row_style(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.text.inverse)
        .bg(theme.border.focus)
}

pub(super) fn open_canvas(theme: &Theme) -> Block<'static> {
    Block::default().style(Style::default().bg(live_transcript_shell_surface(theme)))
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

#[allow(dead_code)]
pub(super) fn elevated_card_block<'a>(
    title: impl Into<Line<'a>>,
    surface: Color,
    border: Color,
    title_color: Color,
) -> Block<'a> {
    modal_card_surface_block(title, surface, border, title_color, ChromeFrame::Frame)
}

pub(super) fn live_transcript_shell_section(theme: &Theme) -> Block<'static> {
    open_canvas(theme)
}

#[allow(dead_code)]
pub(super) fn live_control_dock_section(theme: &Theme) -> Block<'static> {
    control_dock_section(theme, crate::view_model::ControlDockVariant::Live)
}

pub(super) fn control_dock_section(
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) -> Block<'static> {
    unified_bottom_dock(theme, variant)
}

pub(super) fn control_dock_status_section(
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) -> Block<'static> {
    Block::default().style(Style::default().bg(control_dock_status_surface(theme, variant)))
}

pub(super) fn control_dock_status_surface(
    theme: &Theme,
    variant: crate::view_model::ControlDockVariant,
) -> Color {
    control_dock_surface(theme, variant)
}

pub(super) fn chromeless_shell_section(theme: &Theme) -> Block<'static> {
    live_transcript_shell_section(theme)
}

#[allow(dead_code)]
pub(super) fn divided_shell_section<'a>(
    _theme: &Theme,
    title: impl Into<Line<'a>>,
    divider: DividerIntensity,
    frame: ChromeFrame,
    surface: Color,
) -> Block<'a> {
    divided_surface_block(
        _theme,
        title,
        divider,
        frame,
        surface,
        _theme.text.secondary,
    )
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

pub(super) fn request_id_label(request_id: &str) -> Cow<'_, str> {
    if request_id.is_empty() {
        Cow::Borrowed("pending turn")
    } else {
        Cow::Borrowed(request_id)
    }
}

pub(super) fn transcript_label_style(theme: &Theme, is_selected: bool) -> Style {
    let color = if is_selected {
        theme.text.accent
    } else {
        theme.text.primary
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn muted_meta_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.secondary)
}

pub(super) fn subdued_payload_style(theme: &Theme) -> Style {
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

fn quiet_status_badge(label: impl Into<String>, color: Color, surface: Color) -> Span<'static> {
    Span::styled(
        format!(" {} ", label.into()),
        Style::default()
            .fg(color)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    )
}

pub(super) fn tool_detail_label_style(
    label: &str,
    theme: &Theme,
    status: ToolCallDisplayStatus,
) -> Style {
    let color = match label {
        "state" => match status {
            ToolCallDisplayStatus::PendingPermission => theme.status.warning,
            ToolCallDisplayStatus::Queued => theme.text.secondary,
            ToolCallDisplayStatus::Running => theme.text.accent,
            ToolCallDisplayStatus::Succeeded => theme.status.success,
            ToolCallDisplayStatus::Failed => theme.status.error,
        },
        "result" => theme.status.success,
        "error" => theme.status.error,
        _ => theme.text.secondary,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn tool_state_summary(tool_call: &crate::app::ToolCallEntry) -> Option<&'static str> {
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission => Some("awaiting approval before execution"),
        ToolCallDisplayStatus::Queued => Some("waiting for execution"),
        ToolCallDisplayStatus::Running => Some("running…"),
        ToolCallDisplayStatus::Succeeded if tool_call.truncated_output.is_none() => {
            Some("completed without output")
        }
        ToolCallDisplayStatus::Failed if tool_call.truncated_output.is_none() => {
            Some("failed without error payload")
        }
        _ => None,
    }
}

fn tool_status_summary(
    app: &AppState,
) -> Option<(String, crate::view_model::ControlDockSummaryTone)> {
    let activity = app.activities.get(app.selected_activity_index)?;
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
        return Some((format!("tool {} {label}", tool_call.tool_id), tone));
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
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let collapsed = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => compact_inline_json_value(&value),
        Err(_) => trimmed.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    if collapsed.chars().count() <= max_chars {
        return Some(collapsed);
    }

    let truncated = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    Some(format!("{truncated}…"))
}

fn compact_inline_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }

            let mut parts = Vec::new();
            for (key, value) in map.iter().take(4) {
                parts.push(format!("{key}={}", compact_inline_json_leaf(value)));
            }
            if map.len() > 4 {
                parts.push("…".to_string());
            }
            parts.join(", ")
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }

            let mut parts = items
                .iter()
                .take(4)
                .map(compact_inline_json_leaf)
                .collect::<Vec<_>>();
            if items.len() > 4 {
                parts.push("…".to_string());
            }
            format!("[{}]", parts.join(", "))
        }
        _ => compact_inline_json_leaf(value),
    }
}

fn compact_inline_json_leaf(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.split_whitespace().collect::<Vec<_>>().join(" "),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(items) => format!(
            "[{} item{}]",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        serde_json::Value::Object(fields) => format!(
            "{{{} field{}}}",
            fields.len(),
            if fields.len() == 1 { "" } else { "s" }
        ),
    }
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

    let text_width = text.chars().count();
    if text_width <= max_width {
        return text.to_string();
    }

    if max_width == 1 {
        return "…".to_string();
    }

    let truncated = text
        .chars()
        .take(max_width.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}

fn primary_shell_context_width(theme: &Theme) -> u16 {
    theme.live_shell.breakpoints.primary.width
}

fn build_control_dock_view_model(
    app: &AppState,
    theme: &Theme,
) -> crate::view_model::ControlDockViewModel {
    let runtime_state = app.runtime_state();
    let runtime_context = Some(status_context(app, theme, runtime_state.kind).0.to_string());
    let primary_summary = completed_session_status_summary(app, &runtime_state)
        .unwrap_or_else(|| runtime_state.summary.clone());

    if app.startup_shell_visible() {
        let composer_disabled = runtime_state.composer_disabled;
        let composer_focused = app.focus == Focus::Prompt;
        let composer_body = if app.prompt_buffer.is_empty() {
            runtime_state.composer_hint.clone()
        } else {
            visible_composer_text(app, composer_focused && !composer_disabled)
        };

        return crate::view_model::control_dock_view_model(
            crate::view_model::ControlDockInput::Startup {
                runtime_context,
                runtime_state,
                primary_summary,
                composer_body,
                composer_disclosure: composer_shortcut_hints(app, composer_disabled),
                composer_focused,
            },
        );
    }

    if app.replay_mode {
        return crate::view_model::control_dock_view_model(
            crate::view_model::ControlDockInput::ReplayReadOnly {
                runtime_context,
                runtime_state,
                primary_summary,
                composer_body: "Replay is read-only.".to_string(),
                composer_disclosure: replay_read_only_shortcut_hints(app),
                composer_focused: app.focus == Focus::Prompt,
            },
        );
    }

    let composer_disabled = runtime_state.composer_disabled;
    let composer_focused = app.focus == Focus::Prompt;
    let composer_body = if app.prompt_buffer.is_empty() {
        if composer_disabled {
            runtime_state.composer_hint.clone()
        } else {
            composer_placeholder_voice(app, &runtime_state)
        }
    } else {
        visible_composer_text(app, composer_focused && !composer_disabled)
    };

    crate::view_model::control_dock_view_model(crate::view_model::ControlDockInput::Live {
        runtime_context,
        runtime_state,
        primary_summary,
        summary_segment: control_dock_summary_segment(app),
        composer_body,
        composer_disclosure: composer_shortcut_hints(app, composer_disabled),
        composer_focused,
    })
}

fn control_dock_summary_segment(
    app: &AppState,
) -> Option<crate::view_model::ControlDockSummarySegment> {
    if let Some((text, tone)) = tool_status_summary(app) {
        return Some(crate::view_model::ControlDockSummarySegment {
            kind: crate::view_model::ControlDockSummarySegmentKind::Tool,
            text,
            tone,
        });
    }

    let summary = app.orchestration_summary();
    let latest_warning = app.orchestration_latest_warning();
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

fn render_document_composer_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    let surface = control_dock_surface(theme, context.dock.variant);
    let composer_surface = theme.surface.panel_elevated;
    let card_area = inset_rect(area, u16::from(area.width > 6), 0);
    let composer_block = Block::default()
        .style(Style::default().bg(composer_surface))
        .padding(Padding::new(
            theme.live_shell.rhythm.composer_padding_x,
            theme.live_shell.rhythm.composer_padding_x,
            0,
            0,
        ));
    let content_area = composer_block.inner(card_area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    frame.render_widget(composer_block, card_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let hint_visible = content_area.height > context.composer_lines
        && area.width > crate::theme::ShellGeometry::MINIMUM.width;
    let input_height = context
        .composer_lines
        .min(
            content_area
                .height
                .saturating_sub(u16::from(hint_visible))
                .max(1),
        )
        .max(1);
    let top_gap = content_area
        .height
        .saturating_sub(input_height)
        .saturating_sub(u16::from(hint_visible));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_gap),
            Constraint::Length(input_height),
            Constraint::Length(u16::from(hint_visible)),
        ])
        .split(content_area);

    let input_width = usize::from(rows[1].width);
    let body = context.dock.composer_body.clone();
    let body_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if app.prompt_buffer.is_empty() {
        theme.text.secondary
    } else {
        theme.text.primary
    };
    let rail_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if context.dock.composer_focused {
        theme.text.accent
    } else {
        theme.text.secondary
    };
    let rail = "▎ ";
    let body_lines = composer_body_lines(
        &body,
        input_width.saturating_sub(rail.chars().count()),
        usize::from(rows[1].height.max(1)),
    )
    .into_iter()
    .map(|line| {
        Line::from(vec![
            Span::styled(rail, Style::default().fg(rail_color).bg(composer_surface)),
            Span::styled(line, Style::default().fg(body_color).bg(composer_surface)),
        ])
    })
    .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body_lines).style(Style::default().bg(composer_surface)),
        rows[1],
    );

    if hint_visible && rows[2].height > 0 {
        let hint_prefix = "  ";
        let footer = truncate_plain_text(
            &context.dock.composer_disclosure,
            usize::from(rows[2].width).saturating_sub(hint_prefix.chars().count()),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(hint_prefix, Style::default().bg(composer_surface)),
                Span::styled(
                    footer,
                    Style::default()
                        .fg(theme.text.secondary)
                        .bg(composer_surface),
                ),
            ])),
            rows[2],
        );
    }
}

fn composer_body_lines(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }

    if width == 0 {
        return vec![String::new()];
    }

    let logical_lines = text
        .split('\n')
        .map(|line| line.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut visual_lines = Vec::new();
    let mut truncated = false;

    'outer: for (line_index, chars) in logical_lines.iter().enumerate() {
        if chars.is_empty() {
            visual_lines.push(String::new());
            if visual_lines.len() == max_lines {
                truncated = line_index + 1 < logical_lines.len();
                break;
            }
            continue;
        }

        let mut start = 0usize;
        while start < chars.len() {
            if visual_lines.len() == max_lines {
                truncated = true;
                break 'outer;
            }

            let end = (start + width).min(chars.len());
            visual_lines.push(chars[start..end].iter().collect::<String>());
            start = end;
        }

        if start < chars.len() {
            truncated = true;
            break;
        }
        if visual_lines.len() == max_lines && line_index + 1 < logical_lines.len() {
            truncated = true;
            break;
        }
    }

    if visual_lines.is_empty() {
        visual_lines.push(String::new());
    }

    if truncated {
        let last = visual_lines.pop().unwrap_or_default();
        visual_lines.push(ellipsize_composer_line(&last, width));
    }

    visual_lines
}

fn ellipsize_composer_line(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_string();
    }

    format!("{}…", truncate_plain_text(text, width.saturating_sub(1)))
}

fn composer_placeholder_voice(app: &AppState, runtime_state: &crate::app::RuntimeState) -> String {
    if app.completed_session_shell_active() {
        if runtime_state.kind == RuntimeStateKind::Failure {
            return format!(
                "Run failed — inspect transcript, then adjust the draft from {} or a follow-up run…",
                app.keymap
                    .get_binding_label(Action::Palette, "commands")
                    .to_ascii_lowercase()
            );
        }

        return format!(
            "Run complete — use {} for replay/new/quit, or ask Harness for the next step…",
            app.keymap
                .get_binding_label(Action::Palette, "commands")
                .to_ascii_lowercase()
        );
    }

    match runtime_state.kind {
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming => {
            "Queue the next turn while this one finishes…".to_string()
        }
        RuntimeStateKind::Failure | RuntimeStateKind::Cancelled => {
            "Ask Harness what to retry, inspect, or fix…".to_string()
        }
        _ => "Ask Harness to inspect, edit, or explain…".to_string(),
    }
}

fn visible_composer_text(app: &AppState, show_cursor: bool) -> String {
    let mut text = app.prompt_buffer.clone();
    if show_cursor {
        let cursor_byte_pos = app
            .prompt_buffer
            .char_indices()
            .nth(app.prompt_cursor)
            .map(|(i, _)| i)
            .unwrap_or(app.prompt_buffer.len());
        text.insert(cursor_byte_pos, '█');
    }
    text
}

fn composer_shortcut_hints(app: &AppState, composer_disabled: bool) -> String {
    if composer_disabled || app.completed_session_shell_active() {
        return app
            .keymap
            .get_binding_label(Action::Palette, "commands")
            .to_ascii_lowercase();
    }

    app.keymap
        .get_binding_label(Action::InsertNewline, "newline")
        .to_ascii_lowercase()
}

fn status_context(app: &AppState, theme: &Theme, state: RuntimeStateKind) -> (&'static str, Color) {
    if app.startup_shell_visible() {
        return ("startup", theme.text.secondary);
    }
    if app.completed_session_shell_active() {
        return match state {
            RuntimeStateKind::Failure => ("recovery", theme.status.warning),
            _ => ("complete", theme.status.success),
        };
    }
    if app.replay_mode {
        return ("replay", theme.text.secondary);
    }
    if app.details_drawer_open() {
        return ("details", theme.border.focus);
    }
    if matches!(
        state,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming
    ) {
        return ("live", theme.text.accent);
    }
    ("live", theme.text.secondary)
}

fn render_replay_read_only_composer_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
) {
    let surface = control_dock_surface(theme, dock.variant);

    let content_area = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    if content_area.height == 0 {
        return;
    }

    let hint_visible = content_area.height > 1;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(content_area);

    let rail = "▎ ";
    let body = truncate_plain_text(
        &dock.composer_body,
        usize::from(rows[0].width).saturating_sub(rail.chars().count()),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(rail, Style::default().fg(theme.status.disabled).bg(surface)),
            Span::styled(body, Style::default().fg(theme.status.disabled).bg(surface)),
        ])),
        rows[0],
    );

    if hint_visible && rows[1].height > 0 {
        let hint_prefix = "  ";
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(hint_prefix, Style::default().bg(surface)),
                Span::styled(
                    truncate_plain_text(
                        &dock.composer_disclosure,
                        usize::from(rows[1].width).saturating_sub(hint_prefix.chars().count()),
                    ),
                    Style::default().fg(theme.text.secondary).bg(surface),
                ),
            ])),
            rows[1],
        );
    }
}

fn replay_read_only_shortcut_hints(app: &AppState) -> String {
    [
        app.keymap
            .get_binding_label(Action::Help, "shortcuts")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::FocusNext, "focus")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::Reload, "reload")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::Quit, "quit")
            .to_ascii_lowercase(),
    ]
    .join("  ·  ")
}

fn completed_session_status_summary(
    app: &AppState,
    state: &crate::app::RuntimeState,
) -> Option<String> {
    if !app.completed_session_shell_active() || app.replay_mode {
        return None;
    }

    Some(match state.kind {
        RuntimeStateKind::Failure => {
            "run failed · inspect transcript · session shell preserved".to_string()
        }
        _ => "run finished · session shell preserved".to_string(),
    })
}

#[cfg(test)]
pub(crate) fn exact_test_live_control_dock_renders_shared_surface() {
    use ratatui::{backend::TestBackend, Terminal};

    let app = AppState::new_live(None, false, None);
    let theme = Theme::default();
    let width = 100;
    let height = 30;
    let area = Rect::new(0, 0, width, height);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let dock = plan.dock.expect("live dock layout");
    let status = dock.status.expect("live status area");
    let composer = dock.composer;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create live shell terminal");
    terminal
        .draw(|frame| super::render_app(frame, &app))
        .expect("draw live shell frame");

    let buffer = terminal.backend().buffer().clone();
    let right_edge = dock
        .shell
        .x
        .saturating_add(dock.shell.width.saturating_sub(1));

    assert_eq!(buffer[(right_edge, status.y)].bg, theme.surface.panel);
    assert_eq!(buffer[(right_edge, composer.y)].bg, theme.surface.panel);
    assert_eq!(
        buffer[(right_edge, composer.y.saturating_add(1))].bg,
        theme.surface.panel
    );
    assert_eq!(buffer[(right_edge, dock.shell.y)].bg, theme.surface.panel);
    assert_ne!(
        buffer[(right_edge, dock.shell.y)].symbol(),
        "─",
        "quiet dock chrome should rely on surface spacing instead of a hard divider"
    );
    assert_eq!(
        buffer[(
            right_edge,
            composer.y.saturating_add(composer.height.saturating_sub(1)),
        )]
            .bg,
        theme.surface.panel
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_control_dock_collapses_disclosure_before_status() {
    use ratatui::{backend::TestBackend, Terminal};

    let app = AppState::new_live(None, false, None);
    let width = 80;
    let height = 24;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create live shell terminal");
    terminal
        .draw(|frame| super::render_app(frame, &app))
        .expect("draw live shell frame");

    let rendered = terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let lines = rendered.lines().collect::<Vec<_>>();
    let status_row = lines
        .iter()
        .position(|line| line.contains("ready for first turn"))
        .expect("status row");
    let composer_row = lines
        .iter()
        .enumerate()
        .skip(status_row + 1)
        .find_map(|(index, line)| {
            line.contains("▎ Ask Harness to inspect, edit, or explain…")
                .then_some(index)
        })
        .expect("composer row");
    let footer_row = lines
        .iter()
        .enumerate()
        .skip(composer_row + 1)
        .find_map(|(index, line)| line.contains("Shift+Enter nl").then_some(index))
        .expect("footer row");

    assert!(
        status_row < composer_row,
        "dock header should survive constrained layout\n{rendered}"
    );
    assert_eq!(
        footer_row,
        composer_row + 1,
        "tight live shells should drop the dock disclosure before the dock header\n{rendered}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("shift+enter newline")),
        "tight live shells should collapse the local disclosure row\n{rendered}"
    );
}
