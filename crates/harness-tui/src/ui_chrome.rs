use super::*;

use ratatui::widgets::Padding;

use crate::layout::{ControlDockLayout, FrameLayoutPlan, SessionFooterMode, SessionHeaderMode};
use crate::theme::{ChromeMode, DividerIntensity};

struct DocumentComposerRenderContext<'a> {
    dock: &'a crate::view_model::ControlDockViewModel,
    composer_lines: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerViewport {
    lines: Vec<String>,
    cursor: Option<(usize, usize)>,
}

const QUIET_SURFACE_PADDING_X: u16 = 1;
const QUIET_SURFACE_PADDING_TOP: u16 = 1;
const COMPOSER_RAIL_GLYPH: &str = "┃";
const COMPOSER_RAIL_CAP_GLYPH: &str = "╹";

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
        render_live_footer_row(
            frame,
            text_area,
            style,
            live_footer_status_candidates(app, usize::from(text_area.width), theme),
            hint_text,
        );
    }
}

fn render_live_footer_row(
    frame: &mut Frame,
    area: Rect,
    style: Style,
    status_candidates: Vec<String>,
    hint_text: String,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_width = usize::from(area.width);
    let hint_width = hint_text.chars().count();
    let status_gap = usize::from(hint_width > 0 && !status_candidates.is_empty()) * 2;
    let status_width = max_width.saturating_sub(hint_width.saturating_add(status_gap));
    let status_text = if status_width == 0 {
        String::new()
    } else {
        status_candidates
            .into_iter()
            .find(|text| text.chars().count() <= status_width)
            .unwrap_or_default()
    };

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(u16::try_from(hint_width.min(max_width)).unwrap_or(u16::MAX)),
        ])
        .split(area);

    if !status_text.is_empty() && columns[0].width > 0 {
        frame.render_widget(Paragraph::new(status_text).style(style), columns[0]);
    }
    if !hint_text.is_empty() && columns[1].width > 0 {
        frame.render_widget(
            Paragraph::new(hint_text)
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
        SessionHeaderMode::Hidden
        | SessionHeaderMode::Standard
        | SessionHeaderMode::Compact
        | SessionHeaderMode::Minimal => {
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

fn render_control_dock_status_band(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
    disclosure_visible: bool,
) {
    let surface = control_dock_status_surface(theme, dock.variant);
    let base_style = Style::default().fg(theme.text.secondary).bg(surface);
    frame.render_widget(control_dock_status_section(theme, dock.variant), area);
    let status_candidates = if dock.variant == crate::view_model::ControlDockVariant::Startup {
        Vec::new()
    } else {
        live_footer_status_candidates(app, usize::from(area.width), theme)
    };

    let (status_text, hint_text) = control_dock_row_content(
        app,
        usize::from(area.width),
        theme,
        status_candidates,
        disclosure_visible,
    );
    render_live_footer_row(
        frame,
        area,
        base_style,
        (!status_text.is_empty())
            .then_some(status_text)
            .into_iter()
            .collect(),
        hint_text,
    );
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
        render_control_dock_status_band(
            frame,
            app,
            status_area,
            theme,
            &dock,
            dock_layout.disclosure.is_some(),
        );
    }

    if dock.variant == crate::view_model::ControlDockVariant::ReplayReadOnly {
        render_replay_read_only_composer_content(frame, app, dock_layout.composer, theme, &dock);
        return;
    }

    if let Some(disclosure_area) = dock_layout.disclosure {
        render_control_dock_disclosure(frame, disclosure_area, app, theme, &dock);
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
    theme.surface.shell
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
    if let Some((text, tone)) = tool_status_summary(app) {
        return Some(crate::view_model::ControlDockSummarySegment {
            kind: crate::view_model::ControlDockSummarySegmentKind::Tool,
            text,
            tone,
        });
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

fn render_document_composer_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    let surface = control_dock_surface(theme, context.dock.variant);
    let active_composer_surface = theme.token_families().semantic.composer.primary.surface;
    let composer_surface = active_composer_surface;
    let prompt_area = area;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    if prompt_area.width == 0 || prompt_area.height == 0 {
        return;
    }

    let main_columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(prompt_area);
    let rail_area = main_columns[0];
    let body_area = main_columns[1];

    let shell_rows = if body_area.height > 1 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(body_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(body_area)
    };
    let composer_body_area = shell_rows[0];
    let composer_cap_area = shell_rows[1];

    frame.render_widget(
        Block::default().style(Style::default().bg(composer_surface)),
        composer_body_area,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }
    if composer_body_area.height == 1 {
        let body = wrap_composer_lines(
            &context.dock.composer_body,
            usize::from(composer_body_area.width),
        )
        .into_iter()
        .take(1)
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default()
                    .fg(theme.text.secondary)
                    .bg(composer_surface),
            ))
        })
        .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(body).style(Style::default().bg(composer_surface)),
            composer_body_area,
        );
        return;
    }

    let body_inner = inset_rect(
        composer_body_area,
        theme
            .live_shell
            .rhythm
            .composer_padding_x
            .min(composer_body_area.width.saturating_sub(1)),
        0,
    );
    if body_inner.width == 0 || body_inner.height == 0 {
        return;
    }

    let metadata_height = u16::from(body_inner.height >= 2);
    let metadata_gap = u16::from(metadata_height > 0 && body_inner.height >= 3);
    let top_padding = u16::from(
        metadata_height > 0
            && body_inner.height
                >= metadata_height
                    .saturating_add(metadata_gap)
                    .saturating_add(2),
    );
    let available_input_height = body_inner
        .height
        .saturating_sub(metadata_height)
        .saturating_sub(top_padding)
        .saturating_sub(metadata_gap)
        .max(1);
    let input_height = context
        .composer_lines
        .clamp(1, available_input_height)
        .max(1);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_padding),
            Constraint::Length(input_height),
            Constraint::Length(metadata_gap),
            Constraint::Length(metadata_height),
            Constraint::Min(0),
        ])
        .split(body_inner);
    let input_area = rows[1];
    let input_width = usize::from(input_area.width);
    let placeholder_visible = app.prompt_buffer.is_empty()
        && matches!(
            context.dock.variant,
            crate::view_model::ControlDockVariant::Startup
        );
    let body = if placeholder_visible {
        context.dock.composer_body.as_str()
    } else {
        app.prompt_buffer.as_str()
    };
    let body_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if placeholder_visible {
        theme.text.secondary
    } else {
        theme.text.primary
    };
    let rail_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else {
        theme.text.accent
    };

    if rail_area.height > 0 {
        let body_rows = usize::from(rail_area.height.saturating_sub(composer_cap_area.height));
        let mut rail_lines = vec![
            Line::from(Span::styled(
                COMPOSER_RAIL_GLYPH,
                Style::default().fg(rail_color).bg(surface),
            ));
            body_rows
        ];
        if composer_cap_area.height > 0 {
            rail_lines.push(Line::from(Span::styled(
                COMPOSER_RAIL_CAP_GLYPH,
                Style::default().fg(rail_color).bg(surface),
            )));
        }
        if rail_lines.is_empty() {
            rail_lines.push(Line::from(Span::styled(
                COMPOSER_RAIL_GLYPH,
                Style::default().fg(rail_color).bg(surface),
            )));
        }
        frame.render_widget(
            Paragraph::new(rail_lines).style(Style::default().bg(surface)),
            rail_area,
        );
    }
    if composer_cap_area.height > 0 && composer_cap_area.width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "▀".repeat(usize::from(composer_cap_area.width)),
                Style::default().fg(composer_surface).bg(surface),
            )))
            .style(Style::default().bg(surface)),
            composer_cap_area,
        );
    }

    let viewport = composer_viewport(
        body,
        input_width,
        usize::from(input_area.height.max(1)),
        (context.dock.composer_focused && !context.dock.composer_disabled).then_some(
            if placeholder_visible {
                0
            } else {
                app.prompt_cursor
            },
        ),
    );
    let body_lines = viewport
        .lines
        .iter()
        .cloned()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default().fg(body_color).bg(composer_surface),
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body_lines).style(Style::default().bg(composer_surface)),
        input_area,
    );

    if let Some((cursor_row, cursor_col)) = viewport.cursor {
        let cursor_x = input_area
            .x
            .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
        let cursor_y = input_area
            .y
            .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX));
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    if metadata_height > 0 && rows[3].width > 0 {
        frame.render_widget(
            Paragraph::new(composer_metadata_line(
                app,
                context.dock,
                usize::from(rows[3].width),
                theme,
                composer_surface,
            ))
            .style(Style::default().bg(composer_surface)),
            rows[3],
        );
    }

    if rows[4].height > 0 && rows[4].width > 0 {
        if let Some(status_model) = live_composer_status_view_model(app, context.dock) {
            let status_area = Rect::new(rows[4].x, rows[4].y, rows[4].width, 1);
            frame.render_widget(
                Paragraph::new(live_composer_status_line(
                    &status_model,
                    usize::from(status_area.width),
                    theme,
                    composer_surface,
                ))
                .style(Style::default().bg(composer_surface)),
                status_area,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerMetadataTone {
    Accent,
    Primary,
    Secondary,
    Tertiary,
}

fn composer_metadata_line(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
    theme: &Theme,
    surface: Color,
) -> Line<'static> {
    let candidates = composer_metadata_candidates(app, dock);
    let segments = candidates
        .iter()
        .find(|segments| composer_metadata_segments_width(segments) <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            vec![(
                truncate_plain_text(&composer_metadata_text(app, dock, max_width), max_width),
                ComposerMetadataTone::Secondary,
            )]
        });

    Line::from(
        segments
            .into_iter()
            .map(|(text, tone)| {
                Span::styled(
                    text,
                    Style::default()
                        .fg(composer_metadata_color(tone, theme))
                        .bg(surface),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn live_composer_status_line(
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
    theme: &Theme,
    surface: Color,
) -> Line<'static> {
    let candidates = live_composer_status_candidates(dock, max_width, theme);
    let segments = candidates
        .iter()
        .find(|segments| composer_metadata_segments_width(segments) <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            vec![(
                truncate_plain_text(
                    &live_composer_status_text(dock, max_width, theme),
                    max_width,
                ),
                ComposerMetadataTone::Secondary,
            )]
        });

    Line::from(
        segments
            .into_iter()
            .map(|(text, tone)| {
                Span::styled(
                    text,
                    Style::default()
                        .fg(composer_metadata_color(tone, theme))
                        .bg(surface),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn live_composer_status_candidates(
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
    theme: &Theme,
) -> Vec<Vec<(String, ComposerMetadataTone)>> {
    let separator = ("  ·  ".to_string(), ComposerMetadataTone::Tertiary);
    let badge_tone = runtime_kind_metadata_tone(dock.runtime_kind);
    let summary_tone = dock
        .summary_segment
        .as_ref()
        .map(|segment| control_dock_summary_tone_to_metadata_tone(segment.tone));
    let summary_text = dock.summary_segment.as_ref().map(|segment| {
        control_dock_summary_segment_text_for_width(
            segment,
            u16::try_from(max_width).unwrap_or(u16::MAX),
            theme,
        )
    });

    let mut full = Vec::new();
    if let Some(context) = dock
        .runtime_context
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        full.push((context.clone(), ComposerMetadataTone::Secondary));
        full.push(separator.clone());
    }
    full.push((dock.runtime_badge.clone(), badge_tone));
    full.push(separator.clone());
    full.push((
        dock.primary_summary.clone(),
        ComposerMetadataTone::Secondary,
    ));
    if let Some(text) = summary_text.as_ref() {
        full.push(separator.clone());
        full.push((
            text.clone(),
            summary_tone.unwrap_or(ComposerMetadataTone::Secondary),
        ));
    }

    let mut without_context = vec![
        (dock.runtime_badge.clone(), badge_tone),
        separator.clone(),
        (
            dock.primary_summary.clone(),
            ComposerMetadataTone::Secondary,
        ),
    ];
    if let Some(text) = summary_text.as_ref() {
        without_context.push(separator.clone());
        without_context.push((
            text.clone(),
            summary_tone.unwrap_or(ComposerMetadataTone::Secondary),
        ));
    }

    let mut summary_only = vec![(
        dock.primary_summary.clone(),
        ComposerMetadataTone::Secondary,
    )];
    if let Some(text) = summary_text.as_ref() {
        summary_only.push(separator);
        summary_only.push((
            text.clone(),
            summary_tone.unwrap_or(ComposerMetadataTone::Secondary),
        ));
    }

    vec![
        full,
        without_context,
        vec![
            (dock.runtime_badge.clone(), badge_tone),
            (" ".to_string(), ComposerMetadataTone::Tertiary),
            (
                dock.primary_summary.clone(),
                ComposerMetadataTone::Secondary,
            ),
        ],
        summary_only,
        vec![(dock.runtime_badge.clone(), badge_tone)],
        vec![(
            dock.primary_summary.clone(),
            ComposerMetadataTone::Secondary,
        )],
    ]
}

fn live_composer_status_text(
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
    theme: &Theme,
) -> String {
    let summary_text = dock.summary_segment.as_ref().map(|segment| {
        control_dock_summary_segment_text_for_width(
            segment,
            u16::try_from(max_width).unwrap_or(u16::MAX),
            theme,
        )
    });
    let with_badge = format!("{}  ·  {}", dock.runtime_badge, dock.primary_summary);
    let with_context = dock
        .runtime_context
        .as_ref()
        .filter(|text| !text.trim().is_empty())
        .map(|context| format!("{context}  ·  {with_badge}"));
    let with_summary = summary_text
        .as_ref()
        .map(|text| format!("{with_badge}  ·  {text}"));
    let full = with_context.as_ref().and_then(|context_text| {
        summary_text
            .as_ref()
            .map(|summary| format!("{context_text}  ·  {summary}"))
    });

    best_fit_text(
        &[
            full,
            with_summary,
            with_context,
            Some(with_badge),
            Some(dock.primary_summary.clone()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
        max_width,
    )
}

fn runtime_kind_metadata_tone(kind: RuntimeStateKind) -> ComposerMetadataTone {
    match kind {
        RuntimeStateKind::Success => ComposerMetadataTone::Accent,
        RuntimeStateKind::Streaming | RuntimeStateKind::Sending | RuntimeStateKind::Ready => {
            ComposerMetadataTone::Primary
        }
        RuntimeStateKind::Failure
        | RuntimeStateKind::Cancelled
        | RuntimeStateKind::Disconnected
        | RuntimeStateKind::PermissionBlocked => ComposerMetadataTone::Accent,
        RuntimeStateKind::Degraded | RuntimeStateKind::PermissionPending => {
            ComposerMetadataTone::Primary
        }
    }
}

fn control_dock_summary_tone_to_metadata_tone(
    tone: crate::view_model::ControlDockSummaryTone,
) -> ComposerMetadataTone {
    match tone {
        crate::view_model::ControlDockSummaryTone::Secondary => ComposerMetadataTone::Secondary,
        crate::view_model::ControlDockSummaryTone::Accent => ComposerMetadataTone::Accent,
        crate::view_model::ControlDockSummaryTone::Success => ComposerMetadataTone::Accent,
        crate::view_model::ControlDockSummaryTone::Warning => ComposerMetadataTone::Primary,
        crate::view_model::ControlDockSummaryTone::Error => ComposerMetadataTone::Accent,
    }
}

fn composer_metadata_color(tone: ComposerMetadataTone, theme: &Theme) -> Color {
    match tone {
        ComposerMetadataTone::Accent => theme.text.accent,
        ComposerMetadataTone::Primary => theme.text.primary,
        ComposerMetadataTone::Secondary => theme.text.secondary,
        ComposerMetadataTone::Tertiary => theme.text.tertiary,
    }
}

fn composer_metadata_segments_width(segments: &[(String, ComposerMetadataTone)]) -> usize {
    segments
        .iter()
        .map(|(text, _)| text.chars().count())
        .sum::<usize>()
}

fn composer_metadata_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
) -> Vec<Vec<(String, ComposerMetadataTone)>> {
    let theme_segment = (
        format!("theme {}", app.theme_label()),
        ComposerMetadataTone::Secondary,
    );

    if app.startup_shell_visible() {
        let (launch_label, launch_identity) = dock
            .primary_summary
            .split_once(": ")
            .map_or((dock.primary_summary.as_str(), ""), |(label, identity)| {
                (label, identity)
            });
        let mut full = vec![(
            if launch_identity.is_empty() {
                launch_label.to_string()
            } else {
                format!("{launch_label}: ")
            },
            ComposerMetadataTone::Accent,
        )];
        if !launch_identity.is_empty() {
            full.push((launch_identity.to_string(), ComposerMetadataTone::Primary));
        }
        if let Some(provider) = dock
            .runtime_context
            .as_deref()
            .map(str::trim)
            .filter(|provider| !provider.is_empty())
        {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
            full.push((
                format!("provider {provider}"),
                ComposerMetadataTone::Secondary,
            ));
        }
        if let Some(mode) = app
            .launch_mode_label()
            .filter(|mode| !mode.trim().is_empty())
        {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
            full.push((mode.to_string(), ComposerMetadataTone::Accent));
        }
        full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        full.push(theme_segment.clone());

        return vec![
            full,
            vec![
                (dock.primary_summary.clone(), ComposerMetadataTone::Primary),
                (" · ".to_string(), ComposerMetadataTone::Secondary),
                (
                    format!("provider {}", app.active_provider()),
                    ComposerMetadataTone::Secondary,
                ),
                (" · ".to_string(), ComposerMetadataTone::Secondary),
                theme_segment.clone(),
            ],
            vec![
                (dock.primary_summary.clone(), ComposerMetadataTone::Primary),
                (" · ".to_string(), ComposerMetadataTone::Secondary),
                theme_segment.clone(),
            ],
            vec![(dock.primary_summary.clone(), ComposerMetadataTone::Primary)],
        ];
    }

    if app.completed_session_shell_active() {
        let identity = vec![(
            app.runtime_context_identity_line(),
            ComposerMetadataTone::Primary,
        )];
        let identity_with_theme = vec![
            (
                app.runtime_context_identity_line(),
                ComposerMetadataTone::Primary,
            ),
            (" · ".to_string(), ComposerMetadataTone::Secondary),
            theme_segment.clone(),
        ];
        let mut with_disclosure = identity.clone();
        with_disclosure.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        with_disclosure.push(theme_segment.clone());
        if !dock.composer_disclosure.trim().is_empty() {
            with_disclosure.push((" · ".to_string(), ComposerMetadataTone::Secondary));
            with_disclosure.push((
                dock.composer_disclosure.clone(),
                ComposerMetadataTone::Tertiary,
            ));
        }

        return vec![
            with_disclosure,
            identity_with_theme,
            identity,
            vec![(
                app.runtime_context_identity_line(),
                ComposerMetadataTone::Primary,
            )],
        ];
    }

    let mut full = vec![(dock.primary_summary.clone(), ComposerMetadataTone::Primary)];
    if let Some(segment) = dock.summary_segment.as_ref() {
        full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        full.push((segment.text.clone(), ComposerMetadataTone::Secondary));
    }
    if let Some(provider) = dock
        .runtime_context
        .as_deref()
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
    {
        full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        full.push((
            format!("provider {provider}"),
            ComposerMetadataTone::Secondary,
        ));
    }
    full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
    full.push(theme_segment.clone());

    let mut summary_and_provider =
        vec![(dock.primary_summary.clone(), ComposerMetadataTone::Primary)];
    if let Some(provider) = dock
        .runtime_context
        .as_deref()
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
    {
        summary_and_provider.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        summary_and_provider.push((
            format!("provider {provider}"),
            ComposerMetadataTone::Secondary,
        ));
    }
    summary_and_provider.push((" · ".to_string(), ComposerMetadataTone::Secondary));
    summary_and_provider.push(theme_segment.clone());

    vec![
        full,
        summary_and_provider,
        vec![
            (dock.primary_summary.clone(), ComposerMetadataTone::Primary),
            (" · ".to_string(), ComposerMetadataTone::Secondary),
            theme_segment,
        ],
        vec![(dock.primary_summary.clone(), ComposerMetadataTone::Primary)],
        vec![(
            app.runtime_context_identity_line(),
            ComposerMetadataTone::Secondary,
        )],
    ]
}

fn composer_metadata_text(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
) -> String {
    if max_width == 0 {
        return String::new();
    }

    if app.startup_shell_visible() {
        let metadata = app.startup_card_view_model().metadata;
        return best_fit_text(
            &[
                dock.primary_summary.clone(),
                format!("{}  ·  {}", dock.primary_summary, metadata),
                metadata,
            ],
            max_width,
        );
    }

    if app.completed_session_shell_active() {
        let identity = app.runtime_context_identity_line();
        let with_theme = format!("{}  ·  theme {}", identity, app.theme_label());
        let with_disclosure = (!dock.composer_disclosure.trim().is_empty())
            .then(|| format!("{with_theme}  ·  {}", dock.composer_disclosure));

        return best_fit_text(
            &[
                with_disclosure,
                Some(with_theme),
                Some(identity),
                Some(dock.composer_disclosure.clone()),
                Some(app.current_model_label().to_string()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
            max_width,
        );
    }

    let with_next_turns = dock
        .summary_segment
        .as_ref()
        .map(|segment| format!("{}  ·  {}", dock.primary_summary, segment.text));
    let with_provider = dock
        .runtime_context
        .as_deref()
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(|provider| format!("{}  ·  provider {provider}", dock.primary_summary));
    let with_theme = Some(format!(
        "{}  ·  theme {}",
        dock.primary_summary,
        app.theme_label()
    ));

    best_fit_text(
        &[
            with_next_turns,
            with_provider,
            with_theme,
            Some(dock.primary_summary.clone()),
            Some(app.runtime_context_identity_line()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
        max_width,
    )
}

fn live_composer_status_view_model(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
) -> Option<crate::view_model::ControlDockViewModel> {
    if app.startup_shell_visible() || app.replay_mode {
        return None;
    }

    let runtime_state = app.runtime_state();
    Some(crate::view_model::ControlDockViewModel {
        variant: dock.variant,
        runtime_context: Some(app.runtime_context_identity_line()),
        runtime_badge: runtime_state.kind.label().to_string(),
        runtime_kind: runtime_state.kind,
        primary_summary: completed_session_status_summary(app, &runtime_state)
            .unwrap_or_else(|| runtime_state.summary.clone()),
        summary_segment: control_dock_summary_segment(app),
        composer_body: String::new(),
        composer_disclosure: String::new(),
        composer_focused: dock.composer_focused,
        composer_disabled: dock.composer_disabled,
    })
}

fn best_fit_text(options: &[String], max_width: usize) -> String {
    options
        .iter()
        .find(|option| option.chars().count() <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            truncate_plain_text(options.first().map(String::as_str).unwrap_or(""), max_width)
        })
}

fn wrap_composer_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let logical_lines = text
        .split('\n')
        .map(|line| line.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut visual_lines = Vec::new();

    for chars in &logical_lines {
        if chars.is_empty() {
            visual_lines.push(String::new());
            continue;
        }

        let mut start = 0usize;
        while start < chars.len() {
            let end = (start + width).min(chars.len());
            visual_lines.push(chars[start..end].iter().collect::<String>());
            start = end;
        }
    }

    if visual_lines.is_empty() {
        visual_lines.push(String::new());
    }

    visual_lines
}

fn composer_viewport(
    text: &str,
    width: usize,
    max_lines: usize,
    cursor_char_index: Option<usize>,
) -> ComposerViewport {
    if max_lines == 0 {
        return ComposerViewport {
            lines: Vec::new(),
            cursor: None,
        };
    }

    const CURSOR_MARKER: char = '\0';

    let mut raw = text.to_string();
    if let Some(cursor_char_index) = cursor_char_index {
        let cursor_byte_index = text
            .char_indices()
            .nth(cursor_char_index)
            .map(|(index, _)| index)
            .unwrap_or(text.len());
        raw.insert(cursor_byte_index, CURSOR_MARKER);
    }

    let mut wrapped = wrap_composer_lines(&raw, width);
    let cursor = wrapped.iter_mut().enumerate().find_map(|(row, line)| {
        line.find(CURSOR_MARKER).map(|column| {
            line.remove(column);
            (row, column)
        })
    });

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    let total_lines = wrapped.len();
    let visible_count = max_lines.min(total_lines).max(1);
    let anchor_row = cursor
        .map(|(row, _)| row)
        .unwrap_or(total_lines.saturating_sub(1));
    let start_row = anchor_row
        .saturating_add(1)
        .saturating_sub(visible_count)
        .min(total_lines.saturating_sub(visible_count));
    let end_row = start_row.saturating_add(visible_count).min(total_lines);

    ComposerViewport {
        lines: wrapped[start_row..end_row].to_vec(),
        cursor: cursor.and_then(|(row, column)| {
            (row >= start_row && row < end_row).then_some((row - start_row, column))
        }),
    }
}

fn composer_shortcut_hints(app: &AppState, composer_disabled: bool) -> String {
    let newline = composer_newline_binding_hint(app);
    if app.startup_shell_visible() {
        let palette = app.keymap.get_binding_str(Action::Palette);
        return format!("{palette} opens saved sessions · {newline} adds a newline");
    }

    if composer_disabled || app.completed_session_shell_active() {
        return app.keymap.get_binding_label(Action::Palette, "commands");
    }

    let send = app.keymap.get_binding_label(Action::SubmitPrompt, "send");
    let history = composer_history_binding_hint(app);
    if history == "-" {
        format!("{send} · {newline} newline")
    } else {
        format!("{send} · {newline} newline · {history} history")
    }
}

fn composer_newline_binding_hint(app: &AppState) -> String {
    let bindings = app.keymap.get_binding_strs(Action::InsertNewline);
    match bindings.as_slice() {
        [] => "-".to_string(),
        [binding] => binding.clone(),
        [first, second, ..] => format!("{first}/{second}"),
    }
}

fn composer_history_binding_hint(app: &AppState) -> String {
    let up = app
        .keymap
        .get_binding_strs(Action::HistoryUp)
        .into_iter()
        .next();
    let down = app
        .keymap
        .get_binding_strs(Action::HistoryDown)
        .into_iter()
        .next();
    match (up, down) {
        (Some(up), Some(down)) => format!("{up}/{down}"),
        (Some(up), None) => up,
        (None, Some(down)) => down,
        (None, None) => "-".to_string(),
    }
}

fn control_dock_row_content(
    app: &AppState,
    max_width: usize,
    theme: &Theme,
    status_candidates: Vec<String>,
    disclosure_visible: bool,
) -> (String, String) {
    if max_width == 0 {
        return (String::new(), String::new());
    }

    let mut hints = app.footer_hints_view_model().hints;
    let under_input_shortcuts_visible =
        disclosure_visible || (!app.startup_shell_visible() && app.events.is_empty());
    if under_input_shortcuts_visible {
        hints.retain(|hint| {
            if app.completed_session_shell_active() || app.runtime_state().composer_disabled {
                hint.action != Action::Palette
            } else {
                hint.action != Action::SubmitPrompt
            }
        });
    }
    let palette_only = hints
        .iter()
        .find(|hint| hint.action == Action::Palette)
        .copied()
        .map(|hint| vec![hint])
        .unwrap_or_default();
    let last_only = hints
        .last()
        .copied()
        .map(|hint| vec![hint])
        .unwrap_or_default();
    let variants = [
        hints.clone(),
        compact_footer_hints(&hints, 4),
        compact_footer_hints(&hints, 2),
        palette_only,
        last_only,
    ];

    let separator = " ".repeat(theme.live_shell.rhythm.status_separator as usize);
    let candidates = variants
        .into_iter()
        .filter(|variant_hints| !variant_hints.is_empty())
        .map(|variant_hints| {
            variant_hints
                .iter()
                .map(|hint| app.keymap.get_binding_label(hint.action, hint.label))
                .collect::<Vec<_>>()
                .join(&separator)
        })
        .collect::<Vec<_>>();

    if status_candidates.is_empty() {
        let hint = candidates
            .iter()
            .find(|candidate| candidate.chars().count() <= max_width)
            .cloned()
            .unwrap_or_else(|| {
                truncate_plain_text(
                    candidates.first().map(String::as_str).unwrap_or(""),
                    max_width,
                )
            });
        return (String::new(), hint);
    }

    for status in &status_candidates {
        let status_width = status.chars().count();
        for hint in &candidates {
            let hint_width = hint.chars().count();
            let gap = usize::from(status_width > 0 && hint_width > 0) * 2;
            if status_width.saturating_add(hint_width).saturating_add(gap) <= max_width {
                return (status.clone(), hint.clone());
            }
        }
    }

    for status in &status_candidates {
        if status.chars().count() <= max_width {
            return (status.clone(), String::new());
        }
    }

    let hint = candidates
        .iter()
        .find(|candidate| candidate.chars().count() <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            truncate_plain_text(
                candidates.first().map(String::as_str).unwrap_or(""),
                max_width,
            )
        });
    (String::new(), hint)
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
    app: &AppState,
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

    let metadata_visible = content_area.height > 1;
    let hint_visible = content_area.height > 2 && !dock.composer_disclosure.trim().is_empty();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(u16::from(metadata_visible)),
            Constraint::Min(0),
        ])
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

    if metadata_visible && rows[1].height > 0 {
        frame.render_widget(
            Paragraph::new(composer_metadata_line(
                app,
                dock,
                usize::from(rows[1].width),
                theme,
                surface,
            ))
            .style(Style::default().bg(surface)),
            rows[1],
        );
    }

    if hint_visible && rows[2].height > 0 {
        let hint_prefix = "  ";
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(hint_prefix, Style::default().bg(surface)),
                Span::styled(
                    truncate_plain_text(
                        &dock.composer_disclosure,
                        usize::from(rows[2].width).saturating_sub(hint_prefix.chars().count()),
                    ),
                    Style::default().fg(theme.text.secondary).bg(surface),
                ),
            ])),
            rows[2],
        );
    }
}

fn render_control_dock_disclosure(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
) {
    if area.width == 0 || area.height == 0 || dock.composer_disclosure.trim().is_empty() {
        return;
    }

    let surface = control_dock_surface(theme, dock.variant);
    let base = Style::default().bg(surface);
    let text = truncate_plain_text(
        &dock.composer_disclosure,
        usize::from(area.width).saturating_sub(2),
    );

    frame.render_widget(Block::default().style(base), area);
    if dock.variant == crate::view_model::ControlDockVariant::Startup {
        let palette = app.keymap.get_binding_str(Action::Palette);
        let newline = composer_newline_binding_hint(app);
        let candidates = startup_disclosure_candidates(theme, surface, &palette, &newline);
        let spans = candidates
            .into_iter()
            .find(|candidate| startup_disclosure_width(candidate) <= usize::from(area.width))
            .unwrap_or_else(|| {
                vec![
                    Span::styled("  ", base),
                    Span::styled(
                        "● Tip",
                        Style::default()
                            .fg(theme.text.accent)
                            .bg(surface)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  ", base),
                    Span::styled(
                        truncate_plain_text(&palette, usize::from(area.width).saturating_sub(8)),
                        Style::default()
                            .fg(theme.text.primary)
                            .bg(surface)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]
            });
        frame.render_widget(Paragraph::new(Line::from(spans)).style(base), area);
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ", base),
            Span::styled(text, Style::default().fg(theme.text.secondary).bg(surface)),
        ]))
        .style(base),
        area,
    );
}

fn startup_disclosure_candidates(
    theme: &Theme,
    surface: Color,
    palette: &str,
    newline: &str,
) -> Vec<Vec<Span<'static>>> {
    let base = Style::default().bg(surface);
    let tip = Style::default()
        .fg(theme.text.accent)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let key = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let text = Style::default().fg(theme.text.secondary).bg(surface);

    vec![
        vec![
            Span::styled("  ", base),
            Span::styled("● Tip", tip),
            Span::styled("  ", base),
            Span::styled(palette.to_string(), key),
            Span::styled(" opens saved sessions · ", text),
            Span::styled(newline.to_string(), key),
            Span::styled(" adds a newline", text),
        ],
        vec![
            Span::styled("  ", base),
            Span::styled("● Tip", tip),
            Span::styled("  ", base),
            Span::styled(palette.to_string(), key),
            Span::styled(" opens saved sessions", text),
        ],
        vec![
            Span::styled("  ", base),
            Span::styled("● Tip", tip),
            Span::styled("  ", base),
            Span::styled(palette.to_string(), key),
        ],
    ]
}

fn startup_disclosure_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn replay_read_only_shortcut_hints(app: &AppState) -> String {
    app.replay_recovery_shortcut_hint()
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

    let mut app = AppState::new_live(None, false, None);
    let mut events = crate::lib_tests::session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }
    let theme = Theme::default();
    let width = 100;
    let height = 30;
    let area = Rect::new(0, 0, width, height);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let dock = plan.dock.expect("live dock layout");
    let composer = dock.composer;

    assert_eq!(dock.status, Some(Rect::new(0, 29, 100, 1)));
    assert_eq!(dock.shell.height, composer.height.saturating_add(2));
    assert_eq!(dock.shell.y, composer.y);
    assert_eq!(dock.disclosure, Some(Rect::new(0, 28, 100, 1)));

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

    assert_eq!(
        buffer[(right_edge, composer.y)].bg,
        theme.surface.panel_elevated
    );
    assert_eq!(
        buffer[(
            right_edge,
            composer.y.saturating_add(composer.height.saturating_sub(1))
        )]
            .bg,
        control_dock_surface(&theme, crate::view_model::ControlDockVariant::Live)
    );
    assert_eq!(
        buffer[(right_edge, dock.shell.y)].bg,
        theme.surface.panel_elevated
    );
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
        control_dock_surface(&theme, crate::view_model::ControlDockVariant::Live)
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_control_dock_collapses_disclosure_before_status() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = AppState::new_live(None, false, None);
    let mut events = crate::lib_tests::session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }
    let width = 60;
    let height = 18;

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
    assert!(!rendered.contains("↑/↓ history"));
    assert!(rendered.contains("q quit"));
}
