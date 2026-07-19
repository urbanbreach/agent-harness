// allow: SIZE_OK — TUI rendering (indivisible view model)
use super::*;

use ratatui::widgets::BorderType;

const LIFECYCLE_COPY_INSET_X: u16 = 3;
const WELCOME_PANEL_INSET_X: u16 = 3;
const WELCOME_PANEL_TOP_PAD: u16 = 7;
const WELCOME_PANEL_HEIGHT: u16 = 16;
const WELCOME_LOGO_WIDTH: usize = 14;
// Body height at 100x30≈24 / 120x32≈26; width 90 drops 80x24 (shell≈76).
const WELCOME_BORDERED_MIN_WIDTH: u16 = 90;
const WELCOME_BORDERED_MIN_HEIGHT: u16 = 22;
const COMPACT_WELCOME_TOP: u16 = 7;
const COMPACT_WELCOME_TOP_DENSE: u16 = 8;
const COMPACT_WELCOME_CONTENT_WIDTH: u16 = 51;
const COMPACT_WELCOME_NARROW_INSET_X: u16 = 5;
const COMPACT_WELCOME_NARROW_WIDTH: u16 = 70;
const STARTUP_CLIPBOARD_Y: u16 = 4;
const STARTUP_CLIPBOARD_Y_DENSE: u16 = 5;
const WELCOME_PANEL_MAX_WIDTH: u16 = 120;
const STARTUP_CLIPBOARD_WARNING_LINES: [&str; 2] = [
    "Clipboard may be unreachable.",
    "See /terminal-setup for potential fixes.",
];
const WELCOME_LOGO_ROWS: [&str; 7] = [
    "  ██╗  ██╗  ",
    "  ██║  ██║  ",
    "  ███████║  ",
    "  ██╔══██║  ",
    "  ██║  ██║  ",
    "  ╚═╝  ╚═╝  ",
    "            ",
];
const WELCOME_CHANGELOG_BULLETS: [&str; 3] = [
    "Event-sourced agent harness with compose-first TUI.",
    "Native tools, permissions, and offline mock dogfood.",
    "Replay-safe sessions with redacted provider metadata.",
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
    render_startup_breadcrumb(frame, app, area, theme);

    if app.composer.prompt_buffer.is_empty() {
        render_startup_clipboard_warning(frame, area, theme);
        render_welcome_panel(frame, app, area, theme);
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

fn render_startup_breadcrumb(frame: &mut Frame, app: &AppState, area: Rect, _theme: &Theme) {
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
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .bg(Color::Reset)
                .add_modifier(Modifier::DIM),
        ),
        row,
    );
}

pub(super) fn render_live_breadcrumb(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    _theme: &Theme,
) {
    if area.height < LIVE_BREADCRUMB_RESERVE_ROWS {
        return;
    }
    let row = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: 1,
    };
    let text = pack_breadcrumb_line(
        &live_breadcrumb_text(app),
        breadcrumb_context_meta(app).as_deref(),
        usize::from(row.width),
    );
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .bg(Color::Reset)
                .add_modifier(Modifier::DIM),
        ),
        row,
    );
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
    if area.height <= LIVE_BREADCRUMB_RESERVE_ROWS {
        return area;
    }
    Rect {
        x: area.x,
        y: area.y.saturating_add(LIVE_BREADCRUMB_RESERVE_ROWS),
        width: area.width,
        height: area.height.saturating_sub(LIVE_BREADCRUMB_RESERVE_ROWS),
    }
}

const STARTUP_DOCK_ROWS_ESTIMATE: u16 = 5;

fn is_dense_startup_area(area: Rect) -> bool {
    area.width <= 60
}

fn estimated_terminal_height(area: Rect) -> u16 {
    area.height.saturating_add(STARTUP_DOCK_ROWS_ESTIMATE)
}

fn startup_clipboard_y(area: Rect) -> u16 {
    if is_dense_startup_area(area) {
        return STARTUP_CLIPBOARD_Y_DENSE;
    }
    let terminal_h = estimated_terminal_height(area);
    // 100x30 freezes (run1/2/3) place clipboard at L4; 120x32+ stay on the base-4 scale.
    // Compact viewports (< bordered min width) keep base-4 so 80x24 remains L5.
    let base = if welcome_bordered_panel_fits(area) && terminal_h <= 30 {
        STARTUP_CLIPBOARD_Y.saturating_sub(1)
    } else {
        STARTUP_CLIPBOARD_Y
    };
    let scaled = base.saturating_add(terminal_h.saturating_sub(30) / 3);
    scaled.min(area.height.saturating_sub(4))
}

fn compact_welcome_top(area: Rect) -> u16 {
    if is_dense_startup_area(area) {
        COMPACT_WELCOME_TOP_DENSE
    } else {
        COMPACT_WELCOME_TOP
    }
}

fn render_startup_clipboard_warning(frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.height < 7 || area.width < 24 {
        return;
    }
    let _ = theme;
    let style = Style::default();
    let base_y = startup_clipboard_y(area);
    for (offset, line) in STARTUP_CLIPBOARD_WARNING_LINES.iter().enumerate() {
        let row = Rect {
            x: area.x,
            y: area
                .y
                .saturating_add(base_y + u16::try_from(offset).unwrap_or(0)),
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(truncate_plain_text(line, usize::from(row.width)))
                .style(style)
                .alignment(Alignment::Center),
            row,
        );
    }
}

fn welcome_bordered_panel_fits(area: Rect) -> bool {
    area.width >= WELCOME_BORDERED_MIN_WIDTH && area.height >= WELCOME_BORDERED_MIN_HEIGHT
}

fn welcome_panel_top_pad(area: Rect) -> u16 {
    let terminal_h = estimated_terminal_height(area);
    // Paired with startup_clipboard_y: 100x30 freeze welcome top L7; 120x32+ keep base-7 scale.
    let base = if welcome_bordered_panel_fits(area) && terminal_h <= 30 {
        WELCOME_PANEL_TOP_PAD.saturating_sub(1)
    } else {
        WELCOME_PANEL_TOP_PAD
    };
    let scaled = base.saturating_add(terminal_h.saturating_sub(30) / 3);
    scaled.min(area.height.saturating_sub(8))
}

fn welcome_panel_area(area: Rect) -> Option<Rect> {
    if area.width < 24 || area.height < 8 || !welcome_bordered_panel_fits(area) {
        return None;
    }
    let width = area
        .width
        .saturating_sub(WELCOME_PANEL_INSET_X.saturating_mul(2))
        .clamp(20, WELCOME_PANEL_MAX_WIDTH);
    let top_pad = welcome_panel_top_pad(area);
    let height = WELCOME_PANEL_HEIGHT.min(area.height.saturating_sub(top_pad).max(8));
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area.y.saturating_add(top_pad);
    Some(Rect::new(x, y, width, height))
}

fn welcome_action_rows() -> [(&'static str, Option<&'static str>); 4] {
    [
        ("New worktree", Some("ctrl+w")),
        ("Resume session", Some("ctrl+s")),
        ("Changelog", None),
        ("Quit", Some("ctrl+q")),
    ]
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

fn welcome_inner_lines(theme: &Theme, inner_width: usize) -> Vec<Line<'static>> {
    let surface = Color::Reset;
    let title_style = Style::default().bg(surface).add_modifier(Modifier::BOLD);
    let muted = Style::default().bg(surface);
    let body = Style::default().bg(surface);
    let logo_style = Style::default().bg(surface);
    let version = env!("CARGO_PKG_VERSION");
    let title = theme.live_shell.startup.title;
    let text_col = WELCOME_LOGO_WIDTH.saturating_add(5);
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
        let mut spans = vec![Span::styled(logo, logo_style)];
        match idx {
            0 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(&format!("  {title}  {version}"), inner_width),
                    title_style,
                ));
            }
            2 => {
                spans.push(Span::styled(
                    welcome_text_after_logo("  Changelog", inner_width),
                    title_style,
                ));
            }
            4 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(
                        &format!("   • {}", WELCOME_CHANGELOG_BULLETS[0]),
                        inner_width,
                    ),
                    muted,
                ));
            }
            5 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(
                        &format!("   • {}", WELCOME_CHANGELOG_BULLETS[1]),
                        inner_width,
                    ),
                    muted,
                ));
            }
            6 => {
                spans.push(Span::styled(
                    welcome_text_after_logo(
                        &format!("   • {}", WELCOME_CHANGELOG_BULLETS[2]),
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

    for (label, shortcut) in welcome_action_rows() {
        let pad = " ".repeat(text_col.min(inner_width));
        let label_text = truncate_plain_text(label, label_width);
        let mut row = format!("{pad}{label_text}");
        if let Some(shortcut) = shortcut {
            let used = row.chars().count();
            let target = inner_width.saturating_sub(shortcut.chars().count().saturating_add(2));
            if used < target {
                row.push_str(&" ".repeat(target.saturating_sub(used)));
            }
            row.push_str(shortcut);
        }
        lines.push(Line::from(Span::styled(
            truncate_plain_text(&row, inner_width),
            body.add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(Span::styled(" ", muted)));
    lines
}

fn compact_welcome_inset_x(area_width: u16) -> u16 {
    if area_width <= COMPACT_WELCOME_CONTENT_WIDTH {
        return COMPACT_WELCOME_NARROW_INSET_X.min(area_width.saturating_sub(8) / 2);
    }
    let spare = area_width.saturating_sub(COMPACT_WELCOME_CONTENT_WIDTH);
    ((spare + 1) / 2).min(area_width.saturating_sub(8) / 2)
}

fn compact_welcome_lines(
    _theme: &Theme,
    inner_width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let surface = Color::Reset;
    let body = Style::default().bg(surface).add_modifier(Modifier::BOLD);
    let muted = Style::default().bg(surface);
    let title_style = Style::default().bg(surface).add_modifier(Modifier::BOLD);
    let shortcut_width = 8usize;
    let label_width = inner_width
        .saturating_sub(shortcut_width)
        .saturating_sub(1)
        .max(8);

    let mut lines = Vec::new();
    for (label, shortcut) in welcome_action_rows() {
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
        lines.push(Line::from(Span::styled("Changelog", title_style)));
        lines.push(Line::from(Span::styled(" ", muted)));
        for bullet in WELCOME_CHANGELOG_BULLETS {
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

fn render_compact_welcome_body(frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(content) = compact_welcome_content_area(area) else {
        return;
    };
    if content.width == 0 || content.height == 0 {
        return;
    }
    let surface = Color::Reset;
    let lines = compact_welcome_lines(
        theme,
        usize::from(content.width),
        usize::from(content.height),
    );
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().bg(surface)),
        content,
    );
}

fn render_welcome_panel(frame: &mut Frame, _app: &AppState, area: Rect, theme: &Theme) {
    if let Some(panel) = welcome_panel_area(area) {
        let surface = Color::Reset;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().bg(surface))
            .style(Style::default().bg(surface));
        let inner = block.inner(panel);
        frame.render_widget(block, panel);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let lines = welcome_inner_lines(theme, usize::from(inner.width));
        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(surface)),
            inner,
        );
        return;
    }
    render_compact_welcome_body(frame, area, theme);
}

fn startup_lifecycle_flow_selection_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    if area.width == 0 || area.height == 0 || !app.composer.prompt_buffer.is_empty() {
        return None;
    }
    if let Some(panel) = welcome_panel_area(area) {
        let block = Block::default().borders(Borders::ALL);
        let content_area = block.inner(panel);
        if content_area.width == 0 || content_area.height == 0 {
            return None;
        }
        let text_rows = welcome_inner_lines(theme, usize::from(content_area.width))
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
    _theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Reset)),
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
}
