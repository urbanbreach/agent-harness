// allow: SIZE_OK — TUI rendering (indivisible view model)
use super::*;

use crate::welcome_surface::{WelcomeFocus, WelcomeLayout};
use ratatui::widgets::{BorderType, Clear};
use unicode_width::UnicodeWidthStr;

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
const STARTUP_CLIPBOARD_WARNING: &str = "Clipboard may be unreachable.";
const STARTUP_CLIPBOARD_SETUP_HINT: &str = "Run /doctor for details and fixes.";
const WELCOME_ACTION_COL: usize = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WelcomeActionEmphasis {
    Default,
    Hovered,
    Focused,
}

struct WelcomeActionLine {
    label: &'static str,
    shortcut: &'static str,
    width: usize,
    emphasis: WelcomeActionEmphasis,
}

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

fn rect_from_tuple((x, y, width, height): (u16, u16, u16, u16)) -> Rect {
    Rect::new(x, y, width, height)
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

pub(crate) fn live_empty_state_visible(app: &AppState) -> bool {
    !app.replay_mode
        && !app.startup_shell_visible()
        && app.activities.is_empty()
        && app.active_permission_view().is_none()
        && app.transcript_pending_permissions().is_empty()
        && app.composer.prompt_buffer.is_empty()
}

pub(crate) fn live_empty_composer_guidance_visible(app: &AppState) -> bool {
    live_empty_state_visible(app)
        && app.events.is_empty()
        && !app.completed_session_shell_active()
        && !app.composer_disabled()
        && app.review_surface().is_none()
        && !app.overlay_state().command_palette_channel_visible()
        && !app.overlay_state().permission_pending
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

    let surface = theme.surface.canvas;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    render_startup_breadcrumb(frame, app, area, theme);

    if app.welcome_visible() {
        render_welcome_panel(frame, app, area, theme);
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
    let (prefix, path) = startup_breadcrumb_parts(app);
    if path.is_empty() {
        prefix
    } else {
        format!("{prefix} {path}")
    }
}

fn startup_breadcrumb_parts(app: &AppState) -> (String, String) {
    let facts = &app.workspace_display;
    let branch = facts
        .branch
        .as_ref()
        .map(|branch| format!("git:{branch}"))
        .or_else(|| facts.detached.then(|| "git:detached".to_string()))
        .unwrap_or_default();
    let provenance = if facts.linked_worktree {
        " worktree"
    } else {
        ""
    };
    (format!("  {branch}{provenance}"), facts.directory.clone())
}

fn live_breadcrumb_text(app: &AppState, width: u16) -> String {
    let (prefix, path) = startup_breadcrumb_parts(app);
    format!(
        "{}{} {}",
        " ".repeat(usize::from(crate::layout::composer_horizontal_inset(width))),
        prefix.trim(),
        path
    )
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
    let (prefix, path) = startup_breadcrumb_parts(app);
    let prefix = truncate_plain_text(&prefix, usize::from(row.width));
    let prefix_width = super::display_width(&prefix);
    let path = truncate_plain_text(
        &path,
        usize::from(row.width).saturating_sub(prefix_width.saturating_add(1)),
    );
    let text_width = prefix_width
        .saturating_add(usize::from(!path.is_empty()))
        .saturating_add(super::display_width(&path));
    let text_width = u16::try_from(text_width).unwrap_or(u16::MAX).min(row.width);
    let row = Rect {
        width: text_width,
        ..row
    };
    let dim = Style::default()
        .fg(theme.text.tertiary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::DIM);
    if let Some(dimmed_prefix) = prefix.strip_prefix("  ") {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  ", Style::default().bg(theme.surface.canvas)),
                Span::styled(
                    dimmed_prefix.to_string(),
                    dim.fg(theme.text.accent).remove_modifier(Modifier::DIM),
                ),
            ])),
            Rect {
                width: u16::try_from(prefix_width)
                    .unwrap_or(row.width)
                    .min(row.width),
                ..row
            },
        );
        if !path.is_empty() {
            let path_x = row
                .x
                .saturating_add(u16::try_from(prefix_width.saturating_add(1)).unwrap_or(row.width));
            frame.render_widget(
                Paragraph::new(Span::styled(
                    path,
                    Style::default()
                        .fg(theme.text.primary)
                        .bg(theme.surface.canvas),
                )),
                Rect {
                    x: path_x,
                    width: row.right().saturating_sub(path_x),
                    ..row
                },
            );
        }
    } else {
        frame.render_widget(Paragraph::new(Span::styled(prefix, dim)), row);
    }
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
        &live_breadcrumb_text(app, area.width),
        context_meta.as_deref(),
        usize::from(row.width),
    );
    let dim = Style::default()
        .fg(theme.text.tertiary)
        .bg(theme.surface.canvas)
        .add_modifier(Modifier::DIM);
    let metadata = context_meta.as_deref().filter(|meta| text.ends_with(*meta));
    let split = text.len().saturating_sub(metadata.map_or(0, str::len));
    let left = &text[..split];
    let (prefix, _) = startup_breadcrumb_parts(app);
    let prefix = format!(
        "{}{}",
        " ".repeat(usize::from(crate::layout::composer_horizontal_inset(
            area.width
        ))),
        prefix.trim()
    );
    let branch_end = if prefix.trim().is_empty() {
        0
    } else {
        left.char_indices()
            .nth(prefix.chars().count())
            .map_or(left.len(), |(byte, _)| byte)
    };
    let mut spans = vec![
        Span::styled(
            left[..branch_end].to_owned(),
            dim.fg(theme.text.accent).remove_modifier(Modifier::DIM),
        ),
        Span::styled(
            left[branch_end..].to_owned(),
            Style::default()
                .fg(theme.text.primary)
                .bg(theme.surface.canvas),
        ),
    ];
    if let Some(meta) = metadata {
        spans.push(Span::styled(
            meta.to_owned(),
            Style::default()
                .fg(theme.text.primary)
                .bg(theme.surface.canvas),
        ));
    }
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), row);
}

/// Freeze breadcrumb right meta: `12K / 262K` (uppercase K, space slash).
fn breadcrumb_context_meta(app: &AppState) -> Option<String> {
    let snapshot = app.current_request_budget_snapshot()?;
    if snapshot.status != harness_core::context_budget::BudgetStatus::Estimated {
        return None;
    }
    let occupied =
        (snapshot.occupied_input_tokens > 0).then_some(snapshot.occupied_input_tokens)?;
    let threshold = snapshot
        .compaction_threshold_tokens
        .filter(|threshold| *threshold > 0)?;
    Some(format!(
        "{} / {}",
        format_breadcrumb_token_count(occupied),
        format_breadcrumb_token_count(threshold)
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

fn welcome_text_after_logo(text: &str, inner_width: usize, logo_width: usize) -> String {
    let budget = inner_width.saturating_sub(logo_width).max(1);
    truncate_plain_text(text, budget)
}

fn welcome_action_emphasis(app: &AppState, index: usize) -> WelcomeActionEmphasis {
    if app.welcome_state().focus() == WelcomeFocus::Menu(index) {
        WelcomeActionEmphasis::Focused
    } else if app.welcome_state().hovered_action() == Some(index) {
        WelcomeActionEmphasis::Hovered
    } else {
        WelcomeActionEmphasis::Default
    }
}

fn welcome_changelog_section_style(theme: &Theme, app: &AppState) -> Style {
    let surface = theme.surface.canvas;
    if app.welcome_state().hovered_action() == Some(2) {
        Style::default().fg(theme.text.primary).bg(surface)
    } else {
        Style::default()
            .fg(theme.text.secondary)
            .bg(surface)
            .add_modifier(Modifier::DIM)
    }
}

fn welcome_action_spans(theme: &Theme, row: WelcomeActionLine) -> Vec<Span<'static>> {
    if row.width == 0 {
        return Vec::new();
    }

    let surface = match row.emphasis {
        WelcomeActionEmphasis::Default => theme.surface.canvas,
        WelcomeActionEmphasis::Hovered => theme.surface.card,
        WelcomeActionEmphasis::Focused => theme.surface.selected_card,
    };
    let label_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let shortcut_foreground = match row.emphasis {
        WelcomeActionEmphasis::Focused => theme.text.primary,
        WelcomeActionEmphasis::Default | WelcomeActionEmphasis::Hovered => theme.text.secondary,
    };
    let shortcut_style = Style::default().fg(shortcut_foreground).bg(surface);
    let marker = if row.emphasis == WelcomeActionEmphasis::Focused {
        "›"
    } else {
        " "
    };
    let shortcut = truncate_plain_text(row.shortcut, row.width.saturating_sub(1));
    let shortcut_width = super::display_width(&shortcut);
    let label_budget = row.width.saturating_sub(shortcut_width).saturating_sub(3);
    let label = truncate_plain_text(row.label, label_budget);
    let used = 1usize.saturating_add(super::display_width(&label));
    let shortcut_column = row.width.saturating_sub(shortcut_width.saturating_add(1));
    let gap = shortcut_column.saturating_sub(used);
    let trailing = row
        .width
        .saturating_sub(used.saturating_add(gap).saturating_add(shortcut_width));

    vec![
        Span::styled(
            marker,
            if row.emphasis == WelcomeActionEmphasis::Focused {
                label_style
            } else {
                shortcut_style
            },
        ),
        Span::styled(label, label_style),
        Span::styled(" ".repeat(gap), shortcut_style),
        Span::styled(shortcut, shortcut_style),
        Span::styled(" ".repeat(trailing), shortcut_style),
    ]
}

fn welcome_content_lines(app: &AppState, area: Rect, theme: &Theme) -> (Rect, Vec<Line<'static>>) {
    let layout = app.welcome_layout(area);
    let content = rect_from_tuple(layout.content_rect);
    let mut lines = vec![Line::default(); usize::from(content.height)];
    let mut put = |rect: (u16, u16, u16, u16), line: Line<'static>| {
        if rect.2 == 0 || rect.3 == 0 {
            return;
        }
        let Some(row) = lines.get_mut(usize::from(rect.1.saturating_sub(content.y))) else {
            return;
        };
        let offset = usize::from(rect.0.saturating_sub(content.x));
        row.spans
            .push(Span::raw(" ".repeat(offset.saturating_sub(row.width()))));
        row.spans.extend(line.spans);
    };
    let logo = if layout.panel_rect.is_some() {
        crate::startup_logo::full_logo(true)
    } else {
        crate::startup_logo::for_height(area.height.saturating_add(5), true)
    };
    if let Some(logo) = logo.filter(|_| layout.logo_rect.3 > 0) {
        for row in 0..usize::from(layout.logo_rect.3).min(logo.height()) {
            put(
                (
                    layout.logo_rect.0,
                    layout.logo_rect.1 + u16::try_from(row).unwrap_or(u16::MAX),
                    layout.logo_rect.2,
                    1,
                ),
                Line::from(crate::startup_logo::shimmer_row(
                    logo,
                    row,
                    app.startup_motion_elapsed(),
                    app.transcript_motion_enabled(),
                    theme,
                )),
            );
        }
    }
    let muted = Style::default().fg(theme.text.secondary);
    let title = Style::default()
        .fg(theme.text.primary)
        .add_modifier(Modifier::BOLD);
    put(
        layout.identity_rect,
        Line::from(vec![
            Span::styled("Harness ", title),
            Span::styled(env!("CARGO_PKG_VERSION"), muted),
        ]),
    );
    if let Some(header) = layout.changelog_header_rect {
        put(
            header,
            Line::from(Span::styled(
                "Changelog",
                welcome_changelog_section_style(theme, app),
            )),
        );
    }
    let bullet = if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
        "*"
    } else {
        "•"
    };
    let notes = welcome::changelog_bullets()
        .into_iter()
        .flat_map(|text| {
            super::wrap_completion_text(
                &format!("{bullet} {text}"),
                usize::from(layout.notes_rect.2),
            )
        })
        .collect::<Vec<_>>();
    for (index, text) in notes
        .into_iter()
        .take(usize::from(layout.notes_rect.3))
        .enumerate()
    {
        put(
            (
                layout.notes_rect.0,
                layout.notes_rect.1 + u16::try_from(index).unwrap_or(u16::MAX),
                layout.notes_rect.2,
                1,
            ),
            Line::from(Span::styled(text, muted)),
        );
    }
    for (index, (label, shortcut)) in [
        ("New worktree", "ctrl+w"),
        ("Resume session", "ctrl+s"),
        ("Changelog", ""),
        ("Quit", "ctrl+q"),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = layout.action_rects[index];
        put(
            rect,
            Line::from(welcome_action_spans(
                theme,
                WelcomeActionLine {
                    label,
                    shortcut,
                    width: usize::from(rect.2),
                    emphasis: welcome_action_emphasis(app, index),
                },
            )),
        );
    }
    if app.status_banner.is_some() {
        for (index, text) in
            super::wrap_completion_text(&app.welcome_notice(), usize::from(layout.notices_rect.2))
                .into_iter()
                .take(usize::from(layout.notices_rect.3))
                .enumerate()
        {
            put(
                (
                    layout.notices_rect.0,
                    layout.notices_rect.1 + u16::try_from(index).unwrap_or(u16::MAX),
                    layout.notices_rect.2,
                    1,
                ),
                Line::from(Span::styled(text, muted)),
            );
        }
    }
    (content, lines)
}

fn render_welcome_panel(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let layout = app.welcome_layout(area);
    if let Some(panel) = layout.panel_rect.map(rect_from_tuple) {
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
                    BorderType::Plain
                } else {
                    BorderType::Rounded
                })
                .border_style(Style::default().fg(theme.terminal_colors.welcome_border))
                .style(Style::default().bg(theme.surface.canvas)),
            panel,
        );
    }
    let (content, lines) = welcome_content_lines(app, area, theme);
    if content.width > 0 && content.height > 0 {
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.surface.canvas)),
            content,
        );
    }
}

/// Render the folder-trust prompt as a centered overlay dialog.
///
/// Display-only: the actual trust decision is persisted through the
/// coordinator when the operator confirms. This renders on top of the
/// startup content (welcome panel + breadcrumb) with correct z-order.
fn render_trust_folder_prompt_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
    let root = frame.area();

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
    let border = Style::default().fg(theme.terminal_colors.muted).bg(surface);

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
    if area.width == 0 || area.height == 0 || !app.welcome_visible() {
        return None;
    }
    let (viewport, lines) = welcome_content_lines(app, area, theme);
    Some(LifecycleSelectionSurface {
        viewport,
        text_rows: lines
            .into_iter()
            .enumerate()
            .map(|(row, line)| LifecycleSelectableText {
                row,
                max_height: 1,
                line,
                alignment: Alignment::Left,
            })
            .collect(),
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

    let surface = crate::layout::live_empty_state_area(area, theme);
    let copy = theme.live_shell.empty_state;
    let show_examples = surface.width >= 32 && surface.height >= 8;
    let mut lines = vec![
        Line::from(Span::styled(
            copy.title,
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            copy.value_prop,
            Style::default().fg(theme.text.secondary),
        )),
    ];
    if show_examples {
        let example_width = copy
            .example_prompts
            .iter()
            .map(|example| UnicodeWidthStr::width(example.prompt))
            .max()
            .unwrap_or(0);
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            copy.example_label,
            Style::default().fg(theme.text.tertiary),
        )));
        lines.extend(copy.example_prompts.into_iter().map(|example| {
            let trailing_padding =
                " ".repeat(example_width.saturating_sub(UnicodeWidthStr::width(example.prompt)));
            Line::from(vec![
                Span::styled(
                    format!("{}  ", theme.live_shell.transcript_glyphs.user_marker),
                    Style::default().fg(theme.text.accent),
                ),
                Span::styled(example.prompt, Style::default().fg(theme.text.secondary)),
                Span::raw(trailing_padding),
            ])
        }));
    }
    let content = crate::layout::centered_overlay_area(
        surface,
        surface.width,
        u16::try_from(lines.len()).unwrap_or(u16::MAX),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().bg(theme.surface.canvas)),
        content,
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
    use super::{format_breadcrumb_token_count, live_breadcrumb_text, pack_breadcrumb_line};

    #[test]
    fn format_breadcrumb_token_count_matches_freeze_style() {
        assert_eq!(format_breadcrumb_token_count(42), "42");
        assert_eq!(format_breadcrumb_token_count(1_500), "1.5K");
        assert_eq!(format_breadcrumb_token_count(10_000), "10K");
        assert_eq!(format_breadcrumb_token_count(12_000), "12K");
        assert_eq!(format_breadcrumb_token_count(262_144), "262K");
    }

    #[test]
    fn pack_breadcrumb_line_right_aligns_token_meta() {
        let packed = pack_breadcrumb_line("  git:main ~/proj", Some("12K / 262K"), 40);
        assert!(packed.ends_with("12K / 262K"), "packed={packed:?}");
        assert_eq!(super::super::display_width(&packed), 40);
        assert!(
            packed.contains("main") || packed.contains("proj"),
            "packed={packed:?}"
        );
    }

    #[test]
    fn pack_breadcrumb_line_without_meta_truncates_left_only() {
        let packed = pack_breadcrumb_line("  git:main ~/very/long/path/here", None, 20);
        assert_eq!(super::super::display_width(&packed), 20);
        assert!(
            packed.starts_with("  "),
            "left breadcrumb retained when meta absent: {packed:?}"
        );
        assert!(!packed.contains("12K / 262K"), "packed={packed:?}");
    }

    #[test]
    fn live_breadcrumb_uses_composer_inset_at_compact_widths() {
        // arrange
        let app = crate::app::AppState::new_live(None, false, None);

        // act
        let compact = live_breadcrumb_text(&app, 60);
        let wider = live_breadcrumb_text(&app, 79);

        // assert
        assert!(compact.starts_with("  "));
        assert!(wider.starts_with("  "));
    }

    #[test]
    fn startup_breadcrumb_does_not_dim_trailing_blank_cells() {
        // arrange
        use ratatui::{
            backend::TestBackend,
            style::{Color, Modifier, Style},
            widgets::Block,
            Terminal,
        };
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        // Given: the real startup renderer at the standard canary viewport.
        let mut app = crate::app::AppState::new_startup(Vec::new(), None);
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = Arc::clone(&calls);
        app.set_current_directory_probe_for_test(Arc::new(move || {
            let call = probe_calls.fetch_add(1, Ordering::Relaxed);
            harness_core::workspace::WorkspaceEnvironment {
                working_directory: "/workspace/checkout/subdir".into(),
                workspace_root: "/workspace/checkout".into(),
                is_git_repository: true,
                git_branch: Some(format!("branch-{call}")),
            }
        }));
        let _ = app.startup_directory_branch_label();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = app.theme();

        // When: the live startup shell is rendered.
        for _ in 0..3 {
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        Block::default().style(Style::default().bg(theme.surface.canvas)),
                        frame.area(),
                    );
                    super::render_startup_breadcrumb(frame, &app, frame.area(), &theme);
                    super::render_live_breadcrumb(
                        frame,
                        &app,
                        ratatui::layout::Rect::new(0, 3, 100, 2),
                        &theme,
                    );
                })
                .expect("startup render");
        }

        // act
        // Then: a cell after the visible breadcrumb retains the canvas style.
        let text = super::startup_breadcrumb_text(&app);
        let text_width = u16::try_from(super::super::display_width(&text))
            .expect("breadcrumb fits standard viewport");
        let cell = &terminal.backend().buffer()[(text_width + 1, 1)];
        // assert
        assert_eq!(cell.bg, theme.surface.canvas);
        assert!(!cell.modifier.contains(Modifier::DIM));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "repaints must not discover Git metadata"
        );
        assert_eq!(
            crate::render_test::buffer_to_string(terminal.backend().buffer(), 100)
                .matches("branch-0")
                .count(),
            2
        );
        assert!(app.refresh_current_directory_label());
        let refreshed = crate::render_test::render_to_string(
            &app,
            ratatui::layout::Rect::new(0, 0, 100, 30),
            |app, frame, area| {
                super::render_startup_breadcrumb(frame, app, area, app.theme());
                super::render_live_breadcrumb(
                    frame,
                    app,
                    ratatui::layout::Rect::new(0, 3, 100, 2),
                    app.theme(),
                );
            },
        );
        assert_eq!(refreshed.matches("branch-1").count(), 2);
        assert!(!refreshed.contains("branch-0"));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}

#[cfg(test)]
mod welcome_action_style_tests {
    use super::{welcome_action_spans, WelcomeActionEmphasis, WelcomeActionLine};
    use crate::theme::Theme;
    use crate::welcome_surface::WelcomeAction;

    #[test]
    fn hovered_action_uses_the_hover_surface_across_the_complete_row() {
        // arrange
        let theme = Theme::harness_dark();

        // act
        let spans = welcome_action_spans(
            &theme,
            WelcomeActionLine {
                label: WelcomeAction::NewWorktree.label(),
                shortcut: WelcomeAction::NewWorktree.shortcut(),
                width: 40,
                emphasis: WelcomeActionEmphasis::Hovered,
            },
        );

        // assert
        assert_eq!(
            (
                spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .starts_with(" New worktree"),
                spans.iter().map(|span| span.width()).sum::<usize>(),
                spans
                    .iter()
                    .all(|span| span.style.bg == Some(theme.surface.card)),
            ),
            (true, 40, true)
        );
    }

    #[test]
    fn focused_action_uses_the_selected_surface_and_visible_marker() {
        // arrange
        let theme = Theme::harness_dark();

        // act
        let spans = welcome_action_spans(
            &theme,
            WelcomeActionLine {
                label: WelcomeAction::ResumeSession.label(),
                shortcut: WelcomeAction::ResumeSession.shortcut(),
                width: 40,
                emphasis: WelcomeActionEmphasis::Focused,
            },
        );

        // assert
        assert_eq!(
            (
                spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .starts_with("›Resume session"),
                spans.iter().map(|span| span.width()).sum::<usize>(),
                spans
                    .iter()
                    .all(|span| span.style.bg == Some(theme.surface.selected_card)),
                spans
                    .iter()
                    .all(|span| span.style.fg == Some(theme.text.primary)),
            ),
            (true, 40, true, true)
        );
    }
}
