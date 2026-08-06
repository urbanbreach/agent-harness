// allow: SIZE_OK — TUI rendering (indivisible view model)
use super::*;

use crate::welcome_surface::{WelcomeFocus, WelcomeLayout};
use ratatui::widgets::{BorderType, Clear};

#[path = "app/first_prompt.rs"]
#[allow(
    dead_code,
    reason = "first-prompt composer focus helpers; wired for contract verification"
)]
mod first_prompt;
#[path = "app/trust_prompt.rs"]
#[allow(
    dead_code,
    reason = "trust prompt constants and helpers; consumed by render_trust_folder_prompt_overlay"
)]
mod trust_prompt;
#[path = "app/welcome.rs"]
#[allow(
    dead_code,
    reason = "view model helpers wired for contract use; not all consumed yet"
)]
mod welcome;

const LIFECYCLE_COPY_INSET_X: u16 = 3;
const WELCOME_PANEL_INSET_X: u16 = 3;
const WELCOME_PANEL_TOP_PAD: u16 = 4;
const WELCOME_PANEL_HEIGHT: u16 = 16;
const STARTUP_CLIPBOARD_WARNING: &str = "Clipboard may be unreachable.";
const STARTUP_CLIPBOARD_SETUP_HINT: &str = "Run /doctor for details and fixes.";
const WELCOME_LOGO_WIDTH: usize = 14;
const WELCOME_TEXT_COL: usize = 19;
// Body height at 100x30≈24 / 120x32≈26; width 90 drops 80x24 (shell≈76).
const WELCOME_BORDERED_MIN_WIDTH: u16 = 90;
const WELCOME_BORDERED_MIN_HEIGHT: u16 = 22;
const COMPACT_WELCOME_TOP: u16 = 7;
const COMPACT_WELCOME_TOP_DENSE: u16 = 8;
const COMPACT_WELCOME_CONTENT_WIDTH: u16 = 51;
const COMPACT_WELCOME_NARROW_INSET_X: u16 = 5;
const COMPACT_WELCOME_NARROW_WIDTH: u16 = 70;
const WELCOME_PANEL_MAX_WIDTH: u16 = 120;
const WELCOME_LOGO_ROWS: [&str; 7] = [
    "  ⠷        ⠾",
    "  ⣿        ⣿",
    "  ⣿        ⣿",
    "  ⣿⣶⣶⣶⣶⣶⣶⣶⣿",
    "  ⣿        ⣿",
    "  ⣿        ⣿",
    "  ⢿        ⢿",
];

#[derive(Debug, Clone)]
pub(super) struct LifecycleSelectionSurface {
    pub viewport: Rect,
    pub text_rows: Vec<LifecycleSelectableText>,
}

#[derive(Debug, Clone)]
pub(super) struct LifecycleSelectableText {
    pub row: usize,
    pub max_height: u16,
    pub line: Line<'static>,
    pub alignment: Alignment,
}

fn lifecycle_surface_copy_area(area: Rect) -> Rect {
    inset_rect(
        area,
        LIFECYCLE_COPY_INSET_X.min(area.width.saturating_sub(1) / 2),
        0,
    )
}

fn lifecycle_surface_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
) -> Block<'a> {
    ui_chrome::message_surface(theme, title, is_focused, theme.surface.panel_elevated)
}

fn render_lifecycle_copy_line(
    frame: &mut Frame,
    area: Rect,
    text: &str,
    style: Style,
    alignment: Alignment,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(truncate_plain_text(text, usize::from(area.width)))
            .style(style)
            .alignment(alignment),
        area,
    );
}

pub(super) fn live_empty_state_visible(app: &AppState) -> bool {
    !app.replay_mode
        && !app.startup_shell_visible()
        && app.activities.is_empty()
        && app.active_permission_view().is_none()
        && app.transcript_pending_permissions().is_empty()
        && app.composer.prompt_buffer.is_empty()
}

pub(super) fn startup_shell_visible(app: &AppState) -> bool {
    app.startup_shell_visible()
}

pub(crate) fn render_startup_lifecycle_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    render_startup_lifecycle_flow(frame, app, area, theme);
}

pub(super) fn startup_lifecycle_selection_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    startup_lifecycle_flow_selection_surface(app, area, theme)
}

pub(crate) fn render_startup_lifecycle_flow(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = Color::Reset;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    let welcome_layout = app.welcome_layout(area);
    let welcome_hit_map = app.welcome_hit_map(area);
    debug_assert_eq!(welcome_hit_map.layout(), &welcome_layout);
    render_startup_breadcrumb(frame, app, area, theme);

    if app.composer.prompt_buffer.is_empty() {
        render_startup_clipboard_warning(frame, app, area, theme);
        render_welcome_panel(frame, app, area, theme, &welcome_layout);
    }

    // Trust prompt z-order: render on top of the startup content so the
    // folder-trust dialog overlays the welcome panel and breadcrumb.
    if app.trust_folder_prompt_visible {
        render_trust_folder_prompt_overlay(frame, area, theme);
    }
}

fn render_startup_clipboard_warning(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if !startup_clipboard_warning_visible(app) || area.width <= 4 || area.height <= 4 {
        return;
    }
    for (offset, line) in [STARTUP_CLIPBOARD_WARNING, STARTUP_CLIPBOARD_SETUP_HINT]
        .into_iter()
        .enumerate()
    {
        let row = Rect::new(
            area.x,
            area.y
                .saturating_add(4)
                .saturating_add(u16::try_from(offset).unwrap_or(0)),
            area.width,
            1,
        );
        frame.render_widget(
            Paragraph::new(truncate_plain_text(line, usize::from(row.width)))
                .style(
                    Style::default()
                        .fg(theme.text.secondary)
                        .bg(theme.surface.canvas),
                )
                .alignment(Alignment::Center),
            row,
        );
    }
}

fn startup_breadcrumb_text(app: &AppState) -> String {
    let label = app.startup_directory_branch_label();
    if let Some((path, branch)) = label.rsplit_once(':') {
        let branch = branch.trim();
        if !branch.is_empty() && !branch.contains('/') && !branch.contains('\\') {
            let _ = path;
            return format!("   {branch} ~");
        }
    }
    format!("  {label}")
}

fn live_breadcrumb_text(app: &AppState) -> String {
    let label = app.startup_directory_branch_label();
    if let Some((path, branch)) = label.rsplit_once(':') {
        let branch = branch.trim();
        if !branch.is_empty() && !branch.contains('/') && !branch.contains('\\') {
            return format!("   {branch} {path}");
        }
    }
    format!("  {label}")
}

pub(super) const LIVE_BREADCRUMB_RESERVE_ROWS: u16 = 2;

fn render_startup_breadcrumb(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if area.height < 2 {
        return;
    }
    let row = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: 1,
    };
    let text = truncate_plain_text(&startup_breadcrumb_text(app), usize::from(row.width));
    let text_width = u16::try_from(super::display_width(&text))
        .unwrap_or(u16::MAX)
        .min(row.width);
    let row = Rect {
        width: text_width,
        ..row
    };
    let dim = Style::default()
        .fg(theme.text.tertiary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::DIM);
    let line = if let Some(prefix) = text.strip_suffix(" ~") {
        Line::from(vec![
            Span::styled(prefix.to_string(), dim),
            Span::styled(
                " ~",
                Style::default()
                    .fg(theme.text.primary)
                    .bg(theme.surface.canvas),
            ),
        ])
    } else {
        Line::from(Span::styled(text, dim))
    };
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.surface.canvas)),
        row,
    );
}

pub(super) fn render_live_breadcrumb(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let reserve = crate::layout::breadcrumb_reserve_rows(area.width);
    if area.height < reserve {
        return;
    }
    let row = Rect {
        x: area.x,
        y: area
            .y
            .saturating_add(crate::layout::breadcrumb_top_margin(area.width)),
        width: area.width,
        height: 1,
    };
    let context_meta = breadcrumb_context_meta(app);
    let text = pack_breadcrumb_line(
        &live_breadcrumb_text(app),
        context_meta.as_deref(),
        usize::from(row.width),
    );
    let dim = Style::default()
        .fg(theme.text.tertiary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::DIM);
    let line = context_meta
        .as_deref()
        .filter(|meta| text.ends_with(*meta))
        .map_or_else(
            || Line::from(Span::styled(text.clone(), dim)),
            |meta| {
                let split = text.len().saturating_sub(meta.len());
                Line::from(vec![
                    Span::styled(text[..split].to_string(), dim),
                    Span::styled(
                        meta.to_string(),
                        Style::default().fg(theme.text.primary).bg(Color::Reset),
                    ),
                ])
            },
        );
    frame.render_widget(Paragraph::new(line), row);
}

/// Freeze breadcrumb right meta: `12K / 262K` (uppercase K, space slash).
fn breadcrumb_context_meta(app: &AppState) -> Option<String> {
    let used = app
        .active_context_usage()?
        .tokens
        .filter(|tokens| *tokens > 0)?;
    let limit = app
        .current_context_window_tokens()
        .filter(|limit| *limit > 0)?;
    Some(format!(
        "{} / {}",
        format_breadcrumb_token_count(used),
        format_breadcrumb_token_count(limit)
    ))
}

/// Freeze-matched compact token counts: `12K`, `1.5K`, `262K` (not `12.0K` / `262.1K`).
fn format_breadcrumb_token_count(count: u32) -> String {
    if count < 1_000 {
        return count.to_string();
    }
    if count < 1_000_000 {
        if count.is_multiple_of(1_000) {
            return format!("{}K", count / 1_000);
        }
        // Context windows like 262144 freeze as whole thousands (`262K`).
        if count >= 100_000 {
            return format!("{}K", (count.saturating_add(500)) / 1_000);
        }
        let thousands = f64::from(count) / 1_000.0;
        return format!("{thousands:.1}K");
    }
    let millions = f64::from(count) / 1_000_000.0;
    if count.is_multiple_of(1_000_000) {
        format!("{}M", count / 1_000_000)
    } else {
        format!("{millions:.1}M")
    }
}

fn pack_breadcrumb_line(left: &str, meta: Option<&str>, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let Some(meta) = meta.filter(|value| !value.is_empty()) else {
        return truncate_plain_text(left, width);
    };
    let meta_width = super::display_width(meta);
    if meta_width >= width {
        return truncate_plain_text(meta, width);
    }
    let left_budget = width.saturating_sub(meta_width);
    let left = truncate_plain_text(left, left_budget);
    let left_width = super::display_width(&left);
    let pad = left_budget.saturating_sub(left_width);
    format!("{left}{}{meta}", " ".repeat(pad))
}

pub(super) fn live_transcript_area_with_breadcrumb(area: Rect) -> Rect {
    let reserve = crate::layout::breadcrumb_reserve_rows(area.width);
    if area.height <= reserve {
        return area;
    }
    Rect {
        x: area.x,
        y: area.y.saturating_add(reserve),
        width: area.width,
        height: area.height.saturating_sub(reserve),
    }
}

const STARTUP_DOCK_ROWS_ESTIMATE: u16 = 5;

fn is_dense_startup_area(area: Rect) -> bool {
    area.width <= 60
}

fn estimated_terminal_height(area: Rect) -> u16 {
    area.height.saturating_add(STARTUP_DOCK_ROWS_ESTIMATE)
}

fn compact_welcome_top(area: Rect) -> u16 {
    if is_dense_startup_area(area) {
        COMPACT_WELCOME_TOP_DENSE
    } else {
        COMPACT_WELCOME_TOP
    }
}

fn welcome_bordered_panel_fits(area: Rect) -> bool {
    area.width >= WELCOME_BORDERED_MIN_WIDTH && area.height >= WELCOME_BORDERED_MIN_HEIGHT
}

fn startup_clipboard_warning_visible(app: &AppState) -> bool {
    app.status_banner
        .as_deref()
        .is_some_and(startup_banner_is_clipboard_warning)
}

fn startup_banner_is_clipboard_warning(banner: &str) -> bool {
    let normalized = banner.to_ascii_lowercase();
    normalized.contains("clipboard")
        && (normalized.contains("unreachable") || normalized.contains("inaccessible"))
}

fn welcome_panel_top_pad(area: Rect, clipboard_warning_visible: bool) -> u16 {
    let terminal_h = estimated_terminal_height(area);
    let base = if welcome_bordered_panel_fits(area) && terminal_h <= 30 {
        WELCOME_PANEL_TOP_PAD.saturating_sub(1)
    } else {
        WELCOME_PANEL_TOP_PAD
    };
    let scaled = base
        .saturating_add(terminal_h.saturating_sub(30) / 3)
        .saturating_add(if clipboard_warning_visible { 3 } else { 0 });
    scaled.min(area.height.saturating_sub(8))
}

fn welcome_panel_area(
    area: Rect,
    clipboard_warning_visible: bool,
    layout: &WelcomeLayout,
) -> Option<Rect> {
    if area.width < 24 || area.height < 8 || !welcome_bordered_panel_fits(area) {
        return None;
    }
    let width = area
        .width
        .saturating_sub(WELCOME_PANEL_INSET_X.saturating_mul(2))
        .clamp(20, WELCOME_PANEL_MAX_WIDTH);
    let top_pad =
        welcome_panel_top_pad(area, clipboard_warning_visible).saturating_add(layout.hero_rect.1);
    let height = WELCOME_PANEL_HEIGHT.min(area.height.saturating_sub(top_pad).max(8));
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area.y.saturating_add(top_pad);
    Some(Rect::new(x, y, width, height))
}

fn pad_logo_cell(row: &str) -> String {
    let mut out = row.to_string();
    while out.chars().count() < WELCOME_LOGO_WIDTH {
        out.push(' ');
    }
    out.chars().take(WELCOME_LOGO_WIDTH).collect()
}

fn welcome_text_after_logo(text: &str, inner_width: usize) -> String {
    let budget = inner_width.saturating_sub(WELCOME_LOGO_WIDTH).max(1);
    truncate_plain_text(text, budget)
}

fn welcome_inner_lines(theme: &Theme, inner_width: usize, app: &AppState) -> Vec<Line<'static>> {
    let surface = theme.surface.canvas;
    let title_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(theme.text.secondary).bg(surface);
    let body = Style::default().fg(theme.text.primary).bg(surface);
    let dim_style = Style::default()
        .fg(theme.text.tertiary)
        .bg(surface)
        .add_modifier(Modifier::DIM);
    let identity = welcome::welcome_identity(theme.live_shell.startup.title);
    let changelog_bullets = welcome::changelog_bullets();
    let text_col = WELCOME_TEXT_COL;
    let shortcut_width = 8usize;
    let label_width = inner_width
        .saturating_sub(text_col)
        .saturating_sub(shortcut_width)
        .saturating_sub(2)
        .max(12);

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(" ", muted)));

    for (idx, logo_row) in WELCOME_LOGO_ROWS.iter().enumerate() {
        let logo = pad_logo_cell(logo_row);
        let mut spans = super::ui_startup_logo::animated_logo_line(
            idx,
            WELCOME_LOGO_ROWS.len(),
            &logo,
            app.animation_phase_for_evidence(),
            theme,
            surface,
        )
        .spans;
        match idx {
            0 => {
                let title_text = format!("     {}  ", identity.title);
                let version_text = welcome_text_after_logo(
                    &format!("{title_text}{}", identity.version),
                    inner_width,
                );
                let title_prefix = welcome_text_after_logo(&title_text, inner_width);
                let title_len = title_prefix.chars().count();
                if version_text.chars().count() > title_len {
                    spans.push(Span::styled(title_prefix, title_style));
                    spans.push(Span::styled(
                        version_text.chars().skip(title_len).collect::<String>(),
                        muted,
                    ));
                } else {
                    spans.push(Span::styled(version_text, title_style));
                }
            }
            2 => {
                spans.push(Span::styled(
                    welcome_text_after_logo("     Changelog", inner_width),
                    dim_style,
                ));
            }
            4 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(
                        &format!("      • {}", changelog_bullets[0]),
                        inner_width,
                    ),
                    muted,
                ));
            }
            5 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(
                        &format!("      • {}", changelog_bullets[1]),
                        inner_width,
                    ),
                    muted,
                ));
            }
            6 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(
                        &format!("      • {}", changelog_bullets[2]),
                        inner_width,
                    ),
                    muted,
                ));
            }
            _ => {}
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(Span::styled(" ", muted)));

    for action in welcome::welcome_actions() {
        let label = action.label;
        let shortcut = action.shortcut;
        let pad = " ".repeat(text_col.min(inner_width));
        let label_text = truncate_plain_text(label, label_width);
        let label_width_actual = super::display_width(&label_text);
        let mut spans: Vec<Span<'static>> = vec![
            Span::styled(pad, muted),
            Span::styled(label_text.to_string(), body.add_modifier(Modifier::BOLD)),
        ];
        if let Some(shortcut) = shortcut {
            let shortcut_chars = shortcut.chars().count();
            let used = text_col.min(inner_width) + label_width_actual;
            let target = inner_width.saturating_sub(shortcut_chars.saturating_add(2));
            if used < target {
                spans.push(Span::styled(" ".repeat(target.saturating_sub(used)), muted));
            }
            spans.push(Span::styled(shortcut.to_string(), muted));
        }
        lines.push(Line::from(spans));
    }

    // Local notices section: derived from app status banner (e.g. missing
    // provider). Replaces the trailing blank so the panel height stays stable.
    if let Some(banner) = app
        .status_banner
        .as_deref()
        .filter(|banner| !banner.is_empty() && !startup_banner_is_clipboard_warning(banner))
    {
        let label = "Notices: ";
        let label_width = super::display_width(label);
        let budget = inner_width.saturating_sub(label_width).max(1);
        let notice_text = truncate_plain_text(banner, budget);
        lines.push(Line::from(vec![
            Span::styled(label.to_string(), dim_style),
            Span::styled(notice_text, muted),
        ]));
    } else {
        lines.push(Line::from(Span::styled(" ", muted)));
    }

    lines
}

fn compact_welcome_inset_x(area_width: u16) -> u16 {
    if area_width <= COMPACT_WELCOME_CONTENT_WIDTH {
        return COMPACT_WELCOME_NARROW_INSET_X.min(area_width.saturating_sub(8) / 2);
    }
    let spare = area_width.saturating_sub(COMPACT_WELCOME_CONTENT_WIDTH);
    spare.div_ceil(2).min(area_width.saturating_sub(8) / 2)
}

fn compact_welcome_lines(
    _theme: &Theme,
    inner_width: usize,
    max_lines: usize,
    app: &AppState,
) -> Vec<Line<'static>> {
    let surface = _theme.surface.canvas;
    let body = Style::default()
        .fg(_theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(_theme.text.secondary).bg(surface);
    let dim_style = Style::default()
        .fg(_theme.text.tertiary)
        .bg(surface)
        .add_modifier(Modifier::DIM);
    let shortcut_width = 8usize;
    let label_width = inner_width
        .saturating_sub(shortcut_width)
        .saturating_sub(1)
        .max(8);

    let mut lines = Vec::new();
    for action in welcome::welcome_actions() {
        let label = action.label;
        let shortcut = action.shortcut;
        if lines.len() >= max_lines {
            break;
        }
        let label_text = truncate_plain_text(label, label_width);
        let Some(shortcut) = shortcut else {
            lines.push(Line::from(Span::styled(
                truncate_plain_text(&label_text, inner_width),
                body,
            )));
            continue;
        };
        let used = label_text.chars().count();
        let target = inner_width.saturating_sub(shortcut.chars().count());
        let gap = if used < target {
            " ".repeat(target.saturating_sub(used))
        } else {
            String::new()
        };
        let row_width = used
            .saturating_add(gap.chars().count())
            .saturating_add(shortcut.chars().count());
        let mut spans = vec![
            Span::styled(label_text, body),
            Span::styled(gap, muted),
            Span::styled(shortcut.to_string(), muted),
        ];
        if row_width < inner_width {
            spans.push(Span::styled(
                " ".repeat(inner_width.saturating_sub(row_width)),
                muted,
            ));
        }
        lines.push(Line::from(spans));
    }
    let changelog_min_lines = 2 + 1 + 1 + 1;
    if max_lines.saturating_sub(lines.len()) >= changelog_min_lines {
        lines.push(Line::from(Span::styled(" ", muted)));
        lines.push(Line::from(Span::styled("Changelog", dim_style)));
        lines.push(Line::from(Span::styled(" ", muted)));
        for bullet in welcome::changelog_bullets() {
            if lines.len() >= max_lines {
                break;
            }
            let text = format!("• {bullet}");
            lines.push(Line::from(Span::styled(
                truncate_plain_text(&text, inner_width),
                muted,
            )));
        }
    }
    // Compact local notices: show the status banner if present.
    if lines.len() < max_lines {
        if let Some(banner) = app
            .status_banner
            .as_deref()
            .filter(|banner| !banner.is_empty() && !startup_banner_is_clipboard_warning(banner))
        {
            let label = "Notices: ";
            let label_width = super::display_width(label);
            let budget = inner_width.saturating_sub(label_width).max(1);
            let notice_text = truncate_plain_text(banner, budget);
            lines.push(Line::from(vec![
                Span::styled(label.to_string(), dim_style),
                Span::styled(notice_text, muted),
            ]));
        }
    }
    lines
}

fn compact_welcome_content_area(area: Rect) -> Option<Rect> {
    if area.width < 24 || area.height < 12 {
        return None;
    }
    let top = compact_welcome_top(area).min(area.height.saturating_sub(4));
    let inset = compact_welcome_inset_x(area.width);
    let width = COMPACT_WELCOME_CONTENT_WIDTH
        .min(area.width.saturating_sub(inset))
        .max(16);
    let height = area.height.saturating_sub(top).max(1);
    Some(Rect::new(
        area.x.saturating_add(inset),
        area.y.saturating_add(top),
        width,
        height,
    ))
}

fn render_compact_welcome_body(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let Some(content) = compact_welcome_content_area(area) else {
        return;
    };
    if content.width == 0 || content.height == 0 {
        return;
    }
    let surface = theme.surface.canvas;
    let lines = compact_welcome_lines(
        theme,
        usize::from(content.width),
        usize::from(content.height),
        app,
    );
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().bg(surface)),
        content,
    );
}

fn render_welcome_panel(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    layout: &WelcomeLayout,
) {
    if let Some(panel) = welcome_panel_area(area, startup_clipboard_warning_visible(app), layout) {
        let surface = theme.surface.canvas;
        let border_color = match app.welcome_state().focus() {
            WelcomeFocus::Prompt => theme.border.focus,
            WelcomeFocus::Menu(_) | WelcomeFocus::StatusBar => theme.border.subtle,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(surface))
            .style(Style::default().bg(surface));
        let inner = block.inner(panel);
        frame.render_widget(block, panel);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let lines = welcome_inner_lines(theme, usize::from(inner.width), app);
        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(surface)),
            inner,
        );
        return;
    }
    render_compact_welcome_body(frame, app, area, theme);
}

/// Render the folder-trust prompt as a centered overlay dialog.
///
/// Display-only: the actual trust decision is persisted through the
/// coordinator when the operator confirms. This renders on top of the
/// startup content (welcome panel + breadcrumb) with correct z-order.
fn render_trust_folder_prompt_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
    let root = frame.area();

    // Dim the backdrop so the dialog stands out.
    frame.render_widget(Block::default().style(Style::default().dim()), root);

    let width = 56u16.min(root.width.saturating_sub(4));
    let height = 9u16.min(root.height.saturating_sub(4));
    if width < 28 || height < 5 {
        return;
    }
    let x = root.x + (root.width.saturating_sub(width)) / 2;
    let y = root.y + (root.height.saturating_sub(height)) / 2;
    let dialog = Rect::new(x, y, width, height);

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(dialog);

    let surface = theme.surface.overlay;
    let accent = theme.text.accent;
    let muted = theme.text.secondary;
    let text = theme.text.primary;
    let border = Style::default().fg(theme.border.subtle).bg(surface);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            trust_prompt::TRUST_PROMPT_TITLE,
            Style::default().fg(accent).bg(surface),
        ))
        .border_style(border);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);

    let body_lines: Vec<Line> = trust_prompt::trust_prompt_body_lines()
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let style = if i == 1 {
                Style::default().fg(text).bg(surface)
            } else {
                Style::default().fg(muted).bg(surface)
            };
            Line::from(Span::styled(
                truncate_plain_text(line, usize::from(chunks[1].width)),
                style,
            ))
        })
        .collect();
    let body = Paragraph::new(body_lines).alignment(Alignment::Left);
    frame.render_widget(body, chunks[1]);

    let hints = trust_prompt::trust_prompt_footer_hints();
    let footer_spans: Vec<Span> = hints
        .iter()
        .enumerate()
        .flat_map(|(i, hint)| {
            let style = if i == 0 {
                Style::default().fg(accent).bg(surface)
            } else {
                Style::default().fg(muted).bg(surface)
            };
            let mut spans = vec![Span::styled(*hint, style)];
            if i < hints.len() - 1 {
                spans.push(Span::styled("  ", Style::default().bg(surface)));
            }
            spans
        })
        .collect();
    let footer = Paragraph::new(vec![Line::from(footer_spans)]).alignment(Alignment::Center);
    frame.render_widget(footer, chunks[2]);

    let _ = area; // area is the content rect; the overlay uses the full frame.
}

fn startup_lifecycle_flow_selection_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    if area.width == 0 || area.height == 0 || !app.composer.prompt_buffer.is_empty() {
        return None;
    }
    let welcome_layout = app.welcome_layout(area);
    if let Some(panel) = welcome_panel_area(
        area,
        startup_clipboard_warning_visible(app),
        &welcome_layout,
    ) {
        let block = Block::default().borders(Borders::ALL);
        let content_area = block.inner(panel);
        if content_area.width == 0 || content_area.height == 0 {
            return None;
        }
        let text_rows = welcome_inner_lines(theme, usize::from(content_area.width), app)
            .into_iter()
            .enumerate()
            .map(|(idx, line)| LifecycleSelectableText {
                row: idx,
                max_height: 1,
                line,
                alignment: Alignment::Left,
            })
            .collect();
        return Some(LifecycleSelectionSurface {
            viewport: content_area,
            text_rows,
        });
    }
    let content_area = compact_welcome_content_area(area)?;
    if content_area.width == 0 || content_area.height == 0 {
        return None;
    }
    let text_rows = compact_welcome_lines(
        theme,
        usize::from(content_area.width),
        usize::from(content_area.height),
        app,
    )
    .into_iter()
    .enumerate()
    .map(|(idx, line)| LifecycleSelectableText {
        row: idx,
        max_height: 1,
        line,
        alignment: Alignment::Left,
    })
    .collect();
    Some(LifecycleSelectionSurface {
        viewport: content_area,
        text_rows,
    })
}

pub(super) fn render_live_empty_state(
    frame: &mut Frame,
    _app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.canvas)),
        area,
    );
}

pub(super) fn live_empty_state_selection_surface(
    _app: &AppState,
    _area: Rect,
    _theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    None
}

#[cfg(test)]
mod breadcrumb_token_meta_tests {
    use super::{format_breadcrumb_token_count, pack_breadcrumb_line};

    #[test]
    fn format_breadcrumb_token_count_matches_freeze_style() {
        // arrange
        // act
        // assert
        assert_eq!(format_breadcrumb_token_count(42), "42");
        assert_eq!(format_breadcrumb_token_count(1_500), "1.5K");
        assert_eq!(format_breadcrumb_token_count(10_000), "10K");
        assert_eq!(format_breadcrumb_token_count(12_000), "12K");
        assert_eq!(format_breadcrumb_token_count(262_144), "262K");
    }

    #[test]
    fn pack_breadcrumb_line_right_aligns_token_meta() {
        // arrange
        // act
        // assert
        let packed = pack_breadcrumb_line("   main ~/proj", Some("12K / 262K"), 40);
        assert!(packed.ends_with("12K / 262K"), "packed={packed:?}");
        assert_eq!(super::super::display_width(&packed), 40);
        assert!(
            packed.contains("main") || packed.contains("proj"),
            "packed={packed:?}"
        );
    }

    #[test]
    fn pack_breadcrumb_line_without_meta_truncates_left_only() {
        // arrange
        // act
        // assert
        let packed = pack_breadcrumb_line("   main ~/very/long/path/here", None, 20);
        assert_eq!(super::super::display_width(&packed), 20);
        assert!(
            packed.starts_with("  "),
            "left breadcrumb retained when meta absent: {packed:?}"
        );
        assert!(!packed.contains("12K / 262K"), "packed={packed:?}");
    }

    #[test]
    fn startup_breadcrumb_does_not_dim_trailing_blank_cells() {
        use ratatui::{
            backend::TestBackend,
            style::{Color, Modifier, Style},
            widgets::Block,
            Terminal,
        };

        // Given: the real startup renderer at the standard canary viewport.
        let app = crate::app::AppState::new_startup(Vec::new(), None);
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = app.theme();

        // When: the live startup shell is rendered.
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Block::default().style(Style::default().bg(Color::Reset)),
                    frame.area(),
                );
                super::render_startup_breadcrumb(frame, &app, frame.area(), &theme);
            })
            .expect("startup render");

        // Then: a cell after the visible breadcrumb retains the canvas style.
        let text = super::startup_breadcrumb_text(&app);
        let text_width = u16::try_from(super::super::display_width(&text))
            .expect("breadcrumb fits standard viewport");
        let cell = &terminal.backend().buffer()[(text_width + 1, 1)];
        assert_eq!(cell.bg, Color::Reset);
        assert!(!cell.modifier.contains(Modifier::DIM));
    }
}
