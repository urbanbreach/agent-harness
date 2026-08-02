// allow: SIZE_OK — TUI secondary view rendering (indivisible view model)
use super::*;

use crate::app::{OperatorSidebarSection, OrchestrationTaskState};
use crate::text::has_trimmed_content;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailModel {
    title: Option<OperatorRailTitle>,
    body: OperatorRailBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailTitle {
    Generated(String),
    Pending,
}

#[derive(Debug, Clone, Copy)]
struct StyledVisualChar {
    ch: char,
    style: Style,
    width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailItem {
    Plain(String),
    Todo {
        content: String,
        status: TodoRailStatus,
    },
    Status {
        label: String,
        suffix: Option<String>,
        state: RuntimeHealthState,
    },
    ModifiedFile {
        path: String,
        additions: Option<usize>,
        removals: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubagentRailGroup {
    agent_name: String,
    expanded: bool,
    items: Vec<SubagentRailItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubagentRailItem {
    description: String,
    status: SubagentRailStatus,
    child_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeHealthState {
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TodoRailStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubagentRailStatus {
    Queued,
    Running,
    Completed,
    Error,
}

const SUBAGENT_SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const SUBAGENT_SPINNER_TICK_DIVISOR: usize = 4;

impl OperatorRailItem {
    fn text(&self) -> &str {
        match self {
            Self::Plain(text) => text,
            Self::Todo { content, .. } => content,
            Self::Status { label, .. } => label,
            Self::ModifiedFile { path, .. } => path,
        }
    }

    #[cfg(test)]
    fn display_text(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
            Self::Todo { content, status } => {
                format!("{} {content}", status.checkbox_glyph())
            }
            Self::Status { label, suffix, .. } => suffix
                .as_ref()
                .map(|suffix| format!("{label} {suffix}"))
                .unwrap_or_else(|| label.clone()),
            Self::ModifiedFile {
                path,
                additions,
                removals,
            } => match (additions, removals) {
                (Some(additions), Some(removals)) => format!("{path} · +{additions} -{removals}"),
                _ => path.clone(),
            },
        }
    }
}

impl TodoRailStatus {
    fn from_value(value: &str) -> Self {
        match value {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }

    fn checkbox_glyph(self) -> &'static str {
        match self {
            Self::Completed => "[✓]",
            Self::InProgress => "[•]",
            Self::Pending | Self::Cancelled => "[ ]",
        }
    }

    fn style(self, theme: &Theme) -> Style {
        match self {
            Self::InProgress => Style::default().fg(theme.status.warning),
            Self::Pending | Self::Completed | Self::Cancelled => {
                Style::default().fg(theme.text.secondary)
            }
        }
    }
}

impl SubagentRailStatus {
    fn from_orchestration_state(state: OrchestrationTaskState) -> Self {
        match state {
            OrchestrationTaskState::Queued => Self::Queued,
            OrchestrationTaskState::Running | OrchestrationTaskState::Stale => Self::Running,
            OrchestrationTaskState::Completed | OrchestrationTaskState::LateResult => {
                Self::Completed
            }
            OrchestrationTaskState::Cancelled
            | OrchestrationTaskState::Failed
            | OrchestrationTaskState::TimedOut => Self::Error,
        }
    }

    fn from_tool_call_status(status: crate::app::ToolCallDisplayStatus) -> Self {
        match status {
            crate::app::ToolCallDisplayStatus::PendingPermission
            | crate::app::ToolCallDisplayStatus::Queued => Self::Queued,
            crate::app::ToolCallDisplayStatus::Running => Self::Running,
            crate::app::ToolCallDisplayStatus::Succeeded => Self::Completed,
            crate::app::ToolCallDisplayStatus::Failed => Self::Error,
        }
    }

    fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    fn glyph(self, animation_phase: usize) -> &'static str {
        match self {
            Self::Queued | Self::Running => {
                let frame = animation_phase / SUBAGENT_SPINNER_TICK_DIVISOR;
                SUBAGENT_SPINNER_FRAMES[frame % SUBAGENT_SPINNER_FRAMES.len()]
            }
            Self::Completed => "✓",
            Self::Error => "✗",
        }
    }
}

fn sanitize_operator_sidebar_line(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    let mut pending_space = false;

    for character in text.chars() {
        if character.is_control() || character.is_whitespace() {
            pending_space = !sanitized.is_empty();
            continue;
        }

        if pending_space {
            sanitized.push(' ');
            pending_space = false;
        }
        sanitized.push(character);
    }

    sanitized
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailBody {
    sections: Vec<OperatorRailBodySection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OperatorRailSectionDisclosure {
    section: OperatorSidebarSection,
    collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OperatorRailHeadingHitRegion {
    section: OperatorSidebarSection,
    top_row: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailSubagentHitRegion {
    session_id: String,
    top_row: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailSubagentGroupHitRegion {
    agent_name: String,
    top_row: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailBodyLayout {
    lines: Vec<Line<'static>>,
    heading_hit_regions: Vec<OperatorRailHeadingHitRegion>,
    subagent_hit_regions: Vec<OperatorRailSubagentHitRegion>,
    subagent_group_hit_regions: Vec<OperatorRailSubagentGroupHitRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OperatorSidebarSelectionCell {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperatorSidebarSelection {
    pub anchor: OperatorSidebarSelectionCell,
    pub focus: OperatorSidebarSelectionCell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorSidebarKeyboardTargetKind {
    Section(OperatorSidebarSection),
    SubagentGroup(String),
    SubagentSession(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperatorSidebarKeyboardTarget {
    pub kind: OperatorSidebarKeyboardTargetKind,
    pub top_row: usize,
    pub height: usize,
}

impl OperatorSidebarSelection {
    fn normalized(self) -> (OperatorSidebarSelectionCell, OperatorSidebarSelectionCell) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone)]
struct OperatorSidebarSelectionSnapshot {
    viewport: Rect,
    scroll_top: usize,
    rows: Vec<OperatorSidebarSelectionRow>,
}

#[derive(Debug, Clone)]
struct OperatorSidebarSelectionRow {
    cells: Vec<String>,
    continues_previous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailBodySection {
    Todo {
        items: Vec<OperatorRailItem>,
        disclosure: Option<OperatorRailSectionDisclosure>,
    },
    Subagents {
        groups: Vec<SubagentRailGroup>,
        disclosure: OperatorRailSectionDisclosure,
    },
    Mcp {
        items: Vec<OperatorRailItem>,
        disclosure: OperatorRailSectionDisclosure,
    },
    Lsp {
        items: Vec<OperatorRailItem>,
        disclosure: OperatorRailSectionDisclosure,
    },
    ModifiedFiles {
        items: Vec<OperatorRailItem>,
        disclosure: OperatorRailSectionDisclosure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorRailBodyPresentation {
    Regular,
}

impl OperatorRailBodySection {
    #[cfg(test)]
    fn heading(&self) -> String {
        match self {
            Self::Todo { disclosure, .. } => disclosure
                .map(|disclosure| format!("{} Todo", disclosure_glyph(disclosure)))
                .unwrap_or_else(|| "Todo".to_string()),
            Self::Subagents { disclosure, .. } => {
                format!("{} Subagents", disclosure_glyph(*disclosure))
            }
            Self::Mcp { disclosure, .. } => {
                format!("{} MCP", disclosure_glyph(*disclosure))
            }
            Self::Lsp { disclosure, .. } => {
                format!("{} LSP", disclosure_glyph(*disclosure))
            }
            Self::ModifiedFiles { disclosure, .. } => {
                format!("{} Modified Files", disclosure_glyph(*disclosure))
            }
        }
    }

    fn heading_line(&self, theme: &Theme) -> Line<'static> {
        let heading_style = self.heading_style(theme);
        match self {
            Self::Todo { disclosure, .. } => match disclosure {
                Some(disclosure) => Line::from(vec![
                    Span::styled(format!("{} ", disclosure_glyph(*disclosure)), heading_style),
                    Span::styled("Todo".to_string(), heading_style),
                ]),
                None => Line::from(Span::styled("Todo".to_string(), heading_style)),
            },
            Self::Subagents { groups, disclosure } => {
                let mut spans = vec![
                    Span::styled(format!("{} ", disclosure_glyph(*disclosure)), heading_style),
                    Span::styled("Subagents".to_string(), heading_style),
                ];
                if disclosure.collapsed {
                    spans.push(Span::styled(
                        format!(" ({} types)", groups.len()),
                        Style::default().fg(theme.text.secondary),
                    ));
                }
                Line::from(spans)
            }
            Self::Mcp { items, disclosure } => {
                let mut spans = vec![
                    Span::styled(format!("{} ", disclosure_glyph(*disclosure)), heading_style),
                    Span::styled("MCP".to_string(), heading_style),
                ];
                if disclosure.collapsed {
                    if let Some(summary) = collapsed_mcp_summary(items) {
                        spans.push(Span::styled(
                            format!(" {summary}"),
                            Style::default().fg(theme.text.secondary),
                        ));
                    }
                }
                Line::from(spans)
            }
            Self::Lsp { disclosure, .. } => Line::from(vec![
                Span::styled(format!("{} ", disclosure_glyph(*disclosure)), heading_style),
                Span::styled("LSP".to_string(), heading_style),
            ]),
            Self::ModifiedFiles { disclosure, .. } => Line::from(vec![
                Span::styled(format!("{} ", disclosure_glyph(*disclosure)), heading_style),
                Span::styled("Modified Files".to_string(), heading_style),
            ]),
        }
    }

    fn heading_style(&self, theme: &Theme) -> Style {
        match self {
            Self::Todo { .. }
            | Self::Subagents { .. }
            | Self::Mcp { .. }
            | Self::Lsp { .. }
            | Self::ModifiedFiles { .. } => Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        }
    }

    fn disclosure(&self) -> Option<OperatorRailSectionDisclosure> {
        match self {
            Self::Todo { disclosure, .. } => *disclosure,
            Self::Subagents { disclosure, .. }
            | Self::Mcp { disclosure, .. }
            | Self::Lsp { disclosure, .. }
            | Self::ModifiedFiles { disclosure, .. } => Some(*disclosure),
        }
    }

    fn collapsed(&self) -> bool {
        self.disclosure()
            .is_some_and(|disclosure| disclosure.collapsed)
    }

    #[cfg(test)]
    fn item_texts(&self) -> Vec<String> {
        match self {
            Self::Todo { items, .. }
            | Self::Mcp { items, .. }
            | Self::Lsp { items, .. }
            | Self::ModifiedFiles { items, .. } => {
                items.iter().map(OperatorRailItem::display_text).collect()
            }
            Self::Subagents { groups, .. } => groups
                .iter()
                .flat_map(|group| {
                    if group.items.len() == 1 {
                        return group
                            .items
                            .iter()
                            .map(|item| {
                                format!(
                                    "• {} {} {}",
                                    group.agent_name,
                                    item.status.glyph(0),
                                    item.description
                                )
                            })
                            .collect::<Vec<_>>();
                    }

                    let mut rows = vec![format!(
                        "{} {} {}",
                        if group.expanded { "▼" } else { "▶" },
                        group.agent_name,
                        subagent_group_summary(group)
                    )];
                    if group.expanded {
                        rows.extend(group.items.iter().map(|item| {
                            format!("  {} {}", item.status.glyph(0), item.description)
                        }));
                    }
                    rows
                })
                .collect(),
        }
    }
}

fn disclosure_glyph(disclosure: OperatorRailSectionDisclosure) -> &'static str {
    if disclosure.collapsed {
        "▶"
    } else {
        "▼"
    }
}

fn collapsed_mcp_summary(items: &[OperatorRailItem]) -> Option<String> {
    let active = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                OperatorRailItem::Status {
                    state: RuntimeHealthState::Healthy,
                    ..
                }
            )
        })
        .count();
    let errors = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                OperatorRailItem::Status {
                    state: RuntimeHealthState::Unhealthy,
                    ..
                }
            )
        })
        .count();

    if active == 0 && errors == 0 {
        return None;
    }

    Some(match errors {
        0 => format!("({active} active)"),
        1 => format!("({active} active, 1 error)"),
        _ => format!("({active} active, {errors} errors)"),
    })
}

pub(super) fn render_operator_sidebar(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    render_operator_sidebar_surface(frame, app, area, theme);
}

pub(super) fn render_live_details_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    frame.render_widget(Clear, overlay);
    render_operator_sidebar_surface(frame, app, overlay, theme);
}

#[cfg(test)]
#[path = "ui_secondary/orchestration_card.rs"]
mod orchestration_card;
#[cfg(test)]
pub(crate) use orchestration_card::orchestration_card_text_for_test;

#[path = "ui_secondary/sidebar_data.rs"]
mod sidebar_data;
use sidebar_data::{activity_surface_visible, build_operator_rail_model};
use sidebar_data::{operator_sidebar_lsp_items, operator_sidebar_mcp_items};

#[path = "ui_secondary/sidebar_sections.rs"]
mod sidebar_sections;
use sidebar_sections::build_operator_rail_body_layout;
#[cfg(test)]
use sidebar_sections::subagent_group_summary;

#[path = "ui_secondary/sidebar_interaction.rs"]
mod sidebar_interaction;
#[cfg(test)]
use sidebar_interaction::{
    operator_sidebar_body_area, operator_sidebar_footer_height, operator_sidebar_inner_area,
    operator_sidebar_subagent_group_hit_target_in_surface,
    operator_sidebar_subagent_session_hit_target_in_surface,
};
pub(crate) use sidebar_interaction::{
    operator_sidebar_keyboard_targets, operator_sidebar_section_hit_target,
    operator_sidebar_selection_cell, operator_sidebar_selection_text,
    operator_sidebar_subagent_group_hit_target, operator_sidebar_subagent_session_hit_target,
};
use sidebar_interaction::{
    operator_sidebar_selection_snapshot_in_surface, render_operator_sidebar_keyboard_selection,
    render_operator_sidebar_selection,
};

#[cfg(test)]
pub(crate) fn operator_sidebar_text_for_test(app: &AppState) -> Vec<String> {
    build_operator_sidebar_content(app, app.theme())
        .lines
        .into_iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
fn operator_sidebar_lines_for_test(app: &AppState) -> Vec<Line<'static>> {
    build_operator_sidebar_content(app, app.theme()).lines
}

#[cfg(test)]
#[path = "ui_secondary/operator_rail_test_fixtures.rs"]
mod operator_rail_test_fixtures;

#[cfg(test)]
#[path = "ui_secondary/operator_rail_model_tests.rs"]
mod operator_rail_model_tests;
#[cfg(test)]
pub(crate) use operator_rail_model_tests::{
    exact_test_compaction_applied_updates_active_context_usage_estimate,
    exact_test_operator_rail_collapses_modified_files_section_body,
    exact_test_operator_rail_low_activity_presentation_prefers_primary_stack,
    exact_test_operator_rail_matches_sidebar_text_styles,
    exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings,
    exact_test_operator_rail_section_model_builds_pinned_summary,
    exact_test_operator_rail_section_model_counts_generic_mcp_activity,
    exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order,
    exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp,
    exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity,
    exact_test_operator_rail_section_model_surfaces_pending_permissions_first,
    exact_test_operator_rail_section_model_uses_runtime_mcp_activity_without_config,
    exact_test_operator_rail_uses_generated_session_title,
    exact_test_operator_sidebar_hit_target_maps_section_headers,
};

#[cfg(test)]
#[path = "ui_secondary/operator_rail_todo_tests.rs"]
mod operator_rail_todo_tests;
#[cfg(test)]
pub(crate) use operator_rail_todo_tests::{
    exact_test_operator_rail_collapses_todo_section_body,
    exact_test_operator_rail_hides_completed_todo_state,
    exact_test_operator_rail_places_todo_below_subagents,
    exact_test_operator_rail_renders_todo_items_from_artifact_state,
    exact_test_operator_rail_renders_todo_items_from_tool_state,
};

#[cfg(test)]
#[path = "ui_secondary/subagent_tests.rs"]
mod subagent_tests;
#[cfg(test)]
pub(crate) use subagent_tests::{
    exact_test_operator_rail_keeps_subagents_visible_in_replay,
    exact_test_operator_rail_marks_background_subagent_terminal_from_notification,
    exact_test_operator_rail_renders_subagent_rows_from_orchestration_state,
    exact_test_operator_rail_shows_replay_wakeup_report_without_task_tool_row,
    exact_test_operator_rail_shows_wakeup_report_without_task_tool_row,
    exact_test_operator_rail_uses_simple_subagent_task_labels,
};

fn render_operator_sidebar_surface(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);
    let surface = ui_chrome::divided_shell_surface(theme);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    let block = ui_chrome::secondary_pane_block(theme, Line::default(), is_focused, surface);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let rail = build_operator_rail_model(app);
    let title_text = build_operator_rail_title_text(rail.title.as_ref(), theme, inner.width);
    let body_layout = build_operator_rail_body_layout(
        &rail.body,
        theme,
        inner.width,
        app.transcript_animation_phase(),
    );
    let directory_footer = app
        .sidebar_directory_branch_label()
        .map(|label| operator_sidebar_directory_footer_text(label, theme, inner.width, surface));
    let brand_footer = operator_sidebar_brand_footer_text(theme, surface);
    let title_height =
        u16::try_from(title_text.lines.len().min(usize::from(u16::MAX))).unwrap_or(u16::MAX);
    let footer_height = directory_footer
        .as_ref()
        .map(|footer| u16::try_from(footer.height().min(usize::from(u16::MAX))).unwrap_or(u16::MAX))
        .unwrap_or(0);
    let brand_height =
        u16::try_from(brand_footer.height().min(usize::from(u16::MAX))).unwrap_or(u16::MAX);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
            Constraint::Length(brand_height),
        ])
        .split(inner);
    let title_area = sections[0];
    let body_area = sections[1];
    let footer_area = sections[2];
    let brand_area = sections[3];

    if title_height > 0 {
        frame.render_widget(
            Paragraph::new(title_text).style(panel_style(surface, theme.text.primary)),
            title_area,
        );
    }
    frame.render_widget(
        Paragraph::new(Text::from(body_layout.lines.clone()))
            .style(panel_style(surface, theme.text.primary))
            .scroll((app.details_scroll, 0))
            .wrap(Wrap { trim: true }),
        body_area,
    );
    render_operator_sidebar_keyboard_selection(frame, app, &body_layout, body_area, theme);
    render_operator_sidebar_selection(
        frame,
        app.operator_sidebar_selection(),
        operator_sidebar_selection_snapshot_in_surface(app, area, theme),
        body_area,
        theme,
    );
    if let Some(footer) = directory_footer {
        frame.render_widget(
            Paragraph::new(footer)
                .style(Style::default().bg(surface))
                .wrap(Wrap { trim: false }),
            footer_area,
        );
    }
    if brand_area.height > 0 {
        frame.render_widget(
            Paragraph::new(brand_footer).style(Style::default().bg(surface)),
            brand_area,
        );
    }
}

#[cfg(test)]
fn build_operator_sidebar_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let rail = build_operator_rail_model(app);
    let mut lines = build_operator_rail_title_text(rail.title.as_ref(), theme, 80).lines;
    lines.extend(build_operator_rail_body_layout(&rail.body, theme, 80, 0).lines);
    if let Some(label) = app.sidebar_directory_branch_label() {
        lines.extend(
            operator_sidebar_directory_footer_text(label, theme, 80, theme.surface.panel).lines,
        );
    }
    lines.extend(operator_sidebar_brand_footer_text(theme, theme.surface.panel).lines);

    Text::from(lines)
}

fn operator_sidebar_directory_footer_text(
    label: &str,
    theme: &Theme,
    width: u16,
    surface: ratatui::style::Color,
) -> Text<'static> {
    let (parent, name) = harness_sidebar_path_parts(label);
    let line = Line::from(vec![
        Span::styled(
            format!("{parent}/"),
            Style::default().fg(theme.text.tertiary).bg(surface),
        ),
        Span::styled(name, Style::default().fg(theme.text.primary).bg(surface)),
    ]);
    let wrapped_lines = wrap_line_preserving_spans(line, usize::from(width.max(1)));
    Text::from(wrapped_lines)
}

fn operator_sidebar_brand_footer_text(
    theme: &Theme,
    surface: ratatui::style::Color,
) -> Text<'static> {
    let version = env!("CARGO_PKG_VERSION");
    let short_version: String = version.chars().take(12).collect();
    let line = Line::from(vec![
        Span::styled(
            "HARNESS",
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().bg(surface)),
        Span::styled(
            short_version,
            Style::default().fg(theme.text.tertiary).bg(surface),
        ),
    ]);
    Text::from(line)
}

fn harness_sidebar_path_parts(text: &str) -> (String, String) {
    let mut parts = text.split('/').collect::<Vec<_>>();
    let name = parts.pop().unwrap_or_default().to_string();
    (parts.join("/"), name)
}

fn wrap_line_preserving_spans(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }

    let chars = line
        .spans
        .into_iter()
        .flat_map(|span| {
            let style = span.style;
            span.content
                .chars()
                .map(move |ch| StyledVisualChar {
                    ch,
                    style,
                    width: Line::from(ch.to_string()).width().max(1),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    wrap_styled_chars_by_word(&chars, width)
}

fn wrap_styled_chars_by_word(chars: &[StyledVisualChar], width: usize) -> Vec<Line<'static>> {
    if chars.is_empty() {
        return vec![Line::default()];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let fit_end = styled_fit_end(chars, start, width);
        if fit_end >= chars.len() {
            lines.push(styled_chars_to_line(&chars[start..]));
            break;
        }

        if let Some(break_at) = chars[start..fit_end]
            .iter()
            .rposition(|visual_char| visual_char.ch.is_whitespace())
            .map(|offset| start + offset)
            .filter(|break_at| *break_at > start)
        {
            let end = break_at + 1;
            lines.push(styled_chars_to_line(&chars[start..end]));
            start = end;
        } else if chars[fit_end].ch.is_whitespace() {
            lines.push(styled_chars_to_line(&chars[start..fit_end]));
            start = fit_end + 1;
        } else {
            let end = chars[fit_end..]
                .iter()
                .position(|visual_char| visual_char.ch.is_whitespace())
                .map(|offset| fit_end + offset)
                .unwrap_or(chars.len());
            lines.push(styled_chars_to_line(&chars[start..end]));
            start = end;
        }
    }
    lines
}

fn styled_fit_end(chars: &[StyledVisualChar], start: usize, width: usize) -> usize {
    let mut used = 0usize;
    for (position, visual_char) in chars.iter().enumerate().skip(start) {
        if position > start && used.saturating_add(visual_char.width) > width {
            return position;
        }
        used = used.saturating_add(visual_char.width);
    }
    chars.len()
}

fn styled_chars_to_line(chars: &[StyledVisualChar]) -> Line<'static> {
    if chars.is_empty() {
        return Line::default();
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style = chars[0].style;
    for visual_char in chars {
        if visual_char.style == current_style {
            current.push(visual_char.ch);
        } else {
            spans.push(Span::styled(std::mem::take(&mut current), current_style));
            current_style = visual_char.style;
            current.push(visual_char.ch);
        }
    }
    spans.push(Span::styled(current, current_style));
    Line::from(spans)
}

fn build_operator_rail_title_text(
    title: Option<&OperatorRailTitle>,
    theme: &Theme,
    width: u16,
) -> Text<'static> {
    let Some(title) = title else {
        return Text::from(Vec::<Line<'static>>::new());
    };

    let (label, style) = match title {
        OperatorRailTitle::Generated(title) if has_trimmed_content(title) => (
            title.as_str(),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
        OperatorRailTitle::Generated(_) => return Text::from(Vec::<Line<'static>>::new()),
        OperatorRailTitle::Pending => (
            "Generating title...",
            Style::default().fg(theme.text.secondary),
        ),
    };

    Text::from(vec![
        Line::from(Span::styled(
            truncate_plain_text(label, usize::from(width)),
            style,
        )),
        Line::from(""),
    ])
}

pub(crate) fn format_detail_payload(payload: &str) -> String {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| trimmed.to_string()),
        Err(_) => trimmed.to_string(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FooterStatusClusterData {
    pub pending_permissions: usize,
    pub lsp_count: usize,
    pub lsp_has_error: bool,
    pub mcp_count: usize,
    pub mcp_has_error: bool,
}

pub(crate) fn footer_status_cluster_data(app: &AppState) -> FooterStatusClusterData {
    let pending_permissions = app.transcript_pending_permissions().len();
    let lsp_items = operator_sidebar_lsp_items(app);
    let mcp_items = operator_sidebar_mcp_items(app);

    let lsp_has_error = lsp_items.iter().any(|item| {
        matches!(
            item,
            OperatorRailItem::Status {
                state: RuntimeHealthState::Unhealthy,
                ..
            }
        )
    });
    let mcp_has_error = mcp_items.iter().any(|item| {
        matches!(
            item,
            OperatorRailItem::Status {
                state: RuntimeHealthState::Unhealthy,
                ..
            }
        )
    });

    FooterStatusClusterData {
        pending_permissions,
        lsp_count: lsp_items.len(),
        lsp_has_error,
        mcp_count: mcp_items.len(),
        mcp_has_error,
    }
}

#[cfg(test)]
fn line_to_plain_text(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_directory_footer_keeps_unbroken_path_segments_on_one_row() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let lines = operator_sidebar_directory_footer_text(
            "/tmp/harness-sidebar-overflow/workspaces/golden_path_interactive-run_1234567890abcdef:dev",
            &theme,
            28,
            theme.surface.panel,
        )
        .lines;
        let plain = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(
            lines.len(),
            1,
            "unbroken sidebar footer words should not split"
        );
        assert!(
            !plain.contains('…'),
            "Harness sidebar footer does not ellipsize"
        );
        assert!(plain.ends_with("golden_path_interactive-run_1234567890abcdef:dev"));
    }

    #[test]
    fn sidebar_brand_footer_renders_harness_branding_and_version() {
        // arrange
        let theme = Theme::default();
        // act
        let lines = operator_sidebar_brand_footer_text(&theme, theme.surface.panel).lines;
        // assert
        assert_eq!(lines.len(), 1, "brand footer should be a single line");
        let plain = line_to_plain_text(lines[0].clone());
        assert!(
            plain.starts_with("HARNESS "),
            "brand footer should start with bold HARNESS branding: {plain:?}"
        );
        let version = env!("CARGO_PKG_VERSION");
        let short_version: String = version.chars().take(12).collect();
        assert!(
            plain.contains(&short_version),
            "brand footer should include short version string: {plain:?}"
        );
    }
}
