use super::*;

use ratatui::widgets::Padding;

use crate::app::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
use crate::layout::{
    pad_rect, permission_dock_layout, ControlDockLayout, FrameLayoutPlan, SessionFooterMode,
    SessionHeaderMode,
};
use crate::theme::{ChromeMode, DividerIntensity};

use super::ui_overlays::{
    permission_modal_actions_text, permission_modal_draft_line, permission_modal_guidance,
    permission_modal_icon, permission_modal_metadata_line, permission_modal_subject_line,
    permission_modal_summary_line, permission_modal_title, question_permission_actions_text,
    question_permission_body_text,
};

struct DocumentComposerRenderContext<'a> {
    dock: &'a crate::view_model::ControlDockViewModel,
    composer_lines: u16,
    disclosure_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerViewport {
    lines: Vec<String>,
    cursor: Option<(usize, usize)>,
}

const QUIET_SURFACE_PADDING_X: u16 = 1;
const QUIET_SURFACE_PADDING_TOP: u16 = 1;
pub(super) const COMPOSER_RAIL_GLYPH: &str = "┃";
pub(super) const COMPOSER_RAIL_CAP_GLYPH: &str = "╹";
pub(super) const COMPOSER_SEPARATOR_GLYPH: &str = "▀";

const fn command_palette_accent() -> Color {
    Color::Rgb(0x9D, 0x7C, 0xD8)
}

pub(super) const fn composer_input_surface(theme: &Theme) -> Color {
    theme.surface.panel_elevated
}

pub(super) const fn composer_input_text(theme: &Theme) -> Color {
    theme.text.primary
}

pub(super) const fn composer_input_muted(theme: &Theme) -> Color {
    theme.text.secondary
}

pub(super) const fn composer_input_accent(theme: &Theme) -> Color {
    theme.text.accent
}

pub(super) const fn command_palette_surface(theme: &Theme) -> Color {
    theme.surface.panel
}

pub(super) const fn slash_command_surface(theme: &Theme) -> Color {
    theme.surface.panel
}

pub(super) const fn slash_command_selection_bg(theme: &Theme) -> Color {
    theme.surface.panel_elevated
}

pub(super) const fn command_palette_title(theme: &Theme) -> Color {
    composer_input_text(theme)
}

pub(super) const fn command_palette_muted(theme: &Theme) -> Color {
    composer_input_muted(theme)
}

pub(super) const fn command_palette_section() -> Color {
    command_palette_accent()
}

pub(super) const fn command_palette_selection_bg(theme: &Theme) -> Color {
    theme.text.accent
}

pub(super) const fn command_palette_selection_fg(theme: &Theme) -> Color {
    theme.text.inverse
}

pub(super) const fn command_palette_cursor(theme: &Theme) -> Color {
    theme.text.accent
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

    if dock.variant == crate::view_model::ControlDockVariant::ReplayReadOnly {
        render_replay_read_only_composer_content(frame, dock_layout.composer, theme, &dock);
        return;
    }

    if let Some(permission) = app.active_permission_view() {
        render_inline_permission_dock(frame, app, dock_layout.composer, theme, &permission);
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
            disclosure_visible: dock_layout.disclosure.is_some(),
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

pub(super) fn overlay_focus_row_style(_theme: &Theme) -> Style {
    Style::default()
        .fg(command_palette_selection_fg(_theme))
        .bg(command_palette_selection_bg(_theme))
}

pub(super) fn slash_command_row_style(theme: &Theme, is_selected: bool) -> Style {
    let surface = slash_command_surface(theme);
    if is_selected {
        Style::default().bg(slash_command_selection_bg(theme))
    } else {
        Style::default().bg(surface)
    }
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

fn render_inline_permission_dock(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    permission: &crate::app::ActivePermissionView,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let submission_pending = app.permission_submission_pending(&permission.permission_id);
    let is_question = permission.question_prompts.is_some();
    if is_question {
        render_question_permission_dock(frame, app, area, theme, permission, submission_pending);
        return;
    }

    let dock_layout = permission_dock_layout(area, is_question);
    let always_confirm = !is_question
        && app.permission_modal_stage(&permission.permission_id)
            == PermissionModalStage::AlwaysConfirm;
    let shell_surface = theme.surface.panel;
    let tray_surface = theme.surface.panel_elevated;
    let tray_height = dock_layout.tray_height;
    let sections = if area.height > tray_height {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(tray_height)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(area)
    };
    let shell_area = sections[0];
    let tray_area = sections[1];

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(dock_layout.rail_width),
            Constraint::Min(0),
        ])
        .split(area);
    let rail_area = columns[0];
    let body_area = columns[1];
    let body_sections = if body_area.height > tray_height {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(tray_height)])
            .split(body_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(body_area)
    };
    let shell_body_area = body_sections[0];
    let tray_body_area = body_sections[1];

    frame.render_widget(
        Block::default().style(Style::default().bg(shell_surface)),
        shell_body_area,
    );
    if tray_body_area.height > 0 {
        frame.render_widget(
            Block::default().style(Style::default().bg(tray_surface)),
            tray_body_area,
        );
    }

    if rail_area.width > 0 && rail_area.height > 0 {
        let rail_color = theme.status.warning;
        let shell_rows = usize::from(shell_area.height);
        let tray_rows = usize::from(tray_area.height);
        let mut lines = Vec::with_capacity(shell_rows.saturating_add(tray_rows).max(1));
        lines.extend(
            std::iter::repeat_with(|| {
                Line::from(Span::styled(
                    COMPOSER_RAIL_GLYPH,
                    Style::default().fg(rail_color).bg(shell_surface),
                ))
            })
            .take(shell_rows),
        );
        lines.extend(
            std::iter::repeat_with(|| {
                Line::from(Span::styled(
                    COMPOSER_RAIL_GLYPH,
                    Style::default().fg(rail_color).bg(tray_surface),
                ))
            })
            .take(tray_rows),
        );
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                COMPOSER_RAIL_GLYPH,
                Style::default().fg(rail_color).bg(shell_surface),
            )));
        }
        frame.render_widget(Paragraph::new(lines), rail_area);
    }

    let shell_inner = pad_rect(shell_body_area, dock_layout.shell_padding);
    if shell_inner.width == 0 || shell_inner.height == 0 {
        return;
    }

    let header = if always_confirm {
        Text::from(vec![Line::from(vec![
            Span::styled(
                "△",
                Style::default().fg(theme.status.warning).bg(shell_surface),
            ),
            Span::styled(" ", Style::default().bg(shell_surface)),
            Span::styled(
                "Always allow",
                Style::default().fg(theme.text.primary).bg(shell_surface),
            ),
        ])])
    } else {
        let icon = permission_modal_icon(permission);
        let subject = permission_modal_subject_line(permission);
        Text::from(vec![
            Line::from(vec![
                Span::styled(
                    "△",
                    Style::default().fg(theme.status.warning).bg(shell_surface),
                ),
                Span::styled(" ", Style::default().bg(shell_surface)),
                Span::styled(
                    permission_modal_title(permission),
                    Style::default().fg(theme.text.primary).bg(shell_surface),
                ),
            ]),
            Line::from(vec![
                Span::styled("  ", Style::default().bg(shell_surface)),
                Span::styled(
                    icon,
                    Style::default().fg(theme.text.secondary).bg(shell_surface),
                ),
                Span::styled(" ", Style::default().bg(shell_surface)),
                Span::styled(
                    subject,
                    Style::default().fg(theme.text.primary).bg(shell_surface),
                ),
            ]),
        ])
    };
    let header_height = u16::try_from(header.lines.len())
        .unwrap_or(u16::MAX)
        .min(shell_inner.height);
    let body_gap = dock_layout
        .header_gap
        .min(shell_inner.height.saturating_sub(header_height));
    let shell_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(body_gap),
            Constraint::Min(0),
        ])
        .split(shell_inner);
    frame.render_widget(Paragraph::new(header), shell_rows[0]);

    if shell_rows[2].width > 0 && shell_rows[2].height > 0 {
        let metadata_style = Style::default().fg(theme.text.secondary).bg(shell_surface);
        let summary_style = Style::default().fg(theme.text.primary).bg(shell_surface);
        let guidance_style = Style::default().fg(theme.text.secondary).bg(shell_surface);
        let body = if submission_pending {
            Text::from(vec![
                Line::from(vec![Span::styled(
                    permission_modal_summary_line(permission, submission_pending),
                    summary_style,
                )]),
                Line::from(vec![Span::styled(
                    permission_modal_guidance(permission, submission_pending),
                    metadata_style,
                )]),
            ])
        } else if always_confirm {
            Text::from(vec![
                Line::from(vec![Span::styled(
                    "This will allow this exact request until the harness is restarted.",
                    metadata_style,
                )]),
                Line::from(vec![Span::styled(
                    permission_modal_metadata_line(permission),
                    summary_style,
                )]),
            ])
        } else {
            let mut lines = vec![Line::from(vec![Span::styled(
                permission_modal_summary_line(permission, false),
                summary_style,
            )])];
            lines.push(Line::from(vec![Span::styled(
                permission_modal_metadata_line(permission),
                metadata_style,
            )]));
            let draft = permission_modal_draft_line(app.prompt_buffer.as_str());
            if !draft.is_empty() {
                lines.push(Line::from(vec![Span::styled(draft, guidance_style)]));
            }
            Text::from(lines)
        };

        frame.render_widget(
            Paragraph::new(body)
                .style(Style::default().bg(shell_surface))
                .wrap(Wrap { trim: true }),
            pad_rect(shell_rows[2], dock_layout.body_padding),
        );
    }

    if tray_body_area.width == 0 || tray_body_area.height == 0 {
        return;
    }

    let tray_inner = pad_rect(tray_body_area, dock_layout.tray_padding);
    if tray_inner.width == 0 || tray_inner.height == 0 {
        return;
    }

    if submission_pending {
        frame.render_widget(
            Paragraph::new(permission_modal_actions_text(
                app,
                theme,
                tray_surface,
                submission_pending,
                permission.question_prompts.is_some(),
            ))
            .style(Style::default().bg(tray_surface))
            .wrap(Wrap { trim: true }),
            tray_inner,
        );
        return;
    }

    if is_question {
        frame.render_widget(
            Paragraph::new(question_permission_actions_text(
                app,
                permission,
                permission.question_prompts.as_deref().unwrap_or(&[]),
                theme,
                tray_surface,
            ))
            .style(Style::default().bg(tray_surface))
            .wrap(Wrap { trim: true }),
            tray_inner,
        );
        return;
    }

    let action_line = if always_confirm {
        permission_prompt_action_line(
            theme,
            tray_surface,
            &[
                (
                    "Confirm",
                    app.permission_modal_confirm_selection(&permission.permission_id)
                        == PermissionConfirmSelection::Confirm,
                ),
                (
                    "Cancel",
                    app.permission_modal_confirm_selection(&permission.permission_id)
                        == PermissionConfirmSelection::Cancel,
                ),
            ],
        )
    } else {
        let selection = app.permission_modal_selection(&permission.permission_id);
        permission_prompt_action_line(
            theme,
            tray_surface,
            &[
                (
                    "Allow once",
                    selection == PermissionModalSelection::AllowOnce,
                ),
                (
                    "Allow always",
                    selection == PermissionModalSelection::AllowAlways,
                ),
                ("Reject", selection == PermissionModalSelection::Reject),
            ],
        )
    };
    let hint_line = permission_prompt_hint_line(theme, tray_surface);
    let hint_width = u16::try_from(display_width("⇆ select  enter confirm")).unwrap_or(u16::MAX);
    let narrow = area.width < dock_layout.stacked_hint_min_width
        || tray_inner.width <= hint_width.saturating_add(dock_layout.stacked_hint_min_action_width);

    if narrow && tray_inner.height > 1 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(tray_inner);
        frame.render_widget(
            Paragraph::new(action_line).style(Style::default().bg(tray_surface)),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new(hint_line).style(Style::default().bg(tray_surface)),
            rows[1],
        );
        return;
    }

    let footer_columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(hint_width.min(tray_inner.width)),
        ])
        .split(tray_inner);
    frame.render_widget(
        Paragraph::new(action_line).style(Style::default().bg(tray_surface)),
        footer_columns[0],
    );
    if footer_columns[1].width > 0 {
        frame.render_widget(
            Paragraph::new(hint_line)
                .style(Style::default().bg(tray_surface))
                .alignment(Alignment::Right),
            footer_columns[1],
        );
    }
}

fn render_question_permission_dock(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) {
    let surface = theme.surface.panel;
    let rail_color = question_prompt_accent(theme);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    let rail_area = columns[0];
    let body_area = columns[1];

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        body_area,
    );

    if rail_area.width > 0 && rail_area.height > 0 {
        let lines = std::iter::repeat_with(|| {
            Line::from(Span::styled(
                COMPOSER_RAIL_GLYPH,
                Style::default().fg(rail_color).bg(surface),
            ))
        })
        .take(usize::from(rail_area.height))
        .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), rail_area);
    }

    let inner = pad_rect(body_area, crate::layout::EdgeInsets::new(1, 3, 1, 1));
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content_height = inner.height.saturating_sub(2);
    let content_width = inner.width.saturating_sub(1);
    if content_height > 0 && content_width > 0 {
        let content_area = Rect::new(
            inner.x.saturating_add(1),
            inner.y,
            content_width,
            content_height,
        );
        let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
        let body = question_permission_body_text(app, permission, prompts, theme, surface);
        frame.render_widget(
            Paragraph::new(body)
                .style(Style::default().bg(surface))
                .wrap(Wrap { trim: false }),
            content_area,
        );
    }

    let footer_width = inner.width.saturating_sub(1);
    if footer_width == 0 || inner.height == 0 {
        return;
    }

    let footer_area = Rect::new(
        inner.x.saturating_add(1),
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        footer_width,
        1,
    );
    let footer = if submission_pending {
        permission_modal_actions_text(app, theme, surface, true, true)
    } else {
        question_permission_actions_text(
            app,
            permission,
            permission.question_prompts.as_deref().unwrap_or(&[]),
            theme,
            surface,
        )
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().bg(surface)),
        footer_area,
    );
}

pub(super) const fn question_prompt_accent(theme: &Theme) -> Color {
    theme.question_prompt.accent
}

pub(super) const fn question_prompt_secondary(theme: &Theme) -> Color {
    theme.question_prompt.secondary
}

fn permission_prompt_action_line(
    theme: &Theme,
    surface: Color,
    options: &[(&str, bool)],
) -> Line<'static> {
    let selected_style = Style::default()
        .fg(theme.text.inverse)
        .bg(theme.status.warning);
    let unselected_style = Style::default().fg(theme.text.secondary).bg(surface);
    let mut spans = Vec::new();
    for (index, (label, selected)) in options.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" ", Style::default().bg(surface)));
        }
        spans.push(Span::styled(
            format!(" {label} "),
            if *selected {
                selected_style
            } else {
                unselected_style
            },
        ));
    }
    Line::from(spans)
}

fn permission_prompt_hint_line(theme: &Theme, surface: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("⇆", Style::default().fg(theme.text.primary).bg(surface)),
        Span::styled(
            " select  ",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
        Span::styled("enter", Style::default().fg(theme.text.primary).bg(surface)),
        Span::styled(
            " confirm",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
    ])
}

fn render_document_composer_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    let _startup_composer = matches!(
        context.dock.variant,
        crate::view_model::ControlDockVariant::Startup
    );
    let surface = control_dock_surface(theme, context.dock.variant);
    let composer_surface = composer_input_surface(theme);
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
    let metadata_gap = u16::from(metadata_height > 0 && body_inner.height >= 4);
    let top_padding = u16::from(
        body_inner.height
            >= context
                .composer_lines
                .saturating_add(metadata_height)
                .saturating_add(metadata_gap)
                .saturating_add(1),
    );
    let available_input_height = body_inner
        .height
        .saturating_sub(top_padding)
        .saturating_sub(metadata_gap)
        .saturating_sub(metadata_height)
        .max(1);
    let input_height = context
        .composer_lines
        .clamp(1, available_input_height)
        .max(1);
    let trailing_fill = body_inner
        .height
        .saturating_sub(top_padding)
        .saturating_sub(input_height)
        .saturating_sub(metadata_gap)
        .saturating_sub(metadata_height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_padding),
            Constraint::Length(input_height),
            Constraint::Length(metadata_gap),
            Constraint::Length(metadata_height),
            Constraint::Length(trailing_fill),
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
        composer_input_muted(theme)
    } else {
        composer_input_text(theme)
    };
    let rail_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else {
        composer_input_accent(theme)
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
            Paragraph::new(COMPOSER_SEPARATOR_GLYPH.repeat(usize::from(composer_cap_area.width)))
                .style(Style::default().fg(composer_surface).bg(surface)),
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
                context.disclosure_visible,
                usize::from(rows[3].width),
                theme,
                composer_surface,
            ))
            .style(Style::default().bg(composer_surface)),
            rows[3],
        );
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
    _disclosure_visible: bool,
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
        ComposerMetadataTone::Accent => composer_input_accent(theme),
        ComposerMetadataTone::Primary => composer_input_text(theme),
        ComposerMetadataTone::Secondary => composer_input_muted(theme),
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
    let profile = app.current_agent_label();
    let model = app.current_model_base_label().to_string();
    let source = app.current_source_label();
    let tail = app
        .current_model_reasoning_label()
        .map(str::to_string)
        .or_else(|| {
            (!dock.runtime_badge.trim().is_empty() && dock.runtime_badge != "Ready")
                .then(|| dock.runtime_badge.to_ascii_lowercase())
        });

    let mut full = Vec::new();
    if let Some(profile) = profile.clone() {
        full.push((profile, ComposerMetadataTone::Accent));
    }
    if !model.is_empty() && model != "-" {
        if !full.is_empty() {
            full.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((model.clone(), ComposerMetadataTone::Primary));
    }
    if let Some(source) = source.clone() {
        if !full.is_empty() {
            full.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((source, ComposerMetadataTone::Secondary));
    }
    if let Some(tail) = tail.as_ref() {
        if !full.is_empty() {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((tail.clone(), ComposerMetadataTone::Accent));
    }

    let mut compact = Vec::new();
    if let Some(profile) = profile.as_ref() {
        compact.push((profile.clone(), ComposerMetadataTone::Accent));
    }
    if !model.is_empty() && model != "-" {
        if !compact.is_empty() {
            compact.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        compact.push((model, ComposerMetadataTone::Primary));
    }

    vec![
        full,
        compact,
        source
            .map(|source| vec![(source, ComposerMetadataTone::Secondary)])
            .or_else(|| {
                profile
                    .as_ref()
                    .map(|profile| vec![(profile.clone(), ComposerMetadataTone::Accent)])
            })
            .unwrap_or_default(),
        vec![(
            dock.primary_summary.clone(),
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

    best_fit_text(
        &[
            Some(format!(
                "{} {}",
                app.active_profile(),
                app.current_model_label()
            )),
            app.launch_mode_label().map(str::to_string),
            Some(dock.primary_summary.clone()),
            Some(app.current_model_label().to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
        max_width,
    )
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

fn compact_usage_count(value: u64) -> String {
    if value >= 1_000_000 {
        return format!("{:.1}M", value as f64 / 1_000_000.0);
    }
    if value >= 1_000 {
        return format!("{:.1}K", value as f64 / 1_000.0);
    }
    value.to_string()
}

fn composer_context_usage(app: &AppState) -> (String, Option<String>) {
    let total = app
        .activities
        .iter()
        .filter_map(|activity| activity.usage)
        .map(|usage| u64::from(usage.total_tokens))
        .sum::<u64>();
    let percent = app.current_context_window_tokens().and_then(|limit| {
        (limit > 0).then(|| {
            format!(
                "{}%",
                ((total as f64 / f64::from(limit)) * 100.0)
                    .round()
                    .clamp(0.0, 999.0) as u64
            )
        })
    });
    (compact_usage_count(total), percent)
}

fn composer_context_summary_candidates(
    app: &AppState,
    theme: &Theme,
    surface: Color,
) -> Vec<Vec<Span<'static>>> {
    let (tokens, percent) = composer_context_usage(app);
    let mut primary = vec![disclosure_segment(
        tokens.clone(),
        ComposerMetadataTone::Secondary,
        theme,
        surface,
    )];
    if let Some(percent) = percent.as_deref() {
        primary.push(disclosure_segment(
            format!(" ({percent})"),
            ComposerMetadataTone::Tertiary,
            theme,
            surface,
        ));
    }

    let mut candidates = vec![primary];
    if percent.is_some() {
        candidates.push(vec![disclosure_segment(
            tokens,
            ComposerMetadataTone::Secondary,
            theme,
            surface,
        )]);
    }
    candidates.push(Vec::new());
    candidates
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

fn render_control_dock_disclosure(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
    dock: &crate::view_model::ControlDockViewModel,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = control_dock_surface(theme, dock.variant);
    let base = Style::default().bg(surface);

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

    let active_live_composer = !app.startup_shell_visible()
        && !app.completed_session_shell_active()
        && !dock.composer_disabled;
    let mut hint_candidates = composer_disclosure_hint_candidates(app, dock, theme, surface);
    let summary_candidates = if active_live_composer {
        composer_context_summary_candidates(app, theme, surface)
    } else {
        composer_disclosure_summary_candidates(app, dock, theme, surface)
    };
    let max_width = usize::from(area.width);

    if active_live_composer && area.width < theme.live_shell.breakpoints.minimum.width {
        hint_candidates.retain(|candidate| {
            let text = candidate
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .to_ascii_lowercase();
            !text.contains("history") && !text.contains("quit")
        });
    }

    if active_live_composer {
        let best_fit = summary_candidates
            .iter()
            .enumerate()
            .flat_map(|(summary_idx, summary_spans)| {
                hint_candidates
                    .iter()
                    .enumerate()
                    .filter_map(move |(hint_idx, hint_spans)| {
                        let combined = inline_disclosure_cluster(
                            summary_spans.as_slice(),
                            hint_spans.as_slice(),
                            surface,
                        );
                        let width = spans_width(&combined);
                        (width <= max_width).then_some((summary_idx + (hint_idx * 2), combined))
                    })
            })
            .min_by_key(|(score, _)| *score)
            .map(|(_, spans)| spans);

        let render_spans = best_fit.or_else(|| {
            hint_candidates.iter().find_map(|hint_spans| {
                (spans_width(hint_spans) <= max_width).then(|| hint_spans.clone())
            })
        });

        if let Some(spans) = render_spans {
            frame.render_widget(
                Paragraph::new(Line::from(spans))
                    .style(base)
                    .alignment(Alignment::Right),
                area,
            );
            return;
        }
    }

    let best_fit = summary_candidates
        .iter()
        .enumerate()
        .filter_map(|(summary_idx, summary_spans)| {
            let summary_width = spans_width(summary_spans);
            (summary_width <= max_width).then_some((summary_idx, summary_spans, summary_width))
        })
        .flat_map(|(summary_idx, summary_spans, summary_width)| {
            hint_candidates
                .iter()
                .enumerate()
                .filter_map(move |(hint_idx, hint_spans)| {
                    let hint_width = spans_width(hint_spans);
                    if hint_width > max_width {
                        return None;
                    }

                    let summary_gap =
                        usize::from(!summary_spans.is_empty() && !hint_spans.is_empty()) * 2;
                    (summary_width
                        .saturating_add(hint_width)
                        .saturating_add(summary_gap)
                        <= max_width)
                        .then_some((
                            summary_idx + (hint_idx * 2),
                            summary_spans,
                            hint_spans,
                            hint_width,
                        ))
                })
        })
        .min_by_key(|(score, _, _, _)| *score);

    if let Some((_, summary_spans, hint_spans, hint_width)) = best_fit {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(u16::try_from(hint_width).unwrap_or(u16::MAX)),
            ])
            .split(area);
        if !summary_spans.is_empty() && columns[0].width > 0 {
            frame.render_widget(
                Paragraph::new(Line::from(summary_spans.to_vec())).style(base),
                columns[0],
            );
        }
        if !hint_spans.is_empty() && columns[1].width > 0 {
            frame.render_widget(
                Paragraph::new(Line::from(hint_spans.to_vec()))
                    .style(base)
                    .alignment(Alignment::Right),
                columns[1],
            );
        }
        return;
    }

    if let Some(summary_spans) = summary_candidates.first().cloned() {
        frame.render_widget(Paragraph::new(Line::from(summary_spans)).style(base), area);
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn inline_disclosure_cluster(
    summary_spans: &[Span<'static>],
    hint_spans: &[Span<'static>],
    surface: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(summary_spans.len() + hint_spans.len() + 1);
    spans.extend(summary_spans.iter().cloned());
    if !summary_spans.is_empty() && !hint_spans.is_empty() {
        spans.push(Span::styled("  ", Style::default().bg(surface)));
    }
    spans.extend(hint_spans.iter().cloned());
    spans
}

fn disclosure_segment(
    text: impl Into<String>,
    tone: ComposerMetadataTone,
    theme: &Theme,
    surface: Color,
) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default()
            .fg(composer_metadata_color(tone, theme))
            .bg(surface),
    )
}

fn disclosure_separator(theme: &Theme, surface: Color) -> Span<'static> {
    disclosure_segment("  ·  ", ComposerMetadataTone::Tertiary, theme, surface)
}

fn disclosure_keycap(binding: &str, theme: &Theme, surface: Color) -> Span<'static> {
    Span::styled(
        binding.to_string(),
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    )
}

fn disclosure_shortcut(
    binding: &str,
    label: &str,
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    vec![
        disclosure_keycap(binding, theme, surface),
        disclosure_segment(
            format!(" {label}"),
            ComposerMetadataTone::Secondary,
            theme,
            surface,
        ),
    ]
}

fn shortcut_binding(app: &AppState, action: Action) -> Option<String> {
    app.keymap.get_binding_strs(action).into_iter().next()
}

fn compose_shortcut_row(
    shortcuts: &[(&str, &str)],
    theme: &Theme,
    surface: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (binding, label)) in shortcuts.iter().enumerate() {
        if index > 0 {
            spans.push(disclosure_separator(theme, surface));
        }
        spans.extend(disclosure_shortcut(binding, label, theme, surface));
    }
    spans
}

fn composer_disclosure_hint_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    theme: &Theme,
    surface: Color,
) -> Vec<Vec<Span<'static>>> {
    if app.completed_session_shell_active() {
        let focus = shortcut_binding(app, Action::FocusNext).unwrap_or_else(|| "Tab".to_string());
        let commands =
            shortcut_binding(app, Action::Palette).unwrap_or_else(|| "Ctrl+p".to_string());
        let quit = shortcut_binding(app, Action::Quit).unwrap_or_else(|| "q".to_string());
        return vec![
            compose_shortcut_row(
                &[
                    (focus.as_str(), "focus"),
                    (commands.as_str(), "commands"),
                    (quit.as_str(), "quit"),
                ],
                theme,
                surface,
            ),
            compose_shortcut_row(
                &[(commands.as_str(), "commands"), (quit.as_str(), "quit")],
                theme,
                surface,
            ),
            compose_shortcut_row(&[(quit.as_str(), "quit")], theme, surface),
        ];
    }

    if dock.composer_disabled {
        let commands =
            shortcut_binding(app, Action::Palette).unwrap_or_else(|| "Ctrl+p".to_string());
        let quit = shortcut_binding(app, Action::Quit).unwrap_or_else(|| "q".to_string());
        return vec![
            compose_shortcut_row(
                &[(commands.as_str(), "commands"), (quit.as_str(), "quit")],
                theme,
                surface,
            ),
            compose_shortcut_row(&[(quit.as_str(), "quit")], theme, surface),
        ];
    }

    let commands = shortcut_binding(app, Action::Palette).unwrap_or_else(|| "Ctrl+p".to_string());
    vec![
        compose_shortcut_row(&[(commands.as_str(), "commands")], theme, surface),
        Vec::new(),
    ]
}

fn composer_disclosure_summary_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    theme: &Theme,
    surface: Color,
) -> Vec<Vec<Span<'static>>> {
    let runtime_state = app.runtime_state();
    let runtime_tone = runtime_kind_metadata_tone(runtime_state.kind);
    let mut candidates = Vec::new();
    let short_runtime_summary = runtime_state
        .summary
        .split(" · ")
        .next()
        .unwrap_or(runtime_state.summary.as_str())
        .to_string();

    candidates.push(vec![disclosure_segment(
        runtime_state.summary.clone(),
        runtime_tone,
        theme,
        surface,
    )]);

    candidates.push(vec![disclosure_segment(
        short_runtime_summary,
        runtime_tone,
        theme,
        surface,
    )]);

    if let Some(segment) = dock.summary_segment.as_ref() {
        let segment_tone = control_dock_summary_tone_to_metadata_tone(segment.tone);
        candidates.push(vec![disclosure_segment(
            format!("{}  ·  {}", runtime_state.summary, segment.text),
            segment_tone,
            theme,
            surface,
        )]);
        candidates.push(vec![disclosure_segment(
            segment.text.clone(),
            segment_tone,
            theme,
            surface,
        )]);
    }

    candidates.push(Vec::new());

    candidates
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

    assert_eq!(dock.status, None);
    assert_eq!(dock.shell.height, composer.height.saturating_add(1));
    assert_eq!(dock.shell.y, composer.y);
    assert_eq!(dock.disclosure, Some(Rect::new(0, 29, 100, 1)));

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
        composer_input_surface(&Theme::default())
    );
    assert_eq!(
        buffer[(
            right_edge,
            composer.y.saturating_add(composer.height.saturating_sub(1))
        )]
            .fg,
        composer_input_surface(&Theme::default())
    );
    assert_eq!(
        buffer[(right_edge, dock.shell.y)].bg,
        composer_input_surface(&Theme::default())
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
        theme.surface.shell
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
    assert!(!rendered.contains("q quit"));
}

#[cfg(test)]
pub(crate) fn exact_test_tool_status_summary_uses_effective_tool_identity() {
    let mut app = AppState::new_live(None, false, None);
    app.activities.push_front(ActivityEntry {
        request_id: "req_tool_identity".to_string(),
        model_id: "gpt-5.4".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: vec![crate::app::ToolCallEntry {
            tool_call_id: "tc_wrapper".to_string(),
            tool_id: "mcp.fixture.tool.call".to_string(),
            canonical_tool_id: None,
            alias_source_tool_id: None,
            resolved_tool_identity: Some(harness_core::event::ResolvedToolIdentity {
                invoked_tool_id: Some("mcp.fixture.tool.call".to_string()),
                effective_tool_id: Some("mcp.fixture.echo".to_string()),
                canonical_tool_id: None,
                alias_source_tool_id: None,
            }),
            args_summary: r#"{"tool":"echo"}"#.to_string(),
            args_digest: "digest-wrapper".to_string(),
            lifecycle_state: Some(harness_core::event::ToolCallLifecycleState::Running),
            status: ToolCallDisplayStatus::Running,
            output_summary: None,
            output_digest: None,
            output_json: None,
            truncated_output: None,
            edit: None,
            lineage: None,
            artifact_refs: Vec::new(),
            timing_elapsed_ms: None,
            permissions: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            first_timestamp: None,
            last_timestamp: None,
        }],
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    });

    let summary = control_dock_summary_segment(&app).expect("tool summary segment");
    assert_eq!(
        summary.kind,
        crate::view_model::ControlDockSummarySegmentKind::Tool
    );
    assert_eq!(
        summary.tone,
        crate::view_model::ControlDockSummaryTone::Accent
    );
    assert_eq!(summary.text, "tool mcp.fixture.echo running");
    assert!(!summary.text.contains("mcp.fixture.tool.call"));
}

#[cfg(test)]
pub(crate) fn exact_test_live_composer_reserves_right_gap() {
    let mut app = AppState::new_live(None, false, None);
    let mut events = crate::lib_tests::session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }

    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 160, 30));
    let dock = plan.dock.expect("live dock layout");

    assert!(plan.operator_sidebar.is_some());
    assert_eq!(dock.composer.x, dock.shell.x);
    assert_eq!(dock.composer.width.saturating_add(2), dock.shell.width);
}
