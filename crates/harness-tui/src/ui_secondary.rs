use std::collections::BTreeMap;
use std::path::Path;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::*;

#[cfg(test)]
use crate::app::OrchestrationTaskRow;
#[cfg(test)]
use crate::app::{ActiveContextUsage, ActivityUsage};
use crate::app::{OperatorSidebarSection, OrchestrationTaskState};
use crate::text::{
    has_trimmed_content, trimmed_json_nested_string_field, trimmed_json_string_field,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorSidebarChrome {
    Persistent,
    Overlay,
}

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

#[derive(Debug, Clone)]
struct StyledTextChunk {
    text: String,
    style: Style,
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

const SUBAGENT_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[cfg(test)]
fn operator_sidebar_config_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_mcp_server_connection_states(BTreeMap::new());
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());
    guard
}

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
                SUBAGENT_SPINNER_FRAMES[animation_phase % SUBAGENT_SPINNER_FRAMES.len()]
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

    render_operator_sidebar_surface(frame, app, area, theme, OperatorSidebarChrome::Persistent);
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
    render_operator_sidebar_surface(frame, app, overlay, theme, OperatorSidebarChrome::Overlay);
}

pub(super) fn render_events_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let body = render_secondary_surface_shell(frame, area, theme, events_summary_line(app, theme));
    let [event_list_area, event_details_area] = split_secondary_surface(
        body,
        crate::layout::REVIEW_SURFACE_SPLIT_PERCENT,
        theme.live_shell.rhythm.surface_gap,
    );

    render_event_list(frame, app, event_list_area, theme);
    render_event_details(frame, app, event_details_area, theme);
}

pub(super) fn render_help_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let body = render_secondary_surface_shell(frame, area, theme, help_summary_line(app, theme));
    let surface = theme.surface.panel_elevated;
    let block = panel_block(
        theme,
        if app.replay_mode { "Reference" } else { "Help" },
        false,
        surface,
    );

    let paragraph = Paragraph::new(help_text(app))
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, body);
}

#[cfg(test)]
pub(crate) fn orchestration_card_text_for_test(
    app: &AppState,
    height: u16,
    width: u16,
) -> Vec<String> {
    orchestration_card_lines(
        app,
        &app.orchestration_visible_rows(),
        app.theme(),
        height,
        width,
    )
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
pub(crate) fn exact_test_operator_rail_section_model_builds_pinned_summary() {
    let _guard = operator_sidebar_config_test_guard();
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = operator_rail_test_app();
    let model = build_operator_rail_model(&app);

    assert_eq!(model.title, None);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(model.body.sections[1].heading(), "▼ LSP");
    assert_eq!(model.body.sections[2].heading(), "▼ Modified Files");
    assert_eq!(
        model.body.sections[2].item_texts(),
        ["src/ui_secondary.rs".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_compaction_applied_updates_active_context_usage_estimate() {
    let mut app = AppState::new_live(None, false, None);
    app.active_context_usage = Some(ActiveContextUsage::estimate(500));
    app.activities.push_back(ActivityEntry {
        request_id: "req_000001".to_string(),
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: Some(ActivityUsage {
            prompt_tokens: 400,
            completion_tokens: 100,
            total_tokens: 500,
        }),
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 10,
        last_mono_ms: 20,
    });
    assert_eq!(
        app.active_context_usage(),
        Some(ActiveContextUsage::estimate(500))
    );
    assert_eq!(app.compaction_usage_metrics().completed_count, 0);

    app.ingest_event(operator_rail_test_event(
        3,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::CompactionApplied(
            harness_core::event::CompactionAppliedEvent {
                checkpoint_id: "checkpoint_000003".to_string(),
                agent_id: "agent_000001".to_string(),
                through_seq: 2,
                through_request_id: Some("req_000001".to_string()),
                tokens_before_estimate: Some(500),
                tokens_after_estimate: Some(120),
                summary_tokens_estimate: Some(80),
                compacted_turns: Some(1),
                preserved_turns: Some(1),
                reduction_tokens_estimate: Some(380),
                reduction_percent_estimate: Some(76),
                estimate_source: None,
            },
        ),
    ));

    assert_eq!(
        app.active_context_usage(),
        Some(ActiveContextUsage::estimate(120))
    );
    let metrics = app.compaction_usage_metrics();
    assert_eq!(metrics.completed_count, 1);
    assert_eq!(metrics.summary_tokens_estimate, 80);
    assert_eq!(metrics.reduction_tokens_estimate, 380);
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_control_chars".to_string(),
            path: "src/ui_secondary.rs\nModified Files · 99".to_string(),
            new_file_digest: "digest-edit-control".to_string(),
            diff_rel_path: Some("artifacts/\tcontrol\nrows.diff".to_string()),
            diff_digest: Some("digest-edit-control-diff".to_string()),
        }),
    ));

    let model = build_operator_rail_model(&app);
    let modified_files = model
        .body
        .sections
        .iter()
        .find_map(|section| match section {
            OperatorRailBodySection::ModifiedFiles { items, .. } => Some(items),
            _ => None,
        })
        .expect("modified file sidebar section");

    assert_eq!(
        modified_files
            .iter()
            .map(OperatorRailItem::display_text)
            .collect::<Vec<_>>(),
        ["src/ui_secondary.rs Modified Files · 99".to_string()]
    );
    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Modified Files"));
    assert!(sidebar.contains("src/ui_secondary.rs Modified Files · 99"));
    assert!(!sidebar.contains("rows.diff\n▼ LSP"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order() {
    let _guard = operator_sidebar_config_test_guard();
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let empty_model = build_operator_rail_model(&AppState::new_live(None, false, None));
    assert_eq!(
        empty_model
            .body
            .sections
            .iter()
            .map(OperatorRailBodySection::heading)
            .collect::<Vec<_>>(),
        vec![
            "▼ MCP".to_string(),
            "▼ LSP".to_string(),
            "▶ Modified Files".to_string(),
        ]
    );
    assert_eq!(empty_model.body.sections[0].heading(), "▼ MCP");
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_matches_sidebar_text_styles() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    integrations.mcp.servers.insert(
        "fixture".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://example.com/mcp".to_string(),
            headers: std::collections::BTreeMap::new(),
            timeout_secs: 30,
            enabled: true,
        },
    );
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_mcp_server_connection_states(
        std::collections::BTreeMap::from([(
            "fixture".to_string(),
            harness_core::config::McpServerConnectionState::Connected,
        )]),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let session_path = std::env::temp_dir().join(format!(
        "harness-tui-sidebar-style-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(session_path.join("artifacts"))
        .expect("create sidebar style fixture dir");
    std::fs::write(
        session_path.join("artifacts/edit-1.diff"),
        "diff --git a/src/ui_secondary.rs b/src/ui_secondary.rs\n--- a/src/ui_secondary.rs\n+++ b/src/ui_secondary.rs\n@@ -1 +1 @@\n-old\n+new\n",
    )
    .expect("write sidebar style diff fixture");

    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );
    let mut app = AppState::new_live(Some(session_path.clone()), false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_style_1".to_string(),
            path: "src/ui_secondary.rs".to_string(),
            new_file_digest: "digest-style-edit-1".to_string(),
            diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
            diff_digest: Some("digest-style-diff-1".to_string()),
        }),
    ));
    let lsp_request_id = "req_style_lsp";
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        lsp_request_id,
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: lsp_request_id.to_string(),
                text: "Find the Rust definition".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        lsp_request_id,
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_style_lsp".to_string(),
                tool_id: "code.lsp".to_string(),
                args_summary: r#"{"operation":"goto_definition","path":"src/ui_secondary.rs"}"#
                    .to_string(),
                args_digest: "digest-style-lsp".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        lsp_request_id,
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_style_lsp".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Found definition in src/ui_secondary.rs".to_string()),
                output_digest: Some("digest-style-lsp-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));

    let theme = *app.theme();
    let lines = operator_sidebar_lines_for_test(&app);

    let mcp_connected = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("fixture Connected")
        })
        .expect("connected mcp line");
    assert_eq!(mcp_connected.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(mcp_connected.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(mcp_connected.spans[2].style.fg, Some(theme.text.secondary));

    let mcp_disconnected = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("websearch Disconnected")
        })
        .expect("disconnected mcp line");
    assert_eq!(mcp_disconnected.spans[0].style.fg, Some(theme.status.error));
    assert_eq!(mcp_disconnected.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(
        mcp_disconnected.spans[2].style.fg,
        Some(theme.text.secondary)
    );

    let lsp_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("rust")
        })
        .expect("lsp line");
    assert_eq!(lsp_line.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(lsp_line.spans[1].style.fg, Some(theme.text.secondary));

    let modified_file_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("src/ui_secondary.rs · +1 -1")
        })
        .expect("modified file line");
    assert_eq!(
        modified_file_line.spans[0].style.fg,
        Some(theme.text.secondary)
    );
    assert_eq!(
        modified_file_line.spans[1].style.fg,
        Some(theme.text.secondary)
    );
    assert_eq!(
        modified_file_line.spans[3].style.fg,
        Some(theme.status.success)
    );
    assert_eq!(
        modified_file_line.spans[5].style.fg,
        Some(theme.status.error)
    );

    std::fs::remove_dir_all(session_path).expect("remove sidebar style fixture dir");
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_surfaces_pending_permissions_first() {
    let app = operator_rail_test_app();
    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_renders_todo_items_from_tool_state() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo".to_string(),
                text: "Track the implementation".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo".to_string(),
                tool_id: "todowrite".to_string(),
                args_summary:
                    r#"{"todos":[{"content":"Plan work","status":"pending","priority":"high"}]}"#
                        .to_string(),
                args_digest: "digest-todo".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "Plan work", "status": "completed", "priority": "high"},
                        {"content": "Implement UI", "status": "in_progress", "priority": "high"},
                        {"content": "Verify tests", "status": "pending", "priority": "medium"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ Todo");
    assert_eq!(model.body.sections[1].heading(), "▼ MCP");

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Todo"));
    assert!(sidebar.contains("[✓] Plan work"));
    assert!(sidebar.contains("[•] Implement UI"));
    assert!(sidebar.contains("[ ] Verify tests"));

    let theme = *app.theme();
    let lines = operator_sidebar_lines_for_test(&app);
    let in_progress = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("[•] Implement UI")
        })
        .expect("in-progress todo line");
    assert_eq!(in_progress.spans[0].style.fg, Some(theme.status.warning));
    assert_eq!(in_progress.spans[1].style.fg, Some(theme.status.warning));

    let pending = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("[ ] Verify tests")
        })
        .expect("pending todo line");
    assert_eq!(pending.spans[0].style.fg, Some(theme.text.secondary));
    assert_eq!(pending.spans[1].style.fg, Some(theme.text.secondary));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_places_todo_below_subagents() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-sidebar-order",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-sidebar-order".to_string(),
                text: "Inspect sidebar section order".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-sidebar-order",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_order".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "inspect subagent ordering",
                    "subagent_type": "general"
                })
                .to_string(),
                args_digest: "digest-task-order".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-sidebar-order",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_order".to_string(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"Keep todo below subagents","status":"in_progress","priority":"high"}]}"#
                    .to_string(),
                args_digest: "digest-todo-order".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-sidebar-order",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_order".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-order-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "Keep todo below subagents", "status": "in_progress", "priority": "high"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ Subagents");
    assert_eq!(model.body.sections[1].heading(), "Todo");
    assert_eq!(model.body.sections[2].heading(), "▼ MCP");
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_uses_generated_session_title() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-title",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-title".to_string(),
                text: "Please implement the flaky retry button".to_string(),
            },
        ),
    ));

    let theme = *app.theme();
    let pending =
        build_operator_rail_title_text(build_operator_rail_model(&app).title.as_ref(), &theme, 80);
    assert_eq!(
        pending.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "Generating title..."
    );
    assert_eq!(
        pending.lines[0].spans[0].style.fg,
        Some(theme.text.secondary)
    );

    app.ingest_event(operator_rail_test_event(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::SessionTitleUpdated(
            harness_core::event::SessionTitleUpdatedEvent {
                title: "Retry button implementation".to_string(),
            },
        ),
    ));

    let generated =
        build_operator_rail_title_text(build_operator_rail_model(&app).title.as_ref(), &theme, 80);
    assert_eq!(
        generated.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "Retry button implementation"
    );
    assert_eq!(
        generated.lines[0].spans[0].style.fg,
        Some(theme.text.primary)
    );
    assert!(operator_sidebar_text_for_test(&app)
        .join("\n")
        .contains("Retry button implementation"));
    assert!(!operator_sidebar_text_for_test(&app)
        .join("\n")
        .contains("Please implement the flaky retry button"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_renders_subagent_rows_from_orchestration_state() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req_subagents",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_subagents".to_string(),
                text: "Delegate the investigation".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_running".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "inspect README and summarize task background behavior in three bullets",
                    "subagent_type": "explore"
                })
                .to_string(),
                args_digest: "digest-task-running".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_task_running".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_explore_done".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "cross-check docs",
                    "subagent_type": "explore"
                })
                .to_string(),
                args_digest: "digest-task-explore-done".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        5,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_explore_done".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Task Result: cross-check docs".to_string()),
                output_digest: Some("digest-task-explore-done-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        6,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_done".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "summary",
                    "subagent_type": "general"
                })
                .to_string(),
                args_digest: "digest-task-done".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        7,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_done".to_string(),
            result_summary: "summarized an intentionally long repository behavior report"
                .to_string(),
            result_digest: "digest-done".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        8,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_done".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Task Result: summary".to_string()),
                output_digest: Some("digest-task-done-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        9,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_failed".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "cancelled",
                    "subagent_type": "plan"
                })
                .to_string(),
                args_digest: "digest-task-failed".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        10,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_failed".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("operator cancelled".to_string()),
                output_digest: Some("digest-task-failed-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        11,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_wrapped_child".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "open wrapped child session after reviewing enough sidebar text to wrap cleanly",
                    "subagent_type": "navigator"
                })
                .to_string(),
                args_digest: "digest-task-wrapped-child".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        12,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_wrapped_child".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Task Result: wrapped child".to_string()),
                output_digest: Some("digest-task-wrapped-child-output".to_string()),
                output_json: Some(serde_json::json!({
                    "session_id": "child_running_session"
                })),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        13,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "req_subagents",
        harness_core::event::EventV1::RunStarted(harness_core::event::RunStartedEvent {
            run_name: "subagent sidebar".to_string(),
            workspace_root: "/tmp/subagent-sidebar-footer".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        14,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_orphan_running",
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_orphan_running".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:orphan".to_string()),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        15,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_orphan_queued",
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_orphan_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:queued".to_string()),
        }),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("▶ explore ⠋ 2 tasks · 1 active"));
    assert!(!sidebar.contains("  ⠋ inspect README"));
    assert!(!sidebar.contains("  ✓ cross-check docs"));
    assert!(!sidebar.contains("            bullets"));
    assert!(sidebar.contains("• general ✓ summary"));
    assert!(sidebar.contains("• plan ✗ cancelled"));
    assert!(sidebar.contains("• navigator ⠋ open wrapped child session"));
    assert!(sidebar.contains("• orphan ⠋ task_orphan_running"));
    assert!(sidebar.contains("• queued ⠋ task_orphan_queued"));
    assert!(!sidebar.contains("intentionally long repository"));

    let theme = *app.theme();
    let lines = operator_sidebar_lines_for_test(&app);
    let explore_group = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("▶ explore ⠋ 2 tasks · 1 active")
        })
        .expect("collapsed explore group line");
    assert_eq!(explore_group.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(explore_group.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(explore_group.spans[2].style.fg, Some(theme.status.success));
    assert_eq!(explore_group.spans[3].style.fg, Some(theme.text.primary));

    let cancelled = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("• plan ✗ cancelled")
        })
        .expect("cancelled subagent line");
    assert_eq!(cancelled.spans[0].style.fg, Some(theme.text.primary));
    assert_eq!(cancelled.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(cancelled.spans[3].style.fg, Some(theme.text.secondary));
    assert_eq!(cancelled.spans[4].style.fg, Some(theme.text.secondary));

    let sidebar_area = Rect::new(0, 0, 32, 40);
    let theme = &theme;
    let inner =
        operator_sidebar_inner_area(&app, sidebar_area, theme, OperatorSidebarChrome::Persistent)
            .expect("sidebar inner area");
    let rail = build_operator_rail_model(&app);
    let title_height = build_operator_rail_title_text(rail.title.as_ref(), theme, inner.width)
        .lines
        .len()
        .min(usize::from(u16::MAX)) as u16;
    let wrapped_layout = build_operator_rail_body_layout(&rail.body, theme, inner.width, 0);
    let explore_group_region = wrapped_layout
        .subagent_group_hit_regions
        .iter()
        .find(|region| region.agent_name == "explore")
        .expect("explore group hit region");
    assert_eq!(
        operator_sidebar_subagent_group_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            inner
                .y
                .saturating_add(title_height)
                .saturating_add(explore_group_region.top_row as u16),
        ),
        Some("explore".to_string())
    );
    assert!(
        wrapped_layout
            .subagent_hit_regions
            .iter()
            .all(|region| region.session_id != "child_explore_session"),
        "collapsed subagent group should hide child-session hit regions"
    );
    let wrapped_region = wrapped_layout
        .subagent_hit_regions
        .iter()
        .find(|region| region.session_id == "child_running_session")
        .expect("running child session hit region");
    assert!(
        wrapped_region.height == 1,
        "long subagent row should stay compact and keep a one-row hit region"
    );
    let body_y = inner.y.saturating_add(title_height);
    let wrapped_row = body_y.saturating_add(wrapped_region.top_row as u16);
    assert_eq!(
        operator_sidebar_subagent_session_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            wrapped_row,
        ),
        Some("child_running_session".to_string())
    );
    assert_ne!(
        operator_sidebar_subagent_session_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            wrapped_row.saturating_add(1),
        ),
        Some("child_running_session".to_string())
    );

    let footer_height = operator_sidebar_footer_height(&app, theme, inner.width);
    assert!(
        footer_height > 0,
        "test setup should render a sidebar footer"
    );
    let footer_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(inner);
    assert_eq!(
        operator_sidebar_subagent_session_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            footer_sections[2].y,
        ),
        None,
        "footer clicks must not activate hidden subagent rows"
    );

    let frame_area = Rect::new(0, 0, 140, 40);
    let plan = crate::layout::FrameLayoutPlan::for_app(&app, frame_area);
    let sidebar_area = plan.operator_sidebar.expect("operator sidebar area");
    let inner =
        operator_sidebar_inner_area(&app, sidebar_area, theme, OperatorSidebarChrome::Persistent)
            .expect("operator sidebar inner area");
    let rail = build_operator_rail_model(&app);
    let body_area = operator_sidebar_body_area(&app, inner, theme, rail.title.as_ref())
        .expect("operator sidebar body area");
    let layout = build_operator_rail_body_layout(&rail.body, theme, body_area.width, 0);
    let group_region = layout
        .subagent_group_hit_regions
        .iter()
        .find(|region| region.agent_name == "explore")
        .expect("explore group hit region in frame layout");
    let group_row = body_area.y.saturating_add(group_region.top_row as u16);
    let group_col = body_area.x;
    for kind in [
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
    ] {
        app.handle_mouse(
            crossterm::event::MouseEvent {
                kind,
                column: group_col,
                row: group_row,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            frame_area,
            None,
            None,
            None,
        );
    }

    let expanded_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(expanded_sidebar.contains("▼ explore ⠋ 2 tasks · 1 active"));
    assert!(expanded_sidebar.contains("  ⠋ inspect README"));
    assert!(expanded_sidebar.contains("  ✓ cross-check docs"));
    let expanded_lines = operator_sidebar_lines_for_test(&app);
    let running = expanded_lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("  ⠋ inspect README")
        })
        .expect("running subagent line after expanding group");
    assert_eq!(running.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(running.spans[1].style.fg, Some(theme.status.success));
    assert_eq!(running.spans[2].style.fg, Some(theme.text.primary));

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::Subagents),
        None,
    );

    let collapsed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(collapsed_sidebar.contains("▶ Subagents (6 types)"));
    assert!(!collapsed_sidebar.contains("inspect README"));
    assert!(!collapsed_sidebar.contains("✓ summary"));
    assert!(!collapsed_sidebar.contains("✗ cancelled"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_marks_background_subagent_terminal_from_notification() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req_background_parent",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_background_parent".to_string(),
                text: "Start a background subagent".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_background_parent",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_background_task".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "summarize README",
                    "subagent_type": "general"
                })
                .to_string(),
                args_digest: "digest-background-task-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_background_parent",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_background_task".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Background task scheduled".to_string()),
                output_digest: Some("digest-background-task-output".to_string()),
                output_json: Some(serde_json::json!({
                    "background": true,
                    "child_session_id": "agent_child",
                    "child_request_id": "req_child"
                })),
                metadata: None,
            },
        ),
    ));

    let active_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(active_sidebar.contains("• general ⠋ summarize README"));

    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "background_task_notification:req_child",
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_child".to_string(),
                child_request_id: "req_child".to_string(),
                task_id: "agent_child".to_string(),
                description: "summarize README".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                summary: "README summarized".to_string(),
                terminal_event_id: "evt_child_done".to_string(),
                terminal_task_id: "agent_child".to_string(),
                delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
            },
        ),
    ));

    let completed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(completed_sidebar.contains("• general ✓ summarize README"));
    assert!(!completed_sidebar.contains("• general ⠋ summarize README"));
    assert!(!completed_sidebar.contains("1 active"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_shows_wakeup_report_without_task_tool_row() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "agent_child",
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_child".to_string(),
            profile: "plan".to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "background_task_notification:req_child",
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_child".to_string(),
                child_request_id: "req_child".to_string(),
                task_id: "task_child".to_string(),
                description: "Inspect sidebar wakeup".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                summary: "Wakeup report finished".to_string(),
                terminal_event_id: "evt_child_done".to_string(),
                terminal_task_id: "task_child".to_string(),
                delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
            },
        ),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("• plan ✓ Wakeup report finished"));
    assert!(!sidebar.contains("• subagent ✓"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_keeps_subagents_visible_in_replay() {
    let events = vec![
        operator_rail_test_event_with_correlation(
            1,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
            "req_replay_subagent",
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_replay_subagent".to_string(),
                    text: "Review replay sidebar parity".to_string(),
                },
            ),
        ),
        operator_rail_test_event_with_correlation(
            2,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
            "req_replay_subagent",
            harness_core::event::EventV1::ToolCallRequested(
                harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "tool_call_replay_subagent".to_string(),
                    tool_id: "task".to_string(),
                    args_summary: serde_json::json!({
                        "description": "audit replay subagent sidebar",
                        "subagent_type": "researcher"
                    })
                    .to_string(),
                    args_digest: "digest-replay-subagent".to_string(),
                    metadata: Some(harness_core::event::ToolCallMetadata {
                        lineage: Some(harness_core::event::TaskLineageMetadata {
                            child_session_id: Some("child_replay_session".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ),
        ),
        operator_rail_test_event_with_correlation(
            3,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
            "req_replay_subagent",
            harness_core::event::EventV1::ToolCallFinished(
                harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "tool_call_replay_subagent".to_string(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("Replay sidebar audited".to_string()),
                    output_digest: Some("digest-replay-subagent-output".to_string()),
                    output_json: None,
                    metadata: Some(harness_core::event::ToolCallMetadata {
                        lineage: Some(harness_core::event::TaskLineageMetadata {
                            child_session_id: Some("child_replay_session".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ),
        ),
    ];
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-subagent"), events);

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("• researcher ✓ audit replay subagent sidebar"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_hides_completed_todo_state() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo-completed",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo-completed".to_string(),
                text: "Finish the checklist".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-completed",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_completed".to_string(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"Ship todo panel","status":"completed","priority":"high"}]}"#
                    .to_string(),
                args_digest: "digest-todo-completed".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-completed",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_completed".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list completed".to_string()),
                output_digest: Some("digest-todo-completed-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "Ship todo panel", "status": "completed", "priority": "high"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(!sidebar.contains("Todo"));
    assert!(!sidebar.contains("[✓] Ship todo panel"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_renders_todo_items_from_artifact_state() {
    let temp_dir = tempfile::tempdir().expect("todo artifact session dir");
    let artifact_path = temp_dir
        .path()
        .join("artifacts")
        .join("toolcalls")
        .join("tool_call_todo_artifact")
        .join("result.json");
    std::fs::create_dir_all(artifact_path.parent().expect("artifact parent"))
        .expect("create todo artifact dir");
    std::fs::write(
        &artifact_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "todos": [
                {"content": "Persisted todo", "status": "in_progress", "priority": "high"}
            ]
        }))
        .expect("serialize todo artifact"),
    )
    .expect("write todo artifact");

    let mut app = AppState::new_live(Some(temp_dir.path().to_path_buf()), false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo-artifact",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo-artifact".to_string(),
                text: "Render persisted todo state".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-artifact",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_artifact".to_string(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"Persisted todo","status":"in_progress","priority":"high"}]}"#
                    .to_string(),
                args_digest: "digest-todo-artifact".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-artifact",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_artifact".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-artifact-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    artifact_refs: vec![harness_core::event::EventArtifactRef {
                        path: "artifacts/toolcalls/tool_call_todo_artifact/result.json".to_string(),
                        digest: None,
                    }],
                    ..Default::default()
                }),
            },
        ),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("Todo"));
    assert!(sidebar.contains("[•] Persisted todo"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_collapses_todo_section_body() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo-collapse",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo-collapse".to_string(),
                text: "Track several tasks".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-collapse",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_collapse".to_string(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"One","status":"completed","priority":"high"},{"content":"Two","status":"in_progress","priority":"high"},{"content":"Three","status":"pending","priority":"medium"}]}"#
                    .to_string(),
                args_digest: "digest-todo-collapse".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-collapse",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_collapse".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-collapse-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "One", "status": "completed", "priority": "high"},
                        {"content": "Two", "status": "in_progress", "priority": "high"},
                        {"content": "Three", "status": "pending", "priority": "medium"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let open_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(open_sidebar.contains("▼ Todo"));
    assert!(open_sidebar.contains("[•] Two"));

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::Todo),
        None,
    );

    let collapsed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(collapsed_sidebar.contains("▶ Todo"));
    assert!(!collapsed_sidebar.contains("[✓] One"));
    assert!(!collapsed_sidebar.contains("[•] Two"));
    assert!(!collapsed_sidebar.contains("[ ] Three"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = operator_rail_activity_test_app();
    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        ["websearch Connected".to_string()]
    );
    assert_eq!(
        operator_sidebar_lsp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        ["rust".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_uses_runtime_mcp_activity_without_config() {
    let _guard = operator_sidebar_config_test_guard();
    let app = operator_rail_activity_test_app();
    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        ["websearch Connected".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let mut app = operator_rail_test_app();

    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_native_prefix_tool".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4".to_string(),
                prompt_summary: "Apply the sidebar patch".to_string(),
                request_digest: "digest-native-prefix-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        5,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_edit".to_string(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: serde_json::json!({"path": "src/ui_secondary.rs"}).to_string(),
                args_digest: "digest-native-prefix-edit".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        6,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_edit".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Applied inline hashline edit".to_string()),
                output_digest: Some("digest-native-prefix-edit-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);
    assert_eq!(
        model
            .body
            .sections
            .iter()
            .map(OperatorRailBodySection::heading)
            .collect::<Vec<_>>(),
        vec![
            "▼ MCP".to_string(),
            "▼ LSP".to_string(),
            "▼ Modified Files".to_string(),
        ]
    );
    let mcp_items = model.body.sections[0].item_texts();
    assert!(
        mcp_items == ["websearch Disconnected".to_string()]
            || mcp_items == ["No MCP servers configured".to_string()],
        "unexpected MCP summary items: {mcp_items:?}"
    );

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(
        sidebar.contains("websearch Disconnected") || sidebar.contains("No MCP servers configured")
    );
    assert!(!sidebar.contains("edit.hashline_apply"));
    assert!(!sidebar.contains("hashline_apply"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_low_activity_presentation_prefers_primary_stack() {
    exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp();
    exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity();

    let app = operator_rail_modified_file_only_app();
    let rail = build_operator_rail_model(&app);
    let render_text = |text: Text<'static>| {
        text.lines
            .into_iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let wide = render_text(build_operator_rail_body_text(&rail.body, app.theme(), 42));
    let narrow = render_text(build_operator_rail_body_text(&rail.body, app.theme(), 32));

    for rendered in [&wide, &narrow] {
        assert!(
            rendered.contains("▼ MCP"),
            "missing MCP section\n{rendered}"
        );
        assert!(
            rendered.contains("▼ LSP"),
            "missing LSP section\n{rendered}"
        );
        assert!(
            rendered.contains("▼ Modified Files"),
            "missing modified files section\n{rendered}"
        );
        assert!(
            rendered.contains("• demo.txt"),
            "missing modified file row\n{rendered}"
        );
    }
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_counts_generic_mcp_activity() {
    let _guard = operator_sidebar_config_test_guard();
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.remote_search.endpoint = "https://mcp.exa.ai/mcp".to_string();
    integrations.mcp.servers.insert(
        "fixture".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://example.com/mcp".to_string(),
            headers: std::collections::BTreeMap::new(),
            timeout_secs: 30,
            enabled: true,
        },
    );
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_mcp_server_connection_states(
        std::collections::BTreeMap::from([(
            "fixture".to_string(),
            harness_core::config::McpServerConnectionState::Connected,
        )]),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let mut app = operator_rail_test_app();

    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("w1".to_string()),
    );
    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-mcp".to_string(),
                text: "Use the MCP fixture".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        5,
        worker.clone(),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_mcp_1".to_string(),
                tool_id: "mcp.fixture.tool.call".to_string(),
                args_summary: r#"{"tool":"echo"}"#.to_string(),
                args_digest: "digest-mcp-1".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        6,
        worker.clone(),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_mcp_1".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("fixture ok".to_string()),
                output_digest: Some("digest-output-mcp-1".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        7,
        worker.clone(),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_search_1".to_string(),
                tool_id: "search.web".to_string(),
                args_summary: r#"{"query":"docs"}"#.to_string(),
                args_digest: "digest-search-1".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        8,
        worker,
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_search_1".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("search failed".to_string()),
                output_digest: Some("digest-output-search-1".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);

    assert_eq!(model.body.sections[0].heading(), "▼ MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.display_text())
            .collect::<Vec<_>>(),
        [
            "fixture Connected".to_string(),
            "websearch Disconnected".to_string(),
        ]
    );

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::Mcp),
        None,
    );

    let collapsed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(collapsed_sidebar.contains("▶ MCP (1 active, 1 error)"));
    assert!(!collapsed_sidebar.contains("• fixture Connected"));
    assert!(!collapsed_sidebar.contains("• websearch Disconnected"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_collapses_modified_files_section_body() {
    let mut app = operator_rail_test_app();
    app.ingest_event(operator_rail_test_event(
        50,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_more_1".to_string(),
            path: "src/ui.rs".to_string(),
            new_file_digest: "digest-edit-more-1".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        51,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_more_2".to_string(),
            path: "src/app.rs".to_string(),
            new_file_digest: "digest-edit-more-2".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::ModifiedFiles),
        None,
    );

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▶ Modified Files"));
    assert!(!sidebar.contains("src/ui_secondary.rs"));
    assert!(!sidebar.contains("src/ui.rs"));
    assert!(!sidebar.contains("src/app.rs"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_sidebar_hit_target_maps_section_headers() {
    let mut app = operator_rail_test_app();
    app.ingest_event(operator_rail_test_event(
        50,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_more_1".to_string(),
            path: "src/ui.rs".to_string(),
            new_file_digest: "digest-edit-more-1".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));

    let frame_area = Rect::new(0, 0, 160, 30);
    let sidebar_area = crate::layout::FrameLayoutPlan::for_app(&app, frame_area)
        .operator_sidebar
        .expect("persistent operator sidebar");
    let inner = operator_sidebar_inner_area(
        &app,
        sidebar_area,
        app.theme(),
        OperatorSidebarChrome::Persistent,
    )
    .expect("sidebar inner area");
    let column = inner.x.saturating_add(1);
    let hits = (inner.y..inner.y.saturating_add(inner.height))
        .filter_map(|row| operator_sidebar_section_hit_target(&app, frame_area, column, row))
        .collect::<std::collections::BTreeSet<_>>();

    assert!(hits.contains(&OperatorSidebarSection::Mcp));
    assert!(hits.contains(&OperatorSidebarSection::Lsp));
    assert!(hits.contains(&OperatorSidebarSection::ModifiedFiles));
}

#[cfg(test)]
fn operator_rail_test_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(Some("/tmp/sessions/run_fixture".into()), false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("w1".to_string()),
        ),
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "w1".to_string(),
            profile: "deep".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        2,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("w1".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_queue".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("tool:fs.read".to_string()),
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_1".to_string(),
            path: "src/ui_secondary.rs".to_string(),
            new_file_digest: "digest-edit-1".to_string(),
            diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
            diff_digest: Some("digest-edit-artifact-1".to_string()),
        }),
    ));
    app
}

#[cfg(test)]
fn operator_rail_modified_file_only_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(Some("/tmp/sessions/run_fixture".into()), false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_1".to_string(),
            path: "demo.txt".to_string(),
            new_file_digest: "digest-edit-1".to_string(),
            diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
            diff_digest: Some("digest-edit-artifact-1".to_string()),
        }),
    ));
    app
}

#[cfg(test)]
fn operator_rail_activity_test_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_activity_request".to_string(),
        seq: 1,
        run_id: "run_fixture".to_string(),
        mono_ms: 10,
        ts: None,
        actor: harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        correlation_id: Some("req_activity".to_string()),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload: harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_activity".to_string(),
                text: "Inspect operator rail taxonomy".to_string(),
            },
        ),
    });
    app.ingest_event(operator_rail_test_event(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_mcp".to_string(),
                tool_id: "exa.search".to_string(),
                args_summary: r#"{"query":"ratatui sidebar"}"#.to_string(),
                args_digest: "digest-tool-mcp".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_mcp".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_mcp".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Returned 3 results".to_string()),
                output_digest: Some("digest-tool-mcp-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        5,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_web".to_string(),
                tool_id: "search.web".to_string(),
                args_summary: r#"{"query":"sidebar parity"}"#.to_string(),
                args_digest: "digest-tool-web".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        6,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_web".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Fetched sidebar examples".to_string()),
                output_digest: Some("digest-tool-web-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        7,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_batch".to_string(),
                tool_id: "tool.batch".to_string(),
                args_summary: r#"{"requested_call_count":2}"#.to_string(),
                args_digest: "digest-tool-batch".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("tool.batch".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        8,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_batch".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        9,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_lsp".to_string(),
                tool_id: "code.lsp".to_string(),
                args_summary: r#"{"operation":"goto_definition","path":"src/ui_secondary.rs"}"#
                    .to_string(),
                args_digest: "digest-tool-lsp".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        10,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_lsp".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Found definition in src/ui_secondary.rs".to_string()),
                output_digest: Some("digest-tool-lsp-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app
}

#[cfg(test)]
fn operator_rail_test_event(
    seq: u64,
    actor: harness_core::event::EventActor,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt_{seq}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq * 10,
        ts: None,
        actor,
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

#[cfg(test)]
fn operator_rail_test_event_with_correlation(
    seq: u64,
    actor: harness_core::event::EventActor,
    correlation_id: &str,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    let mut event = operator_rail_test_event(seq, actor, payload);
    event.correlation_id = Some(correlation_id.to_string());
    event
}

fn help_row(app: &AppState, action: Action, label: &str) -> String {
    format!("  {:<12} {label}", app.keymap.get_binding_str(action))
}

fn newline_help_row(app: &AppState) -> String {
    let binding = match app
        .keymap
        .get_binding_strs(Action::InsertNewline)
        .as_slice()
    {
        [] => "-".to_string(),
        [binding] => binding.clone(),
        [first, second, ..] => format!("{first}/{second}"),
    };
    format!("  {:<20} Insert newline", binding)
}

fn help_text(app: &AppState) -> String {
    let mut lines = vec![
        "Keyboard Shortcuts:".to_string(),
        String::new(),
        "Navigation:".to_string(),
        help_row(app, Action::MoveDown, "Move down in list"),
        help_row(app, Action::MoveUp, "Move up in list"),
        help_row(app, Action::FocusNext, "Cycle focus forward"),
        help_row(app, Action::FocusPrev, "Cycle focus backward"),
        help_row(app, Action::ToggleFollow, "Toggle follow mode"),
    ];

    if app.replay_mode {
        lines.extend([
            String::new(),
            "Replay shell:".to_string(),
            "  Read-only transcript and review surfaces.".to_string(),
            help_row(app, Action::Reload, "Reload session"),
        ]);
    } else {
        lines.extend([
            String::new(),
            "Live shell:".to_string(),
            help_row(app, Action::CloseReviewSurface, "Return to session shell"),
            help_row(app, Action::ToggleTerminalPanel, "Toggle terminal panel"),
            String::new(),
            "Prompt (when focused):".to_string(),
            help_row(app, Action::SubmitPrompt, "Submit prompt"),
            newline_help_row(app),
            help_row(app, Action::ClearPrompt, "Clear prompt"),
            help_row(app, Action::HistoryUp, "History up"),
            help_row(app, Action::HistoryDown, "History down"),
        ]);
    }

    lines.extend([
        String::new(),
        "Permission modal:".to_string(),
        help_row(app, Action::AllowPermission, "Allow permission"),
        help_row(app, Action::DenyPermission, "Deny permission"),
        help_row(app, Action::DismissModal, "Reject permission"),
        String::new(),
        "General:".to_string(),
        help_row(app, Action::Help, "Show this help"),
    ]);

    lines.push(help_row(app, Action::Quit, "Quit"));
    lines.join("\n")
}

fn render_operator_sidebar_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    chrome: OperatorSidebarChrome,
) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);
    let surface = match chrome {
        OperatorSidebarChrome::Persistent => ui_chrome::divided_shell_surface(theme),
        OperatorSidebarChrome::Overlay => ui_chrome::divided_shell_surface(theme),
    };

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    let inner = match chrome {
        OperatorSidebarChrome::Persistent => inset_rect(
            area,
            theme.live_shell.rhythm.sidebar_padding_x,
            theme.live_shell.rhythm.sidebar_padding_y,
        ),
        OperatorSidebarChrome::Overlay => {
            let block =
                ui_chrome::secondary_pane_block(theme, Line::default(), is_focused, surface);
            let inner = block.inner(area);
            frame.render_widget(block, area);
            inner
        }
    };

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
    let footer = app
        .sidebar_directory_branch_label()
        .map(|label| operator_sidebar_directory_footer_text(label, theme, inner.width, surface));
    let title_height = title_text.lines.len().min(usize::from(u16::MAX)) as u16;
    let footer_height = footer
        .as_ref()
        .map(|footer| footer.height().min(usize::from(u16::MAX)) as u16)
        .unwrap_or(0);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(inner);
    let title_area = sections[0];
    let body_area = sections[1];
    let footer_area = sections[2];

    if title_height > 0 {
        frame.render_widget(
            Paragraph::new(title_text).style(panel_style(surface, theme.text.primary)),
            title_area,
        );
    }
    frame.render_widget(
        Paragraph::new(Text::from(body_layout.lines))
            .style(panel_style(surface, theme.text.primary))
            .scroll((app.details_scroll, 0))
            .wrap(Wrap { trim: true }),
        body_area,
    );
    render_operator_sidebar_selection(
        frame,
        app.operator_sidebar_selection(),
        operator_sidebar_selection_snapshot_in_surface(app, area, theme, chrome),
        body_area,
        theme,
    );
    if let Some(footer) = footer {
        frame.render_widget(
            Paragraph::new(footer)
                .style(Style::default().bg(surface))
                .wrap(Wrap { trim: false }),
            footer_area,
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

#[cfg(test)]
fn build_operator_rail_body_text(
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
) -> Text<'static> {
    Text::from(build_operator_rail_body_layout(body, theme, width, 0).lines)
}

fn build_operator_rail_model(app: &AppState) -> OperatorRailModel {
    let mut sections = Vec::new();
    let subagent_groups = operator_sidebar_subagent_groups(app);
    if !subagent_groups.is_empty() {
        sections.push(OperatorRailBodySection::Subagents {
            groups: subagent_groups,
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::Subagents,
                collapsed: app
                    .operator_sidebar_section_collapsed(OperatorSidebarSection::Subagents),
            },
        });
    }
    let todo_items = operator_sidebar_todo_items(app);
    if let Some(items) = todo_items {
        let disclosure = (items.len() > 2).then(|| OperatorRailSectionDisclosure {
            section: OperatorSidebarSection::Todo,
            collapsed: app.operator_sidebar_section_collapsed(OperatorSidebarSection::Todo),
        });
        sections.push(OperatorRailBodySection::Todo { items, disclosure });
    }
    sections.extend([
        OperatorRailBodySection::Mcp {
            items: operator_sidebar_mcp_items(app),
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::Mcp,
                collapsed: app.operator_sidebar_section_collapsed(OperatorSidebarSection::Mcp),
            },
        },
        OperatorRailBodySection::Lsp {
            items: operator_sidebar_lsp_items(app),
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::Lsp,
                collapsed: app.operator_sidebar_section_collapsed(OperatorSidebarSection::Lsp),
            },
        },
        OperatorRailBodySection::ModifiedFiles {
            items: operator_sidebar_modified_file_rows(app),
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::ModifiedFiles,
                collapsed: app
                    .operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles),
            },
        },
    ]);

    OperatorRailModel {
        title: operator_sidebar_session_title(app),
        body: OperatorRailBody { sections },
    }
}

fn build_operator_rail_body_layout(
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
    animation_phase: usize,
) -> OperatorRailBodyLayout {
    let mut lines = Vec::new();
    let mut heading_hit_regions = Vec::new();
    let mut subagent_hit_regions = Vec::new();
    let mut subagent_group_hit_regions = Vec::new();
    let mut visual_row = 0usize;

    let presentation = OperatorRailBodyPresentation::Regular;
    match presentation {
        OperatorRailBodyPresentation::Regular => {
            for (index, section) in body.sections.iter().enumerate() {
                if index > 0 {
                    let blank = Line::from("");
                    visual_row = visual_row.saturating_add(visual_rows_for_line(&blank, width));
                    lines.push(blank);
                }
                let section_top = visual_row;
                let section_lines =
                    build_operator_rail_section_lines(theme, section, width, animation_phase);
                if let Some(disclosure) = section.disclosure() {
                    let heading_height = section_lines
                        .first()
                        .map(|line| visual_rows_for_line(line, width))
                        .unwrap_or(1);
                    heading_hit_regions.push(OperatorRailHeadingHitRegion {
                        section: disclosure.section,
                        top_row: visual_row,
                        height: heading_height,
                    });
                }
                if matches!(section, OperatorRailBodySection::Subagents { .. })
                    && !section.collapsed()
                {
                    let subagent_regions = subagent_hit_regions_for_section(
                        section,
                        theme,
                        width,
                        section_top,
                        animation_phase,
                    );
                    subagent_hit_regions.extend(subagent_regions.item_regions);
                    subagent_group_hit_regions.extend(subagent_regions.group_regions);
                }
                visual_row =
                    visual_row.saturating_add(visual_rows_for_lines(&section_lines, width));
                lines.extend(section_lines);
            }
        }
    }

    OperatorRailBodyLayout {
        lines,
        heading_hit_regions,
        subagent_hit_regions,
        subagent_group_hit_regions,
    }
}

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
        app.transcript_animation_phase(),
    );
    Some(OperatorSidebarSelectionSnapshot {
        viewport: body_area,
        scroll_top: usize::from(app.details_scroll),
        rows: operator_sidebar_selection_rows(&body_layout.lines, body_area.width),
    })
}

fn operator_sidebar_selection_snapshot_in_surface(
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

fn render_operator_sidebar_selection(
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

fn build_operator_rail_section_lines(
    theme: &Theme,
    section: &OperatorRailBodySection,
    width: u16,
    animation_phase: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![section.heading_line(theme)];

    if section.collapsed() {
        return lines;
    }

    match section {
        OperatorRailBodySection::Subagents { groups, .. } => {
            for group in groups {
                if group.items.len() > 1 {
                    lines.push(subagent_group_line(theme, group, animation_phase, width));
                    if !group.expanded {
                        continue;
                    }
                }
                for item in &group.items {
                    lines.push(subagent_item_line(
                        theme,
                        group,
                        item,
                        animation_phase,
                        width,
                    ));
                }
            }
        }
        OperatorRailBodySection::Todo { items, .. }
        | OperatorRailBodySection::Mcp { items, .. }
        | OperatorRailBodySection::Lsp { items, .. }
        | OperatorRailBodySection::ModifiedFiles { items, .. } => {
            for item in items {
                append_operator_rail_item(&mut lines, theme, section, item);
            }
        }
    }

    lines
}

struct OperatorRailSubagentRegions {
    item_regions: Vec<OperatorRailSubagentHitRegion>,
    group_regions: Vec<OperatorRailSubagentGroupHitRegion>,
}

fn subagent_hit_regions_for_section(
    section: &OperatorRailBodySection,
    theme: &Theme,
    width: u16,
    section_top: usize,
    animation_phase: usize,
) -> OperatorRailSubagentRegions {
    let OperatorRailBodySection::Subagents { groups, .. } = section else {
        return OperatorRailSubagentRegions {
            item_regions: Vec::new(),
            group_regions: Vec::new(),
        };
    };

    let mut item_regions = Vec::new();
    let mut group_regions = Vec::new();
    let heading_height = visual_rows_for_line(&section.heading_line(theme), width);
    let mut visual_row = section_top.saturating_add(heading_height);
    for group in groups {
        if group.items.len() > 1 {
            let group_line = subagent_group_line(theme, group, animation_phase, width);
            let height = visual_rows_for_line(&group_line, width);
            group_regions.push(OperatorRailSubagentGroupHitRegion {
                agent_name: group.agent_name.clone(),
                top_row: visual_row,
                height,
            });
            visual_row = visual_row.saturating_add(height);
            if !group.expanded {
                continue;
            }
        }
        for item in &group.items {
            let item_line = subagent_item_line(theme, group, item, animation_phase, width);
            let height = visual_rows_for_line(&item_line, width);
            if let Some(session_id) = item.child_session_id.as_ref() {
                item_regions.push(OperatorRailSubagentHitRegion {
                    session_id: session_id.clone(),
                    top_row: visual_row,
                    height,
                });
            }
            visual_row = visual_row.saturating_add(height);
        }
    }
    OperatorRailSubagentRegions {
        item_regions,
        group_regions,
    }
}

fn subagent_group_line(
    theme: &Theme,
    group: &SubagentRailGroup,
    animation_phase: usize,
    width: u16,
) -> Line<'static> {
    let status = subagent_group_status(group);
    let active = status.is_active();
    subagent_compact_line(
        vec![
            StyledTextChunk {
                text: format!("{} ", if group.expanded { "▼" } else { "▶" }),
                style: subagent_bullet_style(theme, active),
            },
            StyledTextChunk {
                text: group.agent_name.clone(),
                style: Style::default().fg(theme.text.primary),
            },
            StyledTextChunk {
                text: format!(" {} ", status.glyph(animation_phase)),
                style: subagent_indicator_style(theme, status),
            },
            StyledTextChunk {
                text: subagent_group_summary(group),
                style: subagent_description_style(theme, status),
            },
        ],
        width,
    )
}

fn subagent_item_line(
    theme: &Theme,
    group: &SubagentRailGroup,
    item: &SubagentRailItem,
    animation_phase: usize,
    width: u16,
) -> Line<'static> {
    let multi_item = group.items.len() > 1;
    let leading = if multi_item { "  " } else { "• " };
    let bullet_style = subagent_bullet_style(theme, item.status.is_active());
    let mut chunks = Vec::new();
    chunks.push(StyledTextChunk {
        text: leading.to_string(),
        style: bullet_style,
    });
    if !multi_item {
        chunks.push(StyledTextChunk {
            text: group.agent_name.clone(),
            style: Style::default().fg(theme.text.primary),
        });
        chunks.push(StyledTextChunk {
            text: " ".to_string(),
            style: Style::default().fg(theme.text.secondary),
        });
    }
    chunks.extend([
        StyledTextChunk {
            text: format!("{} ", item.status.glyph(animation_phase)),
            style: subagent_indicator_style(theme, item.status),
        },
        StyledTextChunk {
            text: item.description.clone(),
            style: subagent_description_style(theme, item.status),
        },
    ]);
    subagent_compact_line(chunks, width)
}

fn subagent_group_status(group: &SubagentRailGroup) -> SubagentRailStatus {
    if let Some(status) = group
        .items
        .iter()
        .find_map(|item| item.status.is_active().then_some(item.status))
    {
        return status;
    }

    if group
        .items
        .iter()
        .any(|item| matches!(item.status, SubagentRailStatus::Error))
    {
        return SubagentRailStatus::Error;
    }

    SubagentRailStatus::Completed
}

fn subagent_group_summary(group: &SubagentRailGroup) -> String {
    let total = group.items.len();
    let active = group
        .items
        .iter()
        .filter(|item| item.status.is_active())
        .count();
    let failed = group
        .items
        .iter()
        .filter(|item| matches!(item.status, SubagentRailStatus::Error))
        .count();

    if active > 0 {
        return format!("{} · {} active", subagent_task_count(total), active);
    }
    if failed > 0 {
        return format!("{} · {} failed", subagent_task_count(total), failed);
    }

    format!("{} done", subagent_task_count(total))
}

fn subagent_task_count(count: usize) -> String {
    if count == 1 {
        "1 task".to_string()
    } else {
        format!("{count} tasks")
    }
}

fn subagent_bullet_style(theme: &Theme, active: bool) -> Style {
    if active {
        Style::default().fg(theme.status.success)
    } else {
        Style::default().fg(theme.text.primary)
    }
}

fn subagent_indicator_style(theme: &Theme, status: SubagentRailStatus) -> Style {
    if status.is_active() {
        Style::default().fg(theme.status.success)
    } else {
        Style::default().fg(theme.text.secondary)
    }
}

fn subagent_description_style(theme: &Theme, status: SubagentRailStatus) -> Style {
    if status.is_active() {
        Style::default().fg(theme.text.primary)
    } else {
        Style::default().fg(theme.text.secondary)
    }
}

fn subagent_compact_line(chunks: Vec<StyledTextChunk>, width: u16) -> Line<'static> {
    Line::from(truncate_sidebar_styled_chunks(
        &chunks,
        usize::from(width.max(1)),
    ))
}

fn truncate_sidebar_styled_chunks(
    chunks: &[StyledTextChunk],
    max_width: usize,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut rendered = Vec::new();
    let mut used = 0usize;
    for chunk in chunks {
        if used >= max_width {
            break;
        }
        let remaining = max_width - used;
        let text = truncate_plain_text(&chunk.text, remaining);
        used += display_width(&text);
        rendered.push(Span::styled(text, chunk.style));
    }
    rendered
}

fn visual_rows_for_lines(lines: &[Line<'static>], width: u16) -> usize {
    lines
        .iter()
        .map(|line| visual_rows_for_line(line, width))
        .sum()
}

fn visual_rows_for_line(line: &Line<'static>, width: u16) -> usize {
    if width == 0 {
        return 0;
    }

    let line_width = line.width();
    if line_width == 0 {
        return 1;
    }

    line_width.div_ceil(usize::from(width))
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
        app.transcript_animation_phase(),
    );
    let visual_row =
        usize::from(row.saturating_sub(body_area.y)).saturating_add(app.details_scroll.into());

    layout.heading_hit_regions.into_iter().find_map(|region| {
        (visual_row >= region.top_row && visual_row < region.top_row.saturating_add(region.height))
            .then_some(region.section)
    })
}

fn operator_sidebar_subagent_session_hit_target_in_surface(
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
        app.transcript_animation_phase(),
    );
    let visual_row =
        usize::from(row.saturating_sub(body_area.y)).saturating_add(app.details_scroll.into());

    layout.subagent_hit_regions.into_iter().find_map(|region| {
        (visual_row >= region.top_row && visual_row < region.top_row.saturating_add(region.height))
            .then_some(region.session_id)
    })
}

fn operator_sidebar_subagent_group_hit_target_in_surface(
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
        app.transcript_animation_phase(),
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

fn operator_sidebar_body_area(
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

fn operator_sidebar_footer_height(app: &AppState, theme: &Theme, width: u16) -> u16 {
    app.sidebar_directory_branch_label()
        .map(|label| {
            operator_sidebar_directory_footer_text(label, theme, width, theme.surface.panel)
                .height()
                .min(usize::from(u16::MAX)) as u16
        })
        .unwrap_or(0)
}

fn operator_sidebar_inner_area(
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

fn append_operator_rail_item(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    section: &OperatorRailBodySection,
    item: &OperatorRailItem,
) {
    let bullet_prefix = "• ";
    let continuation_prefix = if bullet_prefix == "• " {
        "  "
    } else {
        bullet_prefix
    };
    let mut raw_lines = item.text().lines();
    let Some(first_line) = raw_lines.next() else {
        return;
    };

    let prefix_style = Style::default().fg(theme.text.secondary);
    let primary_style = Style::default().fg(theme.text.primary);
    let secondary_style = Style::default().fg(theme.text.secondary);

    match item {
        OperatorRailItem::Plain(_) => lines.push(Line::from(Span::styled(
            format!("{bullet_prefix}{first_line}"),
            secondary_style,
        ))),
        OperatorRailItem::Todo { status, .. } => {
            let style = status.style(theme);
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", status.checkbox_glyph()), style),
                Span::styled(first_line.to_string(), style),
            ]));
        }
        OperatorRailItem::Status { suffix, state, .. } => {
            let dot_color = match state {
                RuntimeHealthState::Healthy => theme.status.success,
                RuntimeHealthState::Unhealthy => theme.status.error,
            };
            let label_style = if matches!(section, OperatorRailBodySection::Mcp { .. }) {
                primary_style
            } else {
                secondary_style
            };
            let mut spans = vec![
                Span::styled(bullet_prefix.to_string(), Style::default().fg(dot_color)),
                Span::styled(first_line.to_string(), label_style),
            ];
            if let Some(suffix) = suffix {
                spans.push(Span::styled(format!(" {suffix}"), secondary_style));
            }
            lines.push(Line::from(spans));
        }
        OperatorRailItem::ModifiedFile {
            additions,
            removals,
            ..
        } => {
            let mut spans = vec![
                Span::styled(bullet_prefix.to_string(), prefix_style),
                Span::styled(first_line.to_string(), secondary_style),
            ];
            if let (Some(additions), Some(removals)) = (additions, removals) {
                spans.push(Span::styled(" · ".to_string(), secondary_style));
                spans.push(Span::styled(
                    format!("+{additions}"),
                    Style::default().fg(theme.status.success),
                ));
                spans.push(Span::styled(" ".to_string(), secondary_style));
                spans.push(Span::styled(
                    format!("-{removals}"),
                    Style::default().fg(theme.status.error),
                ));
            }
            lines.push(Line::from(spans));
        }
    }

    for line in raw_lines {
        lines.push(Line::from(Span::styled(
            format!("{continuation_prefix}{line}"),
            secondary_style,
        )));
    }
}

fn operator_sidebar_session_title(app: &AppState) -> Option<OperatorRailTitle> {
    if let Some(title) = app
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            harness_core::event::EventV1::SessionTitleUpdated(data) => {
                Some(sanitize_operator_sidebar_line(&data.title))
            }
            _ => None,
        })
    {
        if !title.is_empty() {
            return Some(OperatorRailTitle::Generated(title));
        }
    }

    let user_title = app
        .activities
        .iter()
        .find_map(|activity| activity.user_message.as_ref())
        .map(|message| sanitize_operator_sidebar_line(&message.text))
        .filter(|text| !text.is_empty());

    let prompt_submitted = app
        .activities
        .iter()
        .any(|activity| activity.user_message.is_some());
    let provider_started = app.events.iter().any(|event| {
        matches!(
            event.payload,
            harness_core::event::EventV1::ProviderRequestStarted(_)
        )
    });

    if !app.replay_mode && prompt_submitted && !provider_started {
        return Some(OperatorRailTitle::Pending);
    }

    if let Some(title) = user_title {
        return Some(OperatorRailTitle::Generated(title));
    }

    None
}

fn operator_sidebar_todo_items(app: &AppState) -> Option<Vec<OperatorRailItem>> {
    let todos = app
        .activities
        .iter()
        .flat_map(|activity| activity.tool_calls.iter())
        .rev()
        .find_map(|tool_call| todo_items_from_tool_call(tool_call, app.session_path.as_deref()))?;

    let has_open_todo = todos.iter().any(|item| {
        matches!(
            item,
            OperatorRailItem::Todo { status, .. } if *status != TodoRailStatus::Completed
        )
    });
    (has_open_todo && !todos.is_empty()).then_some(todos)
}

fn operator_sidebar_subagent_groups(app: &AppState) -> Vec<SubagentRailGroup> {
    let mut groups: Vec<SubagentRailGroup> = Vec::new();
    let mut parent_tool_call_ids = std::collections::BTreeSet::new();
    let mut child_session_ids = std::collections::BTreeSet::new();
    let mut child_request_ids = std::collections::BTreeSet::new();
    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            if !operator_sidebar_tool_call_is_task_spawn(tool_call) {
                continue;
            }
            let Some((agent_name, description)) = subagent_args_from_tool_call(tool_call) else {
                continue;
            };
            let child_session_id = subagent_child_session_id(tool_call);
            parent_tool_call_ids.insert(tool_call.tool_call_id.clone());
            if let Some(child_session_id) = child_session_id.as_ref() {
                child_session_ids.insert(child_session_id.clone());
            }
            let child_request_id = subagent_child_request_id(tool_call);
            if let Some(child_request_id) = child_request_id.as_ref() {
                child_request_ids.insert(child_request_id.clone());
            }
            let item = SubagentRailItem {
                description,
                status: subagent_status_from_app(
                    app,
                    tool_call,
                    child_session_id.as_deref(),
                    child_request_id.as_deref(),
                ),
                child_session_id,
            };
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.agent_name == agent_name)
            {
                group.items.push(item);
            } else {
                groups.push(SubagentRailGroup {
                    expanded: app.operator_sidebar_subagent_group_expanded(&agent_name),
                    agent_name,
                    items: vec![item],
                });
            }
        }
    }

    if app.replay_mode {
        return groups;
    }

    for row in app.orchestration_visible_rows() {
        if row
            .parent_tool_call_id
            .as_ref()
            .is_some_and(|id| parent_tool_call_ids.contains(id))
        {
            continue;
        }
        if row
            .effective_child_session_id()
            .is_some_and(|id| child_session_ids.contains(id))
        {
            continue;
        }
        if row
            .effective_child_request_id()
            .is_some_and(|id| child_request_ids.contains(id))
        {
            continue;
        }
        let Some(agent_name) = row
            .queue_key
            .as_deref()
            .and_then(subagent_name_from_queue_key)
            .or_else(|| subagent_name_from_background_notification_row(app, &row))
            .or_else(|| subagent_name_from_child_orchestration_row(app, &row))
        else {
            continue;
        };
        let item = SubagentRailItem {
            description: subagent_description_from_orchestration_row(&row),
            status: SubagentRailStatus::from_orchestration_state(row.state),
            child_session_id: row.effective_child_session_id().map(str::to_string),
        };
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.agent_name == agent_name)
        {
            group.items.push(item);
        } else {
            groups.push(SubagentRailGroup {
                expanded: app.operator_sidebar_subagent_group_expanded(&agent_name),
                agent_name,
                items: vec![item],
            });
        }
    }

    groups
}

fn subagent_name_from_queue_key(queue_key: &str) -> Option<String> {
    ["agent:queue:", "agent:queued:", "agent:running:"]
        .iter()
        .find_map(|prefix| queue_key.strip_prefix(prefix))
        .map(sanitize_operator_sidebar_line)
        .filter(|name| !name.is_empty())
}

fn subagent_name_from_background_notification_row(
    app: &AppState,
    row: &crate::app::OrchestrationTaskRow,
) -> Option<String> {
    let notification = background_notification_for_orchestration_row(app, row)?;
    let agent_name = subagent_profile_for_agent_id(app, &notification.child_session_id)
        .or_else(|| subagent_profile_for_agent_id(app, &notification.task_id))
        .unwrap_or_else(|| "subagent".to_string());
    non_empty_sanitized_operator_sidebar_line(&agent_name)
}

fn subagent_name_from_child_orchestration_row(
    app: &AppState,
    row: &crate::app::OrchestrationTaskRow,
) -> Option<String> {
    let child_session_id = row.effective_child_session_id()?;
    child_subagent_profile_for_agent_id(app, child_session_id).or_else(|| {
        row.owner_agent_id
            .as_deref()
            .and_then(|id| child_subagent_profile_for_agent_id(app, id))
    })
}

fn background_notification_for_orchestration_row<'a>(
    app: &'a AppState,
    row: &crate::app::OrchestrationTaskRow,
) -> Option<&'a harness_core::event::BackgroundTaskNotificationEvent> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::BackgroundTaskNotification(data) = &event.payload else {
            return None;
        };
        (data.task_id == row.task_id
            || row.effective_child_request_id() == Some(data.child_request_id.as_str())
            || row.effective_child_session_id() == Some(data.child_session_id.as_str()))
        .then_some(data)
    })
}

fn subagent_profile_for_agent_id(app: &AppState, agent_id: &str) -> Option<String> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::AgentSpawned(data) = &event.payload else {
            return None;
        };
        (data.agent_id == agent_id)
            .then(|| non_empty_sanitized_operator_sidebar_line(&data.profile))
            .flatten()
    })
}

fn child_subagent_profile_for_agent_id(app: &AppState, agent_id: &str) -> Option<String> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::AgentSpawned(data) = &event.payload else {
            return None;
        };
        (data.agent_id == agent_id && data.parent_agent_id.is_some())
            .then(|| non_empty_sanitized_operator_sidebar_line(&data.profile))
            .flatten()
    })
}

fn non_empty_sanitized_operator_sidebar_line(text: &str) -> Option<String> {
    let sanitized = sanitize_operator_sidebar_line(text);
    (!sanitized.is_empty()).then_some(sanitized)
}

fn subagent_description_from_orchestration_row(row: &crate::app::OrchestrationTaskRow) -> String {
    let description = row
        .result_summary
        .as_deref()
        .unwrap_or(row.task_id.as_str());
    sanitize_operator_sidebar_line(description)
}

fn subagent_status_from_app(
    app: &AppState,
    tool_call: &crate::app::ToolCallEntry,
    child_session_id: Option<&str>,
    child_request_id: Option<&str>,
) -> SubagentRailStatus {
    if let Some(status) =
        subagent_status_from_background_notification(app, child_session_id, child_request_id)
    {
        return status;
    }

    if let Some(row) = app.orchestration_visible_rows().into_iter().find(|row| {
        row.parent_tool_call_id.as_deref() == Some(tool_call.tool_call_id.as_str())
            || child_session_id.is_some_and(|child| {
                row.effective_child_session_id() == Some(child) || row.task_id == child
            })
            || child_request_id.is_some_and(|child| row.effective_child_request_id() == Some(child))
    }) {
        return SubagentRailStatus::from_orchestration_state(row.state);
    }

    if tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_session_id.as_deref())
        .is_none()
        && child_session_id.is_some()
        && matches!(
            tool_call.status,
            crate::app::ToolCallDisplayStatus::Succeeded
        )
    {
        return SubagentRailStatus::Running;
    }

    SubagentRailStatus::from_tool_call_status(tool_call.status)
}

fn subagent_status_from_background_notification(
    app: &AppState,
    child_session_id: Option<&str>,
    child_request_id: Option<&str>,
) -> Option<SubagentRailStatus> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::BackgroundTaskNotification(data) = &event.payload else {
            return None;
        };
        let matches_child = child_request_id == Some(data.child_request_id.as_str())
            || child_session_id == Some(data.child_session_id.as_str())
            || child_session_id == Some(data.task_id.as_str());
        matches_child.then_some(match data.status {
            harness_core::event::BackgroundTaskNotificationStatus::Completed => {
                SubagentRailStatus::Completed
            }
            harness_core::event::BackgroundTaskNotificationStatus::Cancelled
            | harness_core::event::BackgroundTaskNotificationStatus::Failed
            | harness_core::event::BackgroundTaskNotificationStatus::TimedOut => {
                SubagentRailStatus::Error
            }
        })
    })
}

fn operator_sidebar_tool_call_is_task_spawn(tool_call: &crate::app::ToolCallEntry) -> bool {
    matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
        || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
}

fn subagent_args_from_tool_call(tool_call: &crate::app::ToolCallEntry) -> Option<(String, String)> {
    let value = serde_json::from_str::<serde_json::Value>(&tool_call.args_summary).ok()?;
    let agent_name = trimmed_json_string_field(
        Some(&value),
        &["subagent_type", "profile", "profile_name", "category"],
    )?;
    let description = trimmed_json_string_field(Some(&value), &["description"]).unwrap_or_default();
    Some((
        sanitize_operator_sidebar_line(&agent_name),
        sanitize_operator_sidebar_line(&description),
    ))
}

fn subagent_child_session_id(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_session_id.clone())
        .or_else(|| subagent_child_session_id_from_output(tool_call.output_json.as_ref()))
}

fn subagent_child_request_id(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_request_id.clone())
        .or_else(|| subagent_child_request_id_from_output(tool_call.output_json.as_ref()))
}

fn subagent_child_session_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &[
            "child_session_id",
            "session_id",
            "task_id",
            "childSessionId",
            "sessionId",
            "taskId",
        ],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["child_session", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["childSession", "sessionId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "sessionId"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["metadata", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "session_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "sessionId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_session_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "session_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "sessionId"])
    })
}

fn subagent_child_request_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &["child_request_id", "request_id", "requestId"],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "requestId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "requestId"])
    })
}

fn todo_items_from_tool_call(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<Vec<OperatorRailItem>> {
    if !matches!(
        tool_call.effective_tool_id(),
        "todo.write" | "todo.read" | "todowrite" | "todoread"
    ) && !matches!(tool_call.tool_id.as_str(), "todowrite" | "todoread")
    {
        return None;
    }
    if tool_call.status != crate::app::ToolCallDisplayStatus::Succeeded {
        return None;
    }

    let artifact_json = || todo_json_from_artifacts(tool_call, session_path);
    let todos = tool_call
        .output_json
        .as_ref()
        .and_then(todo_array_from_json)
        .cloned()
        .or_else(artifact_json)?;
    Some(
        todos
            .iter()
            .filter_map(|todo| {
                let content = todo.get("content")?.as_str()?;
                let content = sanitize_operator_sidebar_line(content);
                (!content.is_empty()).then(|| OperatorRailItem::Todo {
                    content,
                    status: todo
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .map(TodoRailStatus::from_value)
                        .unwrap_or(TodoRailStatus::Pending),
                })
            })
            .collect(),
    )
}

fn todo_array_from_json(value: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    value
        .get("todos")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .or_else(|| {
            value
                .get("structured_output")
                .and_then(todo_array_from_json)
        })
}

fn todo_json_from_artifacts(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<Vec<serde_json::Value>> {
    let session_path = session_path?;
    tool_call.artifact_refs.iter().find_map(|artifact| {
        if !(artifact.path.ends_with(".json") || artifact.path.ends_with(".txt")) {
            return None;
        }
        let path = Path::new(&artifact.path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return None;
        }
        let contents = std::fs::read_to_string(session_path.join(path)).ok()?;
        let json = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
        todo_array_from_json(&json).cloned()
    })
}

fn operator_sidebar_mcp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let mut items = BTreeMap::new();
    if let Some(integrations) = harness_core::config::registered_integrations_config() {
        if has_trimmed_content(&integrations.remote_search.endpoint) {
            items.insert("websearch".to_string(), RuntimeHealthState::Unhealthy);
        }
        for (name, server) in &integrations.mcp.servers {
            if !server.enabled() {
                continue;
            }
            let state = match harness_core::config::registered_mcp_server_connection_state(name) {
                Some(harness_core::config::McpServerConnectionState::Connected) => {
                    RuntimeHealthState::Healthy
                }
                Some(harness_core::config::McpServerConnectionState::Failed(_)) => {
                    RuntimeHealthState::Unhealthy
                }
                None => RuntimeHealthState::Unhealthy,
            };
            items.insert(sanitize_operator_sidebar_line(name), state);
        }
    }

    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            let Some(server_name) = runtime_mcp_server_name(tool_call) else {
                continue;
            };
            let Some(state) = runtime_health_state_for_tool_call(tool_call) else {
                continue;
            };
            items
                .entry(server_name)
                .and_modify(|entry| *entry = state)
                .or_insert(state);
        }
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain(
            "No MCP servers configured".to_string(),
        )]
    } else {
        items
            .into_iter()
            .map(|(name, state)| OperatorRailItem::Status {
                label: name,
                suffix: Some(
                    match state {
                        RuntimeHealthState::Healthy => "Connected",
                        RuntimeHealthState::Unhealthy => "Disconnected",
                    }
                    .to_string(),
                ),
                state,
            })
            .collect()
    }
}

fn operator_sidebar_lsp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let config = harness_core::config::registered_lsp_config();
    if config.disabled {
        return vec![OperatorRailItem::Plain("LSP is disabled".to_string())];
    }

    let mut items = BTreeMap::new();
    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            let Some(server_name) = runtime_lsp_server_name(tool_call) else {
                continue;
            };
            items.insert(server_name, RuntimeHealthState::Healthy);
        }
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain("No active LSP servers".to_string())]
    } else {
        items
            .into_iter()
            .map(|(name, state)| OperatorRailItem::Status {
                label: name,
                suffix: None,
                state,
            })
            .collect()
    }
}

fn runtime_health_state_for_tool_call(
    tool_call: &crate::app::ToolCallEntry,
) -> Option<RuntimeHealthState> {
    match tool_call.status {
        crate::app::ToolCallDisplayStatus::Succeeded
        | crate::app::ToolCallDisplayStatus::Running => Some(RuntimeHealthState::Healthy),
        crate::app::ToolCallDisplayStatus::Failed => Some(RuntimeHealthState::Unhealthy),
        crate::app::ToolCallDisplayStatus::PendingPermission
        | crate::app::ToolCallDisplayStatus::Queued => None,
    }
}

fn runtime_mcp_server_name(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    match tool_call.effective_tool_id() {
        canonical if canonical.starts_with("mcp.") => canonical
            .split('.')
            .nth(1)
            .map(sanitize_operator_sidebar_line)
            .filter(|name| !name.is_empty()),
        "search.web" | "search.code" => Some("websearch".to_string()),
        _ => None,
    }
}

fn runtime_lsp_server_name(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    match tool_call.effective_tool_id() {
        "lsp" | "lsp.rename" | "code.lsp" | "code.lsp.rename" => {
            trimmed_json_nested_string_field(tool_call.output_json.as_ref(), &["server", "name"])
                .or_else(|| ui_lsp::server_name_from_args(&tool_call.args_summary))
                .map(|name| sanitize_operator_sidebar_line(&name))
                .filter(|name| !name.is_empty())
        }
        _ => None,
    }
}

fn operator_sidebar_modified_file_rows(app: &AppState) -> Vec<OperatorRailItem> {
    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();

    for event in app.events.iter().rev() {
        let harness_core::event::EventV1::EditApplied(edit) = &event.payload else {
            continue;
        };
        if !seen.insert(edit.path.clone()) {
            continue;
        }

        let path = sanitize_operator_sidebar_line(&edit.path);
        let counts = edit.diff_rel_path.as_deref().and_then(|diff_rel_path| {
            app.session_path.as_ref().and_then(|session_path| {
                std::fs::read_to_string(session_path.join(diff_rel_path))
                    .ok()
                    .and_then(|diff_content| {
                        super::ui_diff::structured_diff_stats(&diff_content, Some(&edit.path), true)
                    })
            })
        });

        let (additions, removals) = counts
            .map(|(additions, removals)| (Some(additions), Some(removals)))
            .unwrap_or((None, None));
        items.push(OperatorRailItem::ModifiedFile {
            path,
            additions,
            removals,
        });
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain("No files modified".to_string())]
    } else {
        items
    }
}

fn activity_surface_visible(app: &AppState) -> bool {
    (app.replay_mode && app.active_tab == Tab::Run)
        || (!app.replay_mode && app.review_surface().is_none())
}

#[cfg(test)]
fn orchestration_card_lines(
    app: &AppState,
    rows: &[OrchestrationTaskRow],
    theme: &Theme,
    height: u16,
    width: u16,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(orchestration_summary_line(app, theme, width));

    if height == 1 {
        return lines;
    }

    lines.push(orchestration_warning_line(app, theme, width));
    let task_slots = usize::from(height.saturating_sub(2));
    if task_slots == 0 || rows.is_empty() {
        return lines;
    }

    if rows.len() <= task_slots {
        lines.extend(
            rows.iter()
                .map(|row| orchestration_task_line(app, row, theme, width)),
        );
        return lines;
    }

    if task_slots == 1 {
        lines.push(orchestration_overflow_line(rows.len(), theme));
        return lines;
    }

    let visible_task_count = task_slots.saturating_sub(1);
    lines.extend(
        rows.iter()
            .take(visible_task_count)
            .map(|row| orchestration_task_line(app, row, theme, width)),
    );
    lines.push(orchestration_overflow_line(
        rows.len().saturating_sub(visible_task_count),
        theme,
    ));
    lines
}

#[cfg(test)]
fn orchestration_summary_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let summary = app.orchestration_summary();
    let text = format!(
        "overview · {} active agents · {} queued · {} running · {} stale",
        summary.active_agents, summary.queued, summary.running, summary.stale
    );
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        muted_meta_style(theme),
    ))
}

#[cfg(test)]
fn orchestration_warning_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let warning = app.orchestration_latest_warning().unwrap_or("none");
    let text = format!("watch · {warning}");
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        Style::default().fg(theme.status.warning),
    ))
}

#[cfg(test)]
fn orchestration_task_line(
    app: &AppState,
    row: &OrchestrationTaskRow,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let (state_label, state_color) = orchestration_state_tokens(row.state, theme);
    let owner = app.orchestration_owner_labels(row);
    let queue_key = row.queue_key.as_deref().unwrap_or("queue:none");
    let detail = format!(
        "{} · {}/{} · {}",
        row.task_id, owner.label, owner.profile, queue_key
    );

    let badge_width = state_label.chars().count().saturating_add(4);
    let detail = truncate_plain_text(&detail, usize::from(width).saturating_sub(badge_width));

    Line::from(vec![
        status_badge(state_label, state_color, theme),
        Span::raw(" "),
        Span::styled(detail, muted_meta_style(theme)),
    ])
}

#[cfg(test)]
fn orchestration_overflow_line(hidden_count: usize, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("+{hidden_count} more"),
        Style::default()
            .fg(theme.text.tertiary)
            .add_modifier(Modifier::BOLD),
    ))
}

#[cfg(test)]
fn orchestration_state_tokens(
    state: OrchestrationTaskState,
    theme: &Theme,
) -> (&'static str, Color) {
    match state {
        OrchestrationTaskState::Queued => ("queued", theme.text.secondary),
        OrchestrationTaskState::Running => ("running", theme.status.info),
        OrchestrationTaskState::Stale => ("stale", theme.status.warning),
        OrchestrationTaskState::Completed => ("completed", theme.status.success),
        OrchestrationTaskState::Cancelled => ("cancelled", theme.status.error),
        OrchestrationTaskState::Failed => ("failed", theme.status.error),
        OrchestrationTaskState::TimedOut => ("timed-out", theme.status.error),
        OrchestrationTaskState::LateResult => ("late-result", theme.status.warning),
    }
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

#[cfg(test)]
fn line_to_plain_text(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_directory_footer_keeps_unbroken_path_segments_on_one_row() {
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
}

fn render_event_list(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List;

    let follow_indicator = if app.follow_mode { " · follow" } else { "" };
    let title = if app.replay_mode {
        "Event log".to_string()
    } else {
        format!("Event log · j/k active{follow_indicator}")
    };
    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);

    if app.events.is_empty() {
        let empty = Paragraph::new("No events")
            .block(block)
            .style(panel_style(surface, theme.text.secondary));
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<Line> = app
        .events
        .iter()
        .enumerate()
        .skip(app.events_trimmed_count)
        .map(|(idx, event)| {
            let display_idx = idx + 1;
            let is_selected = idx == app.selected_event_index;
            let prefix = if is_selected { ">" } else { " " };

            let style = if is_selected {
                Style::default()
                    .fg(theme.text.inverse)
                    .bg(theme.border.focus)
                    .add_modifier(Modifier::BOLD)
            } else {
                panel_style(surface, theme.text.primary)
            };

            let event_type = format!("{:?}", event.payload)
                .split(':')
                .next()
                .unwrap_or("Unknown")
                .to_string();

            let content = format!("{:>5} {} {}", display_idx, prefix, event_type);
            Line::from(Span::styled(content, style))
        })
        .collect();

    let list = Paragraph::new(Text::from(items))
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: false });

    frame.render_widget(list, area);
}

fn render_event_details(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details;

    let title = if app.replay_mode {
        "Selected event"
    } else {
        "Event details"
    };
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, title, is_focused, surface);

    let content = if let Some(event) = app.selected_event() {
        match serde_json::to_string_pretty(event) {
            Ok(json) => json,
            Err(_) => "Error serializing event".to_string(),
        }
    } else {
        "No event selected".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_secondary_surface_shell(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    summary: Line<'static>,
) -> Rect {
    let layout = secondary_surface_layout(area, theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.shell)),
        layout.shell,
    );
    if layout.body.width == 0 || layout.body.height == 0 {
        return layout.body;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(layout.body);
    frame.render_widget(
        Paragraph::new(summary).style(panel_style(theme.surface.shell, theme.text.secondary)),
        chunks[0],
    );
    chunks[1]
}

fn secondary_summary_line(
    app: &AppState,
    label: &'static str,
    accent: Color,
    detail: impl Into<String>,
    theme: &Theme,
) -> Line<'static> {
    Line::from(vec![
        status_badge(label, accent, theme),
        Span::styled("  ", panel_style(theme.surface.shell, theme.text.secondary)),
        Span::styled(
            detail.into(),
            panel_style(theme.surface.shell, theme.text.secondary),
        ),
        Span::styled(
            if app.replay_mode {
                "  ·  read-only"
            } else {
                ""
            },
            panel_style(theme.surface.shell, theme.text.tertiary),
        ),
    ])
}

fn events_summary_line(app: &AppState, theme: &Theme) -> Line<'static> {
    let selected = app.selected_event().map_or(0, |event| event.seq);
    secondary_summary_line(
        app,
        "events",
        theme.border.strong,
        format!(
            "{} recorded · selected seq {}{}",
            app.events.len(),
            selected,
            if app.follow_mode { " · follow on" } else { "" }
        ),
        theme,
    )
}

fn help_summary_line(app: &AppState, theme: &Theme) -> Line<'static> {
    secondary_summary_line(
        app,
        "help",
        theme.border.strong,
        if app.replay_mode {
            "replay controls and read-only navigation"
        } else {
            "live controls, drawers, and prompt shortcuts"
        },
        theme,
    )
}
