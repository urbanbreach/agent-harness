use imara_diff::{Algorithm, Diff, InternedInput};
use std::cmp::max;
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle as SyntectFontStyle, ScopeSelectors,
    StyleModifier as SyntectStyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings as SyntectThemeSettings,
};
use syntect::parsing::SyntaxSet;

use super::*;

use crate::app::OperatorSidebarSection;
#[cfg(test)]
use crate::app::{ActiveContextUsage, ActivityUsage, OrchestrationTaskRow, OrchestrationTaskState};
use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPatchFile {
    display_path: String,
    before_label: String,
    after_label: String,
    hunks: Vec<ParsedPatchHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPatchHunk {
    header: String,
    before_lines: Vec<String>,
    after_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuredDiffModel {
    files: Vec<StructuredDiffFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuredDiffFile {
    display_path: String,
    before_path: Option<String>,
    after_path: Option<String>,
    additions: usize,
    removals: usize,
    rows: Vec<StructuredDiffDisplayRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StructuredDiffDisplayRow {
    FileHeader,
    HunkHeader {
        text: String,
    },
    Context {
        before_line: Option<usize>,
        after_line: Option<usize>,
        text: String,
    },
    Changed {
        before: Option<DiffCell>,
        after: Option<DiffCell>,
    },
    Spacer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffCell {
    marker: char,
    line_number: Option<usize>,
    text: String,
    segments: Vec<DiffSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffSegment {
    kind: DiffSegmentKind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSegmentKind {
    Unchanged,
    Removed,
    Added,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorSidebarChrome {
    Persistent,
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailModel {
    title: Option<String>,
    body: OperatorRailBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailItem {
    Plain(String),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeHealthState {
    Healthy,
    Unhealthy,
}

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
            Self::Status { label, .. } => label,
            Self::ModifiedFile { path, .. } => path,
        }
    }

    #[cfg(test)]
    fn display_text(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
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

fn sanitize_operator_sidebar_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
struct OperatorRailBodyLayout {
    lines: Vec<Line<'static>>,
    heading_hit_regions: Vec<OperatorRailHeadingHitRegion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailBodySection {
    Context {
        items: Vec<String>,
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
    fn heading(&self) -> String {
        match self {
            Self::Context { .. } => "Context".to_string(),
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
            Self::Context { .. } => Line::from(Span::styled(self.heading(), heading_style)),
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
            Self::Context { .. }
            | Self::Mcp { .. }
            | Self::Lsp { .. }
            | Self::ModifiedFiles { .. } => Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        }
    }

    fn disclosure(&self) -> Option<OperatorRailSectionDisclosure> {
        match self {
            Self::Context { .. } => None,
            Self::Mcp { disclosure, .. }
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
            Self::Context { items } => items.clone(),
            Self::Mcp { items, .. }
            | Self::Lsp { items, .. }
            | Self::ModifiedFiles { items, .. } => {
                items.iter().map(OperatorRailItem::display_text).collect()
            }
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
    assert_eq!(model.body.sections[0].heading(), "Context");
    assert_eq!(model.body.sections[1].heading(), "▼ MCP");
    assert_eq!(model.body.sections[2].heading(), "▼ LSP");
    assert_eq!(model.body.sections[3].heading(), "▼ Modified Files");
    assert_eq!(
        model.body.sections[0].item_texts(),
        ["0 tokens".to_string()]
    );
    assert_eq!(
        model.body.sections[3].item_texts(),
        ["src/ui_secondary.rs".to_string()]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_compaction_applied_updates_active_context_usage_estimate() {
    let mut app = AppState::new_live(None, false, None);
    app.active_context_usage = Some(ActiveContextUsage::estimate(500));
    app.activities.push_back(ActivityEntry {
        request_id: "req_000001".to_string(),
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
        operator_sidebar_context_items(&app),
        vec![
            "active ctx 500 est".to_string(),
            "spent 500 total".to_string(),
        ]
    );

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
            },
        ),
    ));

    assert_eq!(
        operator_sidebar_context_items(&app),
        vec![
            "active ctx 120 est".to_string(),
            "compaction applied · active ctx ~120".to_string(),
            "spent 500 total".to_string(),
        ]
    );
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
            "Context".to_string(),
            "▼ MCP".to_string(),
            "▼ LSP".to_string(),
            "▶ Modified Files".to_string(),
        ]
    );
    assert_eq!(
        empty_model.body.sections[0].item_texts(),
        ["0 tokens".to_string()]
    );
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

    let context_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref().contains("0 tokens"))
        })
        .expect("context sidebar line");
    assert_eq!(context_line.spans.len(), 1);
    assert_eq!(context_line.spans[0].style.fg, Some(theme.text.secondary));

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
    assert_eq!(model.body.sections[0].heading(), "Context");
    assert!(model.body.sections[0]
        .item_texts()
        .iter()
        .any(|item| item == "0 tokens"));
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
    assert_eq!(model.body.sections[1].heading(), "▼ MCP");
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
    assert_eq!(model.body.sections[1].heading(), "▼ MCP");
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
            "Context".to_string(),
            "▼ MCP".to_string(),
            "▼ LSP".to_string(),
            "▼ Modified Files".to_string(),
        ]
    );
    let mcp_items = model.body.sections[1].item_texts();
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
            rendered.contains("Context"),
            "missing context section\n{rendered}"
        );
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
        worker,
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

    let model = build_operator_rail_model(&app);

    assert_eq!(model.body.sections[1].heading(), "▼ MCP");
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
    let title_text = build_operator_rail_title_text(rail.title.as_deref(), theme, inner.width);
    let body_layout = build_operator_rail_body_layout(&rail.body, theme, inner.width);
    let title_height = title_text.lines.len().min(usize::from(u16::MAX)) as u16;
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(title_height), Constraint::Min(0)])
        .split(inner);
    let title_area = sections[0];
    let body_area = sections[1];

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
}

#[cfg(test)]
fn build_operator_sidebar_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let rail = build_operator_rail_model(app);
    let mut lines = build_operator_rail_title_text(rail.title.as_deref(), theme, 80).lines;
    lines.extend(build_operator_rail_body_layout(&rail.body, theme, 80).lines);

    Text::from(lines)
}

fn build_operator_rail_title_text(title: Option<&str>, theme: &Theme, width: u16) -> Text<'static> {
    let Some(title) = title.filter(|title| !title.trim().is_empty()) else {
        return Text::from(Vec::<Line<'static>>::new());
    };

    Text::from(vec![
        Line::from(Span::styled(
            truncate_plain_text(title, usize::from(width)),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
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
    Text::from(build_operator_rail_body_layout(body, theme, width).lines)
}

fn build_operator_rail_model(app: &AppState) -> OperatorRailModel {
    OperatorRailModel {
        title: operator_sidebar_session_title(app),
        body: OperatorRailBody {
            sections: vec![
                OperatorRailBodySection::Context {
                    items: operator_sidebar_context_items(app),
                },
                OperatorRailBodySection::Mcp {
                    items: operator_sidebar_mcp_items(app),
                    disclosure: OperatorRailSectionDisclosure {
                        section: OperatorSidebarSection::Mcp,
                        collapsed: app
                            .operator_sidebar_section_collapsed(OperatorSidebarSection::Mcp),
                    },
                },
                OperatorRailBodySection::Lsp {
                    items: operator_sidebar_lsp_items(app),
                    disclosure: OperatorRailSectionDisclosure {
                        section: OperatorSidebarSection::Lsp,
                        collapsed: app
                            .operator_sidebar_section_collapsed(OperatorSidebarSection::Lsp),
                    },
                },
                OperatorRailBodySection::ModifiedFiles {
                    items: operator_sidebar_modified_file_rows(app),
                    disclosure: OperatorRailSectionDisclosure {
                        section: OperatorSidebarSection::ModifiedFiles,
                        collapsed: app.operator_sidebar_section_collapsed(
                            OperatorSidebarSection::ModifiedFiles,
                        ),
                    },
                },
            ],
        },
    }
}

fn build_operator_rail_body_layout(
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
) -> OperatorRailBodyLayout {
    let mut lines = Vec::new();
    let mut heading_hit_regions = Vec::new();
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
                let section_lines = build_operator_rail_section_lines(theme, section);
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
                visual_row =
                    visual_row.saturating_add(visual_rows_for_lines(&section_lines, width));
                lines.extend(section_lines);
            }
        }
    }

    OperatorRailBodyLayout {
        lines,
        heading_hit_regions,
    }
}

fn build_operator_rail_section_lines(
    theme: &Theme,
    section: &OperatorRailBodySection,
) -> Vec<Line<'static>> {
    let mut lines = vec![section.heading_line(theme)];

    if section.collapsed() {
        return lines;
    }

    let items = match section {
        OperatorRailBodySection::Context { items } => items
            .iter()
            .cloned()
            .map(OperatorRailItem::Plain)
            .collect::<Vec<_>>(),
        OperatorRailBodySection::Mcp { items, .. }
        | OperatorRailBodySection::Lsp { items, .. }
        | OperatorRailBodySection::ModifiedFiles { items, .. } => items.clone(),
    };

    for item in &items {
        append_operator_rail_item(&mut lines, theme, section, item);
    }

    lines
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
    let title_text = build_operator_rail_title_text(rail.title.as_deref(), theme, inner.width);
    let title_height = title_text.lines.len().min(usize::from(u16::MAX)) as u16;
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(title_height), Constraint::Min(0)])
        .split(inner);
    let body_area = sections[1];
    if !rect_contains(body_area, column, row) {
        return None;
    }

    let layout = build_operator_rail_body_layout(&rail.body, theme, inner.width);
    let visual_row =
        usize::from(row.saturating_sub(body_area.y)).saturating_add(app.details_scroll.into());

    layout.heading_hit_regions.into_iter().find_map(|region| {
        (visual_row >= region.top_row && visual_row < region.top_row.saturating_add(region.height))
            .then_some(region.section)
    })
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
    let bullet_prefix = if matches!(section, OperatorRailBodySection::Context { .. }) {
        "  "
    } else {
        "• "
    };
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

fn operator_sidebar_session_title(app: &AppState) -> Option<String> {
    app.activities
        .iter()
        .find_map(|activity| activity.user_message.as_ref())
        .map(|message| sanitize_operator_sidebar_line(&message.text))
        .filter(|text| !text.is_empty())
}

fn operator_sidebar_context_window_tokens(app: &AppState) -> Option<u32> {
    let model_id = app.activities.back().and_then(|activity| {
        (!activity.model_id.trim().is_empty()).then_some(activity.model_id.clone())
    });
    let metadata =
        crate::app::LaunchMetadata::new(app.active_profile(), app.active_provider(), model_id);
    metadata.context_window_tokens()
}

fn format_token_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + (digits.len() / 3));
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

fn operator_sidebar_context_items(app: &AppState) -> Vec<String> {
    let active_context = app.active_context_usage();
    let Some(active_context) = active_context else {
        return vec!["0 tokens".to_string()];
    };
    let mut items = if active_context.compacted_pending_refresh {
        vec!["active ctx compacted · updates next request".to_string()]
    } else {
        let active_tokens = active_context.tokens.map(u64::from).unwrap_or(0);
        vec![format!(
            "active ctx {} est",
            format_token_count(active_tokens)
        )]
    };
    if let Some(context_window_tokens) =
        operator_sidebar_context_window_tokens(app).filter(|value| *value > 0)
    {
        if let Some(active_tokens) = active_context.tokens {
            let percent =
                ((f64::from(active_tokens) / f64::from(context_window_tokens)) * 100.0).round();
            items.push(format!("{}% active", percent.clamp(0.0, 999.0) as u64));
        }
    }
    if let Some(status) = app.compaction_status() {
        items.push(status.message.clone());
    }
    items.push(format!(
        "spent {} total",
        format_token_count(app.cumulative_token_spend())
    ));
    items
}

fn operator_sidebar_mcp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let mut items = BTreeMap::new();
    if let Some(integrations) = harness_core::config::registered_integrations_config() {
        if !integrations.remote_search.endpoint.trim().is_empty() {
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
                Some(harness_core::config::McpServerConnectionState::Disconnected) => {
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
            json_nested_string_field(tool_call.output_json.as_ref(), &["server", "name"])
                .or_else(|| infer_lsp_server_name_from_args(&tool_call.args_summary))
                .map(|name| sanitize_operator_sidebar_line(&name))
                .filter(|name| !name.is_empty())
        }
        _ => None,
    }
}

fn json_nested_string_field(value: Option<&serde_json::Value>, path: &[&str]) -> Option<String> {
    let mut current = value?;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn infer_lsp_server_name_from_args(args_summary: &str) -> Option<String> {
    let args = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    let path = ["path", "filePath"]
        .iter()
        .find_map(|key| args.get(*key))
        .and_then(serde_json::Value::as_str)?;
    infer_lsp_server_name_for_path(path)
}

fn infer_lsp_server_name_for_path(path: &str) -> Option<String> {
    let extension = format!(
        ".{}",
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())?
            .to_ascii_lowercase()
    );
    let config = harness_core::config::registered_lsp_config();
    if config.disabled {
        return None;
    }

    let mut specs = BTreeMap::from([
        ("go".to_string(), (false, vec![".go".to_string()])),
        (
            "json".to_string(),
            (false, vec![".json".to_string(), ".jsonc".to_string()]),
        ),
        (
            "python".to_string(),
            (false, vec![".py".to_string(), ".pyi".to_string()]),
        ),
        ("rust".to_string(), (false, vec![".rs".to_string()])),
        (
            "typescript".to_string(),
            (
                false,
                vec![
                    ".ts".to_string(),
                    ".tsx".to_string(),
                    ".js".to_string(),
                    ".jsx".to_string(),
                    ".mjs".to_string(),
                    ".cjs".to_string(),
                    ".mts".to_string(),
                    ".cts".to_string(),
                ],
            ),
        ),
        (
            "yaml".to_string(),
            (false, vec![".yaml".to_string(), ".yml".to_string()]),
        ),
    ]);

    for (name, server) in config.servers {
        if let Some((disabled, extensions)) = specs.get_mut(&name) {
            *disabled = server.disabled;
            if let Some(custom_extensions) = server.extensions {
                *extensions = custom_extensions;
            }
        } else if let Some(extensions) = server.extensions {
            specs.insert(name, (server.disabled, extensions));
        }
    }

    specs
        .into_iter()
        .find_map(|(name, (disabled, extensions))| {
            (!disabled && extensions.iter().any(|candidate| candidate == &extension))
                .then_some(name)
        })
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
                        structured_diff_model_from_patch(&diff_content, Some(&edit.path), true).map(
                            |model| {
                                model.files.into_iter().fold(
                                    (0usize, 0usize),
                                    |(adds, removes), file| {
                                        (
                                            adds.saturating_add(file.additions),
                                            removes.saturating_add(file.removals),
                                        )
                                    },
                                )
                            },
                        )
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

#[derive(Debug, Clone, Copy)]
pub(super) struct StructuredDiffRenderOptions {
    pub force_stacked: bool,
    pub highlight_intraline: bool,
    pub highlight_syntax: bool,
    pub show_file_header: bool,
    pub show_hunk_header: bool,
}

pub(super) fn render_structured_diff_lines(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    render_structured_diff_lines_with_options(
        diff_content,
        fallback_path,
        prefix,
        width,
        StructuredDiffRenderOptions {
            force_stacked,
            highlight_intraline: true,
            highlight_syntax: false,
            show_file_header: true,
            show_hunk_header: true,
        },
        theme,
    )
}

pub(super) fn render_structured_diff_lines_with_options(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    options: StructuredDiffRenderOptions,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let model =
        structured_diff_model_from_patch(diff_content, fallback_path, options.highlight_intraline)?;
    Some(render_structured_diff_model(
        &model,
        prefix,
        width,
        options.force_stacked,
        options.highlight_syntax,
        options.show_file_header,
        options.show_hunk_header,
        theme,
    ))
}

fn structured_diff_model_from_patch(
    diff_content: &str,
    fallback_path: Option<&str>,
    highlight_intraline: bool,
) -> Option<StructuredDiffModel> {
    let files = parse_unified_diff_files(diff_content, fallback_path)?;
    Some(StructuredDiffModel {
        files: files
            .into_iter()
            .map(|file| build_structured_diff_file(file, highlight_intraline))
            .collect(),
    })
}

fn build_structured_diff_file(
    file: ParsedPatchFile,
    highlight_intraline: bool,
) -> StructuredDiffFile {
    let mut rows = Vec::new();
    let mut additions = 0;
    let mut removals = 0;

    for (index, hunk) in file.hunks.into_iter().enumerate() {
        if index == 0 {
            rows.push(StructuredDiffDisplayRow::FileHeader);
        } else {
            rows.push(StructuredDiffDisplayRow::Spacer);
        }
        rows.push(StructuredDiffDisplayRow::HunkHeader {
            text: hunk.header.clone(),
        });

        let aligned = align_patch_hunk(&hunk, highlight_intraline);
        additions += aligned.additions;
        removals += aligned.removals;
        rows.extend(aligned.rows);
    }

    StructuredDiffFile {
        display_path: file.display_path,
        before_path: normalize_patch_label(&file.before_label),
        after_path: normalize_patch_label(&file.after_label),
        additions,
        removals,
        rows,
    }
}

struct AlignedHunk {
    rows: Vec<StructuredDiffDisplayRow>,
    additions: usize,
    removals: usize,
}

fn align_patch_hunk(hunk: &ParsedPatchHunk, highlight_intraline: bool) -> AlignedHunk {
    let (mut before_line, mut after_line) = parse_hunk_start_lines(&hunk.header);
    let before_text = hunk.before_lines.join("\n");
    let after_text = hunk.after_lines.join("\n");
    let input = InternedInput::new(
        imara_diff::sources::lines(&before_text),
        imara_diff::sources::lines(&after_text),
    );
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let mut rows = Vec::new();
    let mut before_idx = 0usize;
    let mut after_idx = 0usize;

    for diff_hunk in diff.hunks() {
        let next_before = usize::try_from(diff_hunk.before.start).unwrap_or(before_idx);
        let next_after = usize::try_from(diff_hunk.after.start).unwrap_or(after_idx);
        let unchanged = max(
            next_before.saturating_sub(before_idx),
            next_after.saturating_sub(after_idx),
        );

        for offset in 0..unchanged {
            let line = hunk
                .before_lines
                .get(before_idx + offset)
                .or_else(|| hunk.after_lines.get(after_idx + offset))
                .cloned()
                .unwrap_or_default();
            rows.push(StructuredDiffDisplayRow::Context {
                before_line: visible_diff_line_number(before_line),
                after_line: visible_diff_line_number(after_line),
                text: line,
            });
            before_line = before_line.saturating_add(1);
            after_line = after_line.saturating_add(1);
        }
        before_idx = next_before;
        after_idx = next_after;

        let removed_end = usize::try_from(diff_hunk.before.end).unwrap_or(before_idx);
        let added_end = usize::try_from(diff_hunk.after.end).unwrap_or(after_idx);
        let removed = &hunk.before_lines[before_idx..removed_end];
        let added = &hunk.after_lines[after_idx..added_end];

        for pair_index in 0..max(removed.len(), added.len()) {
            match (removed.get(pair_index), added.get(pair_index)) {
                (Some(before), Some(after)) if before == after => {
                    rows.push(StructuredDiffDisplayRow::Context {
                        before_line: visible_diff_line_number(before_line),
                        after_line: visible_diff_line_number(after_line),
                        text: before.clone(),
                    });
                    before_line = before_line.saturating_add(1);
                    after_line = after_line.saturating_add(1);
                }
                (Some(before), Some(after)) => {
                    let (before_segments, after_segments) = if highlight_intraline {
                        word_diff_segments(before, after)
                    } else {
                        (
                            vec![DiffSegment {
                                kind: DiffSegmentKind::Unchanged,
                                text: before.clone(),
                            }],
                            vec![DiffSegment {
                                kind: DiffSegmentKind::Unchanged,
                                text: after.clone(),
                            }],
                        )
                    };
                    rows.push(StructuredDiffDisplayRow::Changed {
                        before: Some(DiffCell {
                            marker: '-',
                            line_number: visible_diff_line_number(before_line),
                            text: before.clone(),
                            segments: before_segments,
                        }),
                        after: Some(DiffCell {
                            marker: '+',
                            line_number: visible_diff_line_number(after_line),
                            text: after.clone(),
                            segments: after_segments,
                        }),
                    });
                    before_line = before_line.saturating_add(1);
                    after_line = after_line.saturating_add(1);
                }
                (Some(before), None) => rows.push(StructuredDiffDisplayRow::Changed {
                    before: Some(DiffCell {
                        marker: '-',
                        line_number: visible_diff_line_number(before_line),
                        text: before.clone(),
                        segments: vec![DiffSegment {
                            kind: if highlight_intraline {
                                DiffSegmentKind::Removed
                            } else {
                                DiffSegmentKind::Unchanged
                            },
                            text: before.clone(),
                        }],
                    }),
                    after: None,
                }),
                (None, Some(after)) => rows.push(StructuredDiffDisplayRow::Changed {
                    before: None,
                    after: Some(DiffCell {
                        marker: '+',
                        line_number: visible_diff_line_number(after_line),
                        text: after.clone(),
                        segments: vec![DiffSegment {
                            kind: if highlight_intraline {
                                DiffSegmentKind::Added
                            } else {
                                DiffSegmentKind::Unchanged
                            },
                            text: after.clone(),
                        }],
                    }),
                }),
                (None, None) => {}
            }
            match (removed.get(pair_index), added.get(pair_index)) {
                (Some(_), Some(_)) => {}
                (Some(_), None) => before_line = before_line.saturating_add(1),
                (None, Some(_)) => after_line = after_line.saturating_add(1),
                (None, None) => {}
            }
        }

        before_idx = removed_end;
        after_idx = added_end;
    }

    let trailing = max(
        hunk.before_lines.len().saturating_sub(before_idx),
        hunk.after_lines.len().saturating_sub(after_idx),
    );
    for offset in 0..trailing {
        let line = hunk
            .before_lines
            .get(before_idx + offset)
            .or_else(|| hunk.after_lines.get(after_idx + offset))
            .cloned()
            .unwrap_or_default();
        rows.push(StructuredDiffDisplayRow::Context {
            before_line: visible_diff_line_number(before_line),
            after_line: visible_diff_line_number(after_line),
            text: line,
        });
        before_line = before_line.saturating_add(1);
        after_line = after_line.saturating_add(1);
    }

    AlignedHunk {
        rows,
        additions: usize::try_from(diff.count_additions()).unwrap_or(usize::MAX),
        removals: usize::try_from(diff.count_removals()).unwrap_or(usize::MAX),
    }
}

fn word_diff_segments(before: &str, after: &str) -> (Vec<DiffSegment>, Vec<DiffSegment>) {
    let before_tokens = tokenize_diff_words(before);
    let after_tokens = tokenize_diff_words(after);

    if before_tokens.is_empty() || after_tokens.is_empty() {
        return (
            vec![DiffSegment {
                kind: DiffSegmentKind::Removed,
                text: before.to_string(),
            }],
            vec![DiffSegment {
                kind: DiffSegmentKind::Added,
                text: after.to_string(),
            }],
        );
    }

    let mut input = InternedInput::default();
    input.update_before(before_tokens.clone().into_iter());
    input.update_after(after_tokens.clone().into_iter());
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let mut before_segments = Vec::new();
    let mut after_segments = Vec::new();
    let mut before_idx = 0usize;
    let mut after_idx = 0usize;

    for hunk in diff.hunks() {
        let next_before = usize::try_from(hunk.before.start).unwrap_or(before_idx);
        let next_after = usize::try_from(hunk.after.start).unwrap_or(after_idx);
        push_diff_segments(
            &mut before_segments,
            &before_tokens[before_idx..next_before],
            DiffSegmentKind::Unchanged,
        );
        push_diff_segments(
            &mut after_segments,
            &after_tokens[after_idx..next_after],
            DiffSegmentKind::Unchanged,
        );
        push_diff_segments(
            &mut before_segments,
            &before_tokens[next_before..usize::try_from(hunk.before.end).unwrap_or(next_before)],
            DiffSegmentKind::Removed,
        );
        push_diff_segments(
            &mut after_segments,
            &after_tokens[next_after..usize::try_from(hunk.after.end).unwrap_or(next_after)],
            DiffSegmentKind::Added,
        );
        before_idx = usize::try_from(hunk.before.end).unwrap_or(before_idx);
        after_idx = usize::try_from(hunk.after.end).unwrap_or(after_idx);
    }

    push_diff_segments(
        &mut before_segments,
        &before_tokens[before_idx..],
        DiffSegmentKind::Unchanged,
    );
    push_diff_segments(
        &mut after_segments,
        &after_tokens[after_idx..],
        DiffSegmentKind::Unchanged,
    );

    (before_segments, after_segments)
}

fn push_diff_segments(target: &mut Vec<DiffSegment>, tokens: &[String], kind: DiffSegmentKind) {
    if tokens.is_empty() {
        return;
    }
    let chunk = tokens.concat();
    if let Some(previous) = target.last_mut() {
        if previous.kind == kind {
            previous.text.push_str(&chunk);
            return;
        }
    }
    target.push(DiffSegment { kind, text: chunk });
}

fn tokenize_diff_words(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut kind: Option<u8> = None;

    for ch in input.chars() {
        let next_kind = if ch.is_whitespace() {
            0
        } else if ch.is_alphanumeric() || ch == '_' {
            1
        } else {
            2
        };

        if next_kind == 2 {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(ch.to_string());
            kind = None;
            continue;
        }

        if kind.is_some_and(|existing| existing != next_kind) && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        current.push(ch);
        kind = Some(next_kind);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[expect(
    clippy::too_many_arguments,
    reason = "structured diff rendering keeps transcript/file/header toggles explicit at the call site"
)]
fn render_structured_diff_model(
    model: &StructuredDiffModel,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    highlight_syntax: bool,
    show_file_header: bool,
    show_hunk_header: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wide = !force_stacked && content_width >= usize::from(DIFF_SIDE_BY_SIDE_MIN_WIDTH);
    let mut lines = Vec::new();

    for (file_index, file) in model.files.iter().enumerate() {
        if file_index > 0 {
            lines.push(Line::from(""));
        }

        let line_number_width = structured_diff_line_number_width(file);

        for row in &file.rows {
            let syntax_path = file
                .after_path
                .as_deref()
                .or(file.before_path.as_deref())
                .or(Some(file.display_path.as_str()));
            match row {
                StructuredDiffDisplayRow::FileHeader => {
                    if show_file_header {
                        lines.push(render_diff_file_header(prefix, file, content_width, theme));
                    }
                }
                StructuredDiffDisplayRow::HunkHeader { text } => {
                    if show_hunk_header {
                        lines.push(render_diff_hunk_header(
                            prefix,
                            text,
                            content_width,
                            line_number_width,
                            theme,
                        ));
                    }
                }
                StructuredDiffDisplayRow::Context {
                    before_line,
                    after_line,
                    text,
                } => {
                    if wide {
                        lines.push(render_wide_diff_row(
                            prefix,
                            Some(&DiffCell {
                                marker: ' ',
                                line_number: *before_line,
                                text: text.clone(),
                                segments: vec![DiffSegment {
                                    kind: DiffSegmentKind::Unchanged,
                                    text: text.clone(),
                                }],
                            }),
                            Some(&DiffCell {
                                marker: ' ',
                                line_number: *after_line,
                                text: text.clone(),
                                segments: vec![DiffSegment {
                                    kind: DiffSegmentKind::Unchanged,
                                    text: text.clone(),
                                }],
                            }),
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            theme,
                        ));
                    } else {
                        lines.extend(render_stacked_diff_lines(
                            prefix,
                            ' ',
                            *before_line,
                            *after_line,
                            text,
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            theme,
                        ));
                    }
                }
                StructuredDiffDisplayRow::Changed { before, after } => {
                    if wide {
                        lines.push(render_wide_diff_row(
                            prefix,
                            before.as_ref(),
                            after.as_ref(),
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            theme,
                        ));
                    } else {
                        if let Some(before) = before {
                            lines.extend(render_stacked_diff_cell_lines(
                                prefix,
                                before,
                                content_width,
                                line_number_width,
                                syntax_path,
                                highlight_syntax,
                                theme,
                            ));
                        }
                        if let Some(after) = after {
                            lines.extend(render_stacked_diff_cell_lines(
                                prefix,
                                after,
                                content_width,
                                line_number_width,
                                syntax_path,
                                highlight_syntax,
                                theme,
                            ));
                        }
                    }
                }
                StructuredDiffDisplayRow::Spacer => lines.push(Line::from("")),
            }
        }
    }

    lines
}

#[expect(
    clippy::too_many_arguments,
    reason = "wide diff rows need explicit geometry, syntax, and palette inputs to preserve the rendering contract"
)]
fn render_wide_diff_row(
    prefix: &str,
    before: Option<&DiffCell>,
    after: Option<&DiffCell>,
    content_width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Line<'static> {
    let separator = "  ";
    let column_width = content_width.saturating_sub(display_width(separator)) / 2;
    let before_palette = before
        .map(|cell| diff_row_palette(cell.marker, theme))
        .unwrap_or_else(|| diff_row_palette(' ', theme));
    let after_palette = after
        .map(|cell| diff_row_palette(cell.marker, theme))
        .unwrap_or_else(|| diff_row_palette(' ', theme));
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.extend(render_diff_cell(
        before,
        column_width,
        true,
        line_number_width,
        before_palette,
        syntax_path,
        highlight_syntax,
        theme,
    ));
    spans.push(Span::styled(
        separator.to_string(),
        apply_optional_bg(muted_meta_style(theme), Some(diff_panel_background(theme))),
    ));
    spans.extend(render_diff_cell(
        after,
        column_width,
        false,
        line_number_width,
        after_palette,
        syntax_path,
        highlight_syntax,
        theme,
    ));
    Line::from(spans)
}

#[expect(
    clippy::too_many_arguments,
    reason = "diff cells keep width, side, syntax, and palette state explicit to avoid hidden layout coupling"
)]
fn render_diff_cell(
    cell: Option<&DiffCell>,
    width: usize,
    is_before: bool,
    line_number_width: usize,
    palette: DiffRowPalette,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let Some(cell) = cell else {
        return vec![padded_diff_span(width, Some(palette.content_bg))];
    };

    let marker_width = 2usize;
    let number_width = line_number_width.saturating_add(1);
    let text_width = width.saturating_sub(number_width + marker_width);
    let accent_kind = if is_before {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };

    let mut spans = vec![Span::styled(
        format_line_number(cell.line_number, line_number_width),
        diff_line_number_style(cell.marker, Some(palette.gutter_bg), theme),
    )];
    spans.push(Span::styled(
        format!("{} ", cell.marker),
        diff_marker_style(cell.marker, Some(palette.content_bg), theme),
    ));
    if highlight_syntax {
        if let Some(chunks) =
            highlight_diff_line_chunks(syntax_path, &cell.text, Some(palette.content_bg))
        {
            spans.extend(truncate_styled_chunks(&chunks, text_width));
        } else {
            spans.extend(truncate_diff_segments(
                &cell.segments,
                text_width,
                accent_kind,
                Some(palette.content_bg),
                theme,
            ));
        }
    } else {
        spans.extend(truncate_diff_segments(
            &cell.segments,
            text_width,
            accent_kind,
            Some(palette.content_bg),
            theme,
        ));
    }
    let used_width = spans.iter().map(Span::width).sum::<usize>();
    if used_width < width {
        spans.push(padded_diff_span(
            width - used_width,
            Some(palette.content_bg),
        ));
    }
    spans
}

fn truncate_diff_segments(
    segments: &[DiffSegment],
    max_width: usize,
    accent_kind: DiffSegmentKind,
    row_bg: Option<Color>,
    theme: &Theme,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut rendered = Vec::new();
    let mut used = 0usize;

    for segment in segments {
        if used >= max_width {
            break;
        }
        let remaining = max_width - used;
        let text = truncate_plain_text(&segment.text, remaining);
        used += display_width(&text);
        rendered.push(Span::styled(
            text,
            diff_segment_style(segment.kind, accent_kind, row_bg, theme),
        ));
    }

    rendered
}

fn wrap_diff_segments(segments: &[DiffSegment], max_width: usize) -> Vec<Vec<DiffSegment>> {
    if max_width == 0 {
        return vec![Vec::new()];
    }

    let mut lines = vec![Vec::new()];
    let mut remaining = max_width;

    for segment in segments {
        let mut rest = segment.text.as_str();
        if rest.is_empty() {
            continue;
        }

        loop {
            if remaining == 0 {
                lines.push(Vec::new());
                remaining = max_width;
            }

            let piece = take_width_prefix(rest, remaining);
            if piece.is_empty() {
                lines.push(Vec::new());
                remaining = max_width;
                continue;
            }

            push_wrapped_segment(&mut lines[..], segment.kind, piece);
            remaining = remaining.saturating_sub(display_width(piece));
            rest = &rest[piece.len()..];

            if rest.is_empty() {
                break;
            }

            lines.push(Vec::new());
            remaining = max_width;
        }
    }

    lines
}

fn wrap_plain_text_lines(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![String::new()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let piece = take_width_prefix(rest, max_width);
        if piece.is_empty() {
            lines.push(String::new());
            break;
        }
        lines.push(piece.to_string());
        rest = &rest[piece.len()..];
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn push_wrapped_segment(lines: &mut [Vec<DiffSegment>], kind: DiffSegmentKind, text: &str) {
    let Some(current) = lines.last_mut() else {
        return;
    };
    if let Some(last) = current.last_mut() {
        if last.kind == kind {
            last.text.push_str(text);
            return;
        }
    }
    current.push(DiffSegment {
        kind,
        text: text.to_string(),
    });
}

#[expect(
    clippy::too_many_arguments,
    reason = "stacked diff rows keep number gutters and width math explicit at the call site"
)]
fn render_stacked_diff_lines(
    prefix: &str,
    marker: char,
    before_line: Option<usize>,
    after_line: Option<usize>,
    text: &str,
    width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let palette = diff_row_palette(marker, theme);
    let number_width = line_number_width.saturating_add(1) * 2;
    let text_width = width.saturating_sub(number_width + 2);
    let rows = if highlight_syntax {
        highlight_diff_line_chunks(syntax_path, text, Some(palette.content_bg))
            .map(|chunks| wrap_styled_chunks(&chunks, text_width))
    } else {
        None
    };
    rows.unwrap_or_else(|| {
        wrap_plain_text_lines(text, text_width)
            .into_iter()
            .map(|chunk| {
                vec![StyledTextChunk {
                    text: chunk,
                    style: diff_segment_style(
                        DiffSegmentKind::Unchanged,
                        DiffSegmentKind::Unchanged,
                        Some(palette.content_bg),
                        theme,
                    ),
                }]
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .enumerate()
    .map(|(index, chunks)| {
        let mut spans = stacked_diff_gutter_spans(
            prefix,
            StackedDiffGutter {
                marker,
                before_line: (index == 0).then_some(before_line).flatten(),
                after_line: (index == 0).then_some(after_line).flatten(),
                show_marker: index == 0,
                line_number_width,
                gutter_bg: Some(palette.gutter_bg),
                content_bg: Some(palette.content_bg),
            },
            theme,
        );
        spans.extend(styled_chunks_to_spans(chunks));
        pad_diff_row_to_width_with_background(
            spans,
            display_width(prefix),
            width,
            Some(palette.content_bg),
        )
    })
    .collect()
}

fn render_stacked_diff_cell_lines(
    prefix: &str,
    cell: &DiffCell,
    width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let accent_kind = if cell.marker == '-' {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };
    let palette = diff_row_palette(cell.marker, theme);
    let number_width = line_number_width.saturating_add(1) * 2;
    let text_width = width.saturating_sub(number_width + 2);
    let rows = if highlight_syntax {
        highlight_diff_line_chunks(syntax_path, &cell.text, Some(palette.content_bg))
            .map(|chunks| wrap_styled_chunks(&chunks, text_width))
    } else {
        None
    };
    rows.unwrap_or_else(|| {
        wrap_diff_segments(&cell.segments, text_width)
            .into_iter()
            .map(|segments| {
                segments
                    .into_iter()
                    .map(|segment| StyledTextChunk {
                        text: segment.text,
                        style: diff_segment_style(
                            segment.kind,
                            accent_kind,
                            Some(palette.content_bg),
                            theme,
                        ),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .enumerate()
    .map(|(index, chunks)| {
        let mut spans = stacked_diff_gutter_spans(
            prefix,
            StackedDiffGutter {
                marker: cell.marker,
                before_line: (index == 0 && cell.marker == '-')
                    .then_some(cell.line_number)
                    .flatten(),
                after_line: (index == 0 && cell.marker == '+')
                    .then_some(cell.line_number)
                    .flatten(),
                show_marker: index == 0,
                line_number_width,
                gutter_bg: Some(palette.gutter_bg),
                content_bg: Some(palette.content_bg),
            },
            theme,
        );
        spans.extend(styled_chunks_to_spans(chunks));
        pad_diff_row_to_width_with_background(
            spans,
            display_width(prefix),
            width,
            Some(palette.content_bg),
        )
    })
    .collect()
}

#[derive(Debug, Clone, Copy)]
struct DiffRowPalette {
    gutter_bg: Color,
    content_bg: Color,
}

#[derive(Debug, Clone)]
struct StyledTextChunk {
    text: String,
    style: Style,
}

struct StackedDiffGutter {
    marker: char,
    before_line: Option<usize>,
    after_line: Option<usize>,
    show_marker: bool,
    line_number_width: usize,
    gutter_bg: Option<Color>,
    content_bg: Option<Color>,
}

fn stacked_diff_gutter_spans(
    prefix: &str,
    gutter: StackedDiffGutter,
    theme: &Theme,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
        Span::styled(
            format_line_number(gutter.before_line, gutter.line_number_width),
            diff_line_number_style(gutter.marker, gutter.gutter_bg, theme),
        ),
        Span::styled(
            format_line_number(gutter.after_line, gutter.line_number_width),
            diff_line_number_style(gutter.marker, gutter.gutter_bg, theme),
        ),
        if gutter.show_marker {
            Span::styled(
                format!("{} ", gutter.marker),
                diff_marker_style(gutter.marker, gutter.content_bg, theme),
            )
        } else {
            Span::styled(
                "  ".to_string(),
                apply_optional_bg(Style::default(), gutter.content_bg),
            )
        },
    ]
}

fn diff_marker_style(marker: char, row_bg: Option<Color>, _theme: &Theme) -> Style {
    let style = match marker {
        '+' => Style::default().fg(opencode_diff_highlight_added()),
        '-' => Style::default().fg(opencode_diff_highlight_removed()),
        _ => muted_meta_style(_theme),
    };
    apply_optional_bg(style, row_bg)
}

fn diff_line_number_style(marker: char, row_bg: Option<Color>, theme: &Theme) -> Style {
    let gutter_fg = match marker {
        ' ' => theme.text.secondary,
        _ => theme.text.secondary,
    };
    apply_optional_bg(Style::default().fg(gutter_fg), row_bg)
}

fn diff_segment_style(
    kind: DiffSegmentKind,
    accent_kind: DiffSegmentKind,
    row_bg: Option<Color>,
    theme: &Theme,
) -> Style {
    match kind {
        DiffSegmentKind::Unchanged => {
            apply_optional_bg(Style::default().fg(theme.text.primary), row_bg)
        }
        DiffSegmentKind::Removed => {
            let fg = match accent_kind {
                DiffSegmentKind::Removed => theme.text.primary,
                _ => opencode_diff_highlight_removed(),
            };
            apply_optional_bg(Style::default().fg(fg), row_bg)
        }
        DiffSegmentKind::Added => {
            let fg = match accent_kind {
                DiffSegmentKind::Added => theme.text.primary,
                _ => opencode_diff_highlight_added(),
            };
            apply_optional_bg(Style::default().fg(fg), row_bg)
        }
    }
}

fn render_diff_hunk_header(
    prefix: &str,
    text: &str,
    width: usize,
    line_number_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let palette = diff_hunk_palette(theme);
    let header_width = width.saturating_sub(line_number_width.saturating_add(1) * 2 + 2);
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.push(Span::styled(
        format_line_number(None, line_number_width),
        diff_line_number_style(' ', Some(palette.gutter_bg), theme),
    ));
    spans.push(Span::styled(
        format_line_number(None, line_number_width),
        diff_line_number_style(' ', Some(palette.gutter_bg), theme),
    ));
    spans.push(Span::styled(
        "  ".to_string(),
        apply_optional_bg(Style::default(), Some(palette.content_bg)),
    ));
    spans.push(Span::styled(
        truncate_plain_text(&format!("⋮ {text}"), header_width),
        apply_optional_bg(
            Style::default().fg(opencode_diff_hunk_header()),
            Some(palette.content_bg),
        ),
    ));
    pad_diff_row_to_width_with_background(
        spans,
        display_width(prefix),
        width,
        Some(palette.content_bg),
    )
}

fn render_diff_file_header(
    prefix: &str,
    file: &StructuredDiffFile,
    content_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let row_bg = Some(diff_panel_background(theme));
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.push(Span::styled(
        "← Patched ".to_string(),
        apply_optional_bg(Style::default().fg(theme.text.secondary), row_bg),
    ));

    if file.before_path != file.after_path {
        if let Some(before_path) = file.before_path.as_ref() {
            spans.extend(render_diff_path_spans(
                before_path,
                muted_meta_style(theme),
                Style::default().fg(theme.text.primary),
                row_bg,
            ));
            spans.push(Span::styled(
                " → ".to_string(),
                apply_optional_bg(Style::default().fg(theme.text.secondary), row_bg),
            ));
        }
        spans.extend(render_diff_path_spans(
            file.after_path.as_deref().unwrap_or(&file.display_path),
            muted_meta_style(theme),
            Style::default().fg(theme.text.primary),
            row_bg,
        ));
    } else {
        spans.extend(render_diff_path_spans(
            &file.display_path,
            muted_meta_style(theme),
            Style::default().fg(theme.text.primary),
            row_bg,
        ));
    }
    pad_diff_row_to_width_with_background(spans, display_width(prefix), content_width, row_bg)
}

fn render_diff_path_spans(
    path: &str,
    directory_style: Style,
    filename_style: Style,
    row_bg: Option<Color>,
) -> Vec<Span<'static>> {
    let (directory, filename) = split_diff_path(path);
    let mut spans = Vec::new();
    if !directory.is_empty() {
        spans.push(Span::styled(
            format!("{directory}/"),
            apply_optional_bg(directory_style, row_bg),
        ));
    }
    spans.push(Span::styled(
        filename.to_string(),
        apply_optional_bg(filename_style, row_bg),
    ));
    spans
}

fn pad_diff_row_to_width_with_background(
    mut spans: Vec<Span<'static>>,
    prefix_width: usize,
    content_width: usize,
    background: Option<Color>,
) -> Line<'static> {
    let used_width = spans.iter().map(Span::width).sum::<usize>();
    let target_width = prefix_width.saturating_add(content_width);
    if used_width < target_width {
        spans.push(padded_diff_span(target_width - used_width, background));
    }
    Line::from(spans)
}

fn structured_diff_line_number_width(file: &StructuredDiffFile) -> usize {
    let max_line = file.rows.iter().fold(0usize, |current, row| match row {
        StructuredDiffDisplayRow::Context {
            before_line,
            after_line,
            ..
        } => current
            .max(before_line.unwrap_or(0))
            .max(after_line.unwrap_or(0)),
        StructuredDiffDisplayRow::Changed { before, after } => current
            .max(
                before
                    .as_ref()
                    .and_then(|cell| cell.line_number)
                    .unwrap_or(0),
            )
            .max(
                after
                    .as_ref()
                    .and_then(|cell| cell.line_number)
                    .unwrap_or(0),
            ),
        _ => current,
    });
    max(4, max_line.to_string().len())
}

fn split_diff_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn format_line_number(line: Option<usize>, width: usize) -> String {
    match line {
        Some(value) => format!("{value:>width$} "),
        None => " ".repeat(width.saturating_add(1)),
    }
}

fn padded_diff_span(width: usize, row_bg: Option<Color>) -> Span<'static> {
    if let Some(background) = row_bg {
        Span::styled(" ".repeat(width), Style::default().bg(background))
    } else {
        Span::raw(" ".repeat(width))
    }
}

fn styled_chunks_to_spans(chunks: Vec<StyledTextChunk>) -> Vec<Span<'static>> {
    chunks
        .into_iter()
        .map(|chunk| Span::styled(chunk.text, chunk.style))
        .collect()
}

fn truncate_styled_chunks(chunks: &[StyledTextChunk], max_width: usize) -> Vec<Span<'static>> {
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

fn wrap_styled_chunks(chunks: &[StyledTextChunk], max_width: usize) -> Vec<Vec<StyledTextChunk>> {
    if max_width == 0 {
        return vec![Vec::new()];
    }

    let mut lines = vec![Vec::new()];
    let mut remaining = max_width;

    for chunk in chunks {
        let mut rest = chunk.text.as_str();
        if rest.is_empty() {
            continue;
        }

        loop {
            if remaining == 0 {
                lines.push(Vec::new());
                remaining = max_width;
            }

            let piece = take_width_prefix(rest, remaining);
            if piece.is_empty() {
                lines.push(Vec::new());
                remaining = max_width;
                continue;
            }

            if let Some(current) = lines.last_mut() {
                current.push(StyledTextChunk {
                    text: piece.to_string(),
                    style: chunk.style,
                });
            }
            remaining = remaining.saturating_sub(display_width(piece));
            rest = &rest[piece.len()..];

            if rest.is_empty() {
                break;
            }

            lines.push(Vec::new());
            remaining = max_width;
        }
    }

    lines
}

fn highlight_diff_line_chunks(
    path: Option<&str>,
    text: &str,
    row_bg: Option<Color>,
) -> Option<Vec<StyledTextChunk>> {
    let path = path?;
    let assets = diff_syntax_highlight_assets();
    let syntax = assets
        .syntax_set
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| {
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .and_then(|extension| assets.syntax_set.find_syntax_by_extension(extension))
        })?;
    let mut highlighter = HighlightLines::new(syntax, &assets.theme);
    let regions = highlighter.highlight_line(text, &assets.syntax_set).ok()?;
    Some(
        regions
            .into_iter()
            .map(|(style, content)| StyledTextChunk {
                text: content.to_string(),
                style: diff_syntect_style_to_ratatui(style, row_bg),
            })
            .collect(),
    )
}

fn diff_syntax_highlight_assets() -> &'static DiffSyntaxHighlightAssets {
    static SYNTAX_ASSETS: OnceLock<DiffSyntaxHighlightAssets> = OnceLock::new();

    SYNTAX_ASSETS.get_or_init(|| {
        let syntax_set = SyntaxSet::load_defaults_nonewlines();
        let theme = opencode_diff_syntect_theme();
        DiffSyntaxHighlightAssets { syntax_set, theme }
    })
}

fn opencode_diff_syntect_theme() -> SyntectTheme {
    let mut scopes = Vec::new();
    push_syntect_scope(
        &mut scopes,
        "comment, comment.documentation",
        Some(opencode_syntax_comment()),
        None,
        Some(SyntectFontStyle::ITALIC),
    );
    push_syntect_scope(
        &mut scopes,
        "string, string.quoted, string.unquoted, symbol, character.special, constant.character.escape",
        Some(opencode_syntax_string()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "number, boolean, constant.numeric, constant.language.boolean, constant",
        Some(opencode_syntax_number()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "keyword, keyword.control, keyword.return, keyword.conditional, keyword.repeat, keyword.coroutine, storage, storage.modifier",
        Some(opencode_syntax_keyword()),
        None,
        Some(SyntectFontStyle::ITALIC),
    );
    push_syntect_scope(
        &mut scopes,
        "keyword.import, keyword.export, string.escape, string.regexp, keyword.directive, keyword.modifier, keyword.exception, tag.attribute",
        Some(opencode_syntax_keyword()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "keyword.type, storage.type, storage.type.primitive",
        Some(opencode_syntax_type()),
        None,
        Some(SyntectFontStyle::BOLD.union(SyntectFontStyle::ITALIC)),
    );
    push_syntect_scope(
        &mut scopes,
        "keyword.function, function.method, variable.member, function, constructor, entity.name.function, support.function, support.function.builtin",
        Some(opencode_syntax_function()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "variable, variable.parameter, function.method.call, function.call, property, parameter, field",
        Some(opencode_syntax_variable()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "type, module, namespace, class, type.definition, entity.name.type, support.type, support.class",
        Some(opencode_syntax_type()),
        None,
        Some(SyntectFontStyle::BOLD),
    );
    push_syntect_scope(
        &mut scopes,
        "operator, keyword.operator, keyword.operator.word, punctuation.delimiter, punctuation.separator, keyword.conditional.ternary, tag.delimiter",
        Some(opencode_syntax_operator()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "punctuation, punctuation.bracket",
        Some(opencode_syntax_punctuation()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "variable.builtin, type.builtin, function.builtin, module.builtin, constant.builtin, tag, attribute, annotation",
        Some(opencode_syntax_error()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "markup.raw, markup.raw.block, markup.raw.inline",
        Some(opencode_syntax_string()),
        None,
        None,
    );

    SyntectTheme {
        name: Some("opencode-diff".to_string()),
        author: Some("agent-harness".to_string()),
        settings: SyntectThemeSettings {
            foreground: Some(opencode_syntax_punctuation()),
            background: Some(opencode_diff_context_bg()),
            ..SyntectThemeSettings::default()
        },
        scopes,
    }
}

fn push_syntect_scope(
    scopes: &mut Vec<ThemeItem>,
    selector: &str,
    foreground: Option<SyntectColor>,
    background: Option<SyntectColor>,
    font_style: Option<SyntectFontStyle>,
) {
    let scope = ScopeSelectors::from_str(selector)
        .unwrap_or_else(|error| panic!("invalid syntect selector {selector:?}: {error:?}"));
    scopes.push(ThemeItem {
        scope,
        style: SyntectStyleModifier {
            foreground,
            background,
            font_style,
        },
    });
}

fn syntect_rgb(red: u8, green: u8, blue: u8) -> SyntectColor {
    SyntectColor {
        r: red,
        g: green,
        b: blue,
        a: 0xFF,
    }
}

fn opencode_diff_context_bg() -> SyntectColor {
    syntect_rgb(0x14, 0x14, 0x14)
}

fn opencode_syntax_comment() -> SyntectColor {
    syntect_rgb(0x80, 0x80, 0x80)
}

fn opencode_syntax_keyword() -> SyntectColor {
    syntect_rgb(0x9D, 0x7C, 0xD8)
}

fn opencode_syntax_function() -> SyntectColor {
    syntect_rgb(0xFA, 0xB2, 0x83)
}

fn opencode_syntax_variable() -> SyntectColor {
    syntect_rgb(0xE0, 0x6C, 0x75)
}

fn opencode_syntax_string() -> SyntectColor {
    syntect_rgb(0x7F, 0xD8, 0x8F)
}

fn opencode_syntax_number() -> SyntectColor {
    syntect_rgb(0xF5, 0xA7, 0x42)
}

fn opencode_syntax_type() -> SyntectColor {
    syntect_rgb(0xE5, 0xC0, 0x7B)
}

fn opencode_syntax_operator() -> SyntectColor {
    syntect_rgb(0x56, 0xB6, 0xC2)
}

fn opencode_syntax_punctuation() -> SyntectColor {
    syntect_rgb(0xEE, 0xEE, 0xEE)
}

fn opencode_syntax_error() -> SyntectColor {
    syntect_rgb(0xE0, 0x6C, 0x75)
}

struct DiffSyntaxHighlightAssets {
    syntax_set: SyntaxSet,
    theme: SyntectTheme,
}

fn diff_syntect_style_to_ratatui(
    style: syntect::highlighting::Style,
    row_bg: Option<Color>,
) -> Style {
    let mut rendered = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));

    if let Some(row_bg) = row_bg {
        rendered = rendered.bg(row_bg);
    }
    if style.font_style.contains(SyntectFontStyle::BOLD) {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(SyntectFontStyle::ITALIC) {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(SyntectFontStyle::UNDERLINE) {
        rendered = rendered.add_modifier(Modifier::UNDERLINED);
    }
    rendered
}

fn diff_row_palette(marker: char, theme: &Theme) -> DiffRowPalette {
    match marker {
        '+' => DiffRowPalette {
            gutter_bg: opencode_diff_added_line_number_bg(),
            content_bg: opencode_diff_added_bg(),
        },
        '-' => DiffRowPalette {
            gutter_bg: opencode_diff_removed_line_number_bg(),
            content_bg: opencode_diff_removed_bg(),
        },
        _ => DiffRowPalette {
            gutter_bg: diff_context_background(theme),
            content_bg: diff_panel_background(theme),
        },
    }
}

fn diff_panel_background(theme: &Theme) -> Color {
    theme.surface.panel
}

fn diff_context_background(theme: &Theme) -> Color {
    theme.surface.panel
}

fn diff_hunk_palette(theme: &Theme) -> DiffRowPalette {
    DiffRowPalette {
        gutter_bg: diff_context_background(theme),
        content_bg: diff_context_background(theme),
    }
}

fn opencode_diff_added_bg() -> Color {
    Color::Rgb(0x20, 0x30, 0x3B)
}

fn opencode_diff_removed_bg() -> Color {
    Color::Rgb(0x37, 0x22, 0x2C)
}

fn opencode_diff_added_line_number_bg() -> Color {
    Color::Rgb(0x1B, 0x2B, 0x34)
}

fn opencode_diff_removed_line_number_bg() -> Color {
    Color::Rgb(0x2D, 0x1F, 0x26)
}

fn opencode_diff_highlight_added() -> Color {
    Color::Rgb(0xB8, 0xDB, 0x87)
}

fn opencode_diff_highlight_removed() -> Color {
    Color::Rgb(0xE2, 0x6A, 0x75)
}

fn opencode_diff_hunk_header() -> Color {
    Color::Rgb(0x82, 0x8B, 0xB8)
}

fn apply_optional_bg(style: Style, background: Option<Color>) -> Style {
    background.map_or(style, |bg| style.bg(bg))
}

fn parse_hunk_start_lines(header: &str) -> (usize, usize) {
    let mut parts = header.trim().trim_matches('@').split_whitespace();
    let before = parts.next().unwrap_or("-0");
    let after = parts.next().unwrap_or("+0");
    (
        parse_hunk_range_start(before),
        parse_hunk_range_start(after),
    )
}

fn parse_hunk_range_start(segment: &str) -> usize {
    segment
        .trim_start_matches(['-', '+'])
        .split(',')
        .next()
        .unwrap_or("0")
        .parse::<usize>()
        .unwrap_or(0)
}

fn visible_diff_line_number(line: usize) -> Option<usize> {
    (line > 0).then_some(line)
}

fn parse_unified_diff_files(
    diff_content: &str,
    fallback_path: Option<&str>,
) -> Option<Vec<ParsedPatchFile>> {
    let lines = diff_content
        .lines()
        .map(normalize_diff_line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    let mut files = Vec::new();
    let mut cursor = 0usize;

    while cursor < lines.len() {
        if !lines[cursor].starts_with("--- ") {
            cursor += 1;
            continue;
        }
        let before_label = lines[cursor].trim_start_matches("--- ").to_string();
        cursor += 1;
        if cursor >= lines.len() || !lines[cursor].starts_with("+++ ") {
            return None;
        }
        let after_label = lines[cursor].trim_start_matches("+++ ").to_string();
        let display_path = fallback_path
            .map(str::to_string)
            .or_else(|| normalize_patch_label(&after_label))
            .or_else(|| normalize_patch_label(&before_label))
            .unwrap_or_else(|| "diff".to_string());
        cursor += 1;

        let mut hunks = Vec::new();
        while cursor < lines.len() && !lines[cursor].starts_with("--- ") {
            if !lines[cursor].starts_with("@@") {
                cursor += 1;
                continue;
            }
            let header = lines[cursor].to_string();
            cursor += 1;
            let mut before_lines = Vec::new();
            let mut after_lines = Vec::new();

            while cursor < lines.len()
                && !lines[cursor].starts_with("@@")
                && !lines[cursor].starts_with("--- ")
            {
                if lines[cursor].starts_with("\\ No newline at end of file") {
                    cursor += 1;
                    continue;
                }
                let (prefix, body) = lines[cursor].split_at(1);
                match prefix {
                    " " => {
                        before_lines.push(body.to_string());
                        after_lines.push(body.to_string());
                    }
                    "-" => before_lines.push(body.to_string()),
                    "+" => after_lines.push(body.to_string()),
                    _ => return None,
                }
                cursor += 1;
            }

            hunks.push(ParsedPatchHunk {
                header,
                before_lines,
                after_lines,
            });
        }

        if !hunks.is_empty() {
            files.push(ParsedPatchFile {
                display_path,
                before_label,
                after_label,
                hunks,
            });
        }
    }

    if files.is_empty() {
        parse_hunk_only_diff(&lines, fallback_path).map(|file| vec![file])
    } else {
        Some(files)
    }
}

fn parse_hunk_only_diff(lines: &[&str], fallback_path: Option<&str>) -> Option<ParsedPatchFile> {
    let mut cursor = 0usize;
    let mut hunks = Vec::new();

    while cursor < lines.len() {
        if !lines[cursor].starts_with("@@") {
            cursor += 1;
            continue;
        }
        let header = lines[cursor].to_string();
        cursor += 1;
        let mut before_lines = Vec::new();
        let mut after_lines = Vec::new();

        while cursor < lines.len() && !lines[cursor].starts_with("@@") {
            if lines[cursor].starts_with("\\ No newline at end of file") {
                cursor += 1;
                continue;
            }
            let (prefix, body) = lines[cursor].split_at(1);
            match prefix {
                " " => {
                    before_lines.push(body.to_string());
                    after_lines.push(body.to_string());
                }
                "-" => before_lines.push(body.to_string()),
                "+" => after_lines.push(body.to_string()),
                _ => return None,
            }
            cursor += 1;
        }

        hunks.push(ParsedPatchHunk {
            header,
            before_lines,
            after_lines,
        });
    }

    (!hunks.is_empty()).then(|| ParsedPatchFile {
        display_path: fallback_path.unwrap_or("diff").to_string(),
        before_label: fallback_path.unwrap_or("diff").to_string(),
        after_label: fallback_path.unwrap_or("diff").to_string(),
        hunks,
    })
}

fn normalize_patch_label(label: &str) -> Option<String> {
    let trimmed = label
        .split_whitespace()
        .next()
        .unwrap_or(label)
        .trim_start_matches("a/")
        .trim_start_matches("b/");
    (!trimmed.is_empty() && trimmed != "/dev/null").then(|| trimmed.to_string())
}

fn normalize_diff_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
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
    fn structured_diff_rows_respect_display_width_for_wide_glyphs() {
        let diff = "--- demo.txt\n+++ demo.txt\n@@ -1,2 +1,2 @@\n-漢字🙂漢字🙂漢字🙂\n+🙂漢字🙂漢字🙂漢字\n";
        let lines = render_structured_diff_lines(diff, None, "", 24, false, &Theme::default())
            .expect("wide glyph diff lines");

        assert!(
            lines.iter().all(|line| line.width() <= 24),
            "rendered diff rows should honor visible width: {:#?}",
            lines
                .iter()
                .map(|line| line_to_plain_text(line.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn stacked_diff_text_spans_keep_row_backgrounds() {
        let diff = "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n";
        let theme = Theme::default();
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            80,
            StructuredDiffRenderOptions {
                force_stacked: true,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &theme,
        )
        .expect("stacked diff lines");

        let context_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("alpha"))
            .expect("context row");
        let removed_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("beta"))
            .expect("removed row");
        let added_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("BETA"))
            .expect("added row");

        let context_span = context_line
            .spans
            .iter()
            .find(|span| span.content.contains("alpha"))
            .expect("context text span");
        let removed_span = removed_line
            .spans
            .iter()
            .find(|span| span.content.contains("beta"))
            .expect("removed text span");
        let added_span = added_line
            .spans
            .iter()
            .find(|span| span.content.contains("BETA"))
            .expect("added text span");

        assert_eq!(context_span.style.bg, Some(theme.surface.panel));
        assert_eq!(
            removed_span.style.bg,
            Some(diff_row_palette('-', &theme).content_bg)
        );
        assert_eq!(
            added_span.style.bg,
            Some(diff_row_palette('+', &theme).content_bg)
        );
    }

    #[test]
    fn structured_diff_palette_matches_opencode_inline_diff_colors() {
        let theme = Theme::default();

        assert_eq!(
            diff_row_palette('+', &theme).content_bg,
            opencode_diff_added_bg()
        );
        assert_eq!(
            diff_row_palette('+', &theme).gutter_bg,
            opencode_diff_added_line_number_bg()
        );
        assert_eq!(
            diff_row_palette('-', &theme).content_bg,
            opencode_diff_removed_bg()
        );
        assert_eq!(
            diff_row_palette('-', &theme).gutter_bg,
            opencode_diff_removed_line_number_bg()
        );
        assert_eq!(diff_hunk_palette(&theme).content_bg, theme.surface.panel);
        assert_eq!(
            diff_marker_style('+', None, &theme).fg,
            Some(opencode_diff_highlight_added())
        );
        assert_eq!(
            diff_marker_style('-', None, &theme).fg,
            Some(opencode_diff_highlight_removed())
        );
        assert_eq!(
            diff_segment_style(
                DiffSegmentKind::Added,
                DiffSegmentKind::Removed,
                None,
                &theme
            )
            .fg,
            Some(opencode_diff_highlight_added())
        );
        assert_eq!(
            diff_segment_style(
                DiffSegmentKind::Removed,
                DiffSegmentKind::Added,
                None,
                &theme
            )
            .fg,
            Some(opencode_diff_highlight_removed())
        );

        let hunk_header = render_diff_hunk_header("", "@@ -1,1 +1,1 @@", 48, 2, &theme);
        let hunk_span = hunk_header
            .spans
            .iter()
            .find(|span| span.content.contains("@@ -1,1 +1,1 @@"))
            .expect("hunk header span");
        assert_eq!(hunk_span.style.fg, Some(opencode_diff_hunk_header()));
        assert_eq!(hunk_span.style.bg, Some(theme.surface.panel));
    }

    #[test]
    fn structured_diff_syntax_highlighting_uses_opencode_token_colors() {
        let chunks = highlight_diff_line_chunks(
            Some("src/demo.rs"),
            "let value = \"hi\"; let total = 42; // note",
            Some(opencode_diff_added_bg()),
        )
        .expect("syntax-highlighted diff chunks");

        let find_chunk = |needle: &str| {
            chunks
                .iter()
                .find(|chunk| chunk.text.contains(needle))
                .unwrap_or_else(|| panic!("missing chunk containing {needle:?}: {chunks:#?}"))
        };

        assert_eq!(
            find_chunk("hi").style.fg,
            Some(Color::Rgb(0x7F, 0xD8, 0x8F))
        );
        assert_eq!(
            find_chunk("42").style.fg,
            Some(Color::Rgb(0xF5, 0xA7, 0x42))
        );
        assert_eq!(
            find_chunk("note").style.fg,
            Some(Color::Rgb(0x80, 0x80, 0x80))
        );
        assert_eq!(find_chunk("note").style.bg, Some(opencode_diff_added_bg()));
    }

    #[test]
    fn structured_diff_headers_surface_rename_paths() {
        let diff = "--- src/old_name.rs\n+++ src/new_name.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            96,
            StructuredDiffRenderOptions {
                force_stacked: true,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &Theme::default(),
        )
        .expect("rename diff lines");
        let header = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .find(|line| line.contains("old_name.rs") && line.contains("new_name.rs"))
            .expect("rename header");

        assert!(
            header.contains("→"),
            "header should surface rename arrow: {header}"
        );
    }

    #[test]
    fn stacked_diff_long_rows_wrap_instead_of_truncating() {
        let diff = "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            84,
            StructuredDiffRenderOptions {
                force_stacked: true,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &Theme::default(),
        )
        .expect("wrapped stacked diff lines");
        let rendered = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .collect::<Vec<_>>();
        let collect_stacked_cell_text = |rows: &[String], marker: char| {
            let marker_token = format!("{marker} ");
            let start = rows
                .iter()
                .position(|line| line.contains(&marker_token))
                .unwrap_or_else(|| panic!("missing {marker} row marker: {rows:#?}"));
            let marker_column = rows[start]
                .find(&marker_token)
                .unwrap_or_else(|| panic!("missing {marker} marker column: {rows:#?}"));
            let text_column = marker_column + marker_token.len();
            let mut chunks = Vec::new();

            for line in &rows[start..] {
                let marker_cell = line.get(marker_column..text_column).unwrap_or("");
                let Some(text) = line.get(text_column..) else {
                    break;
                };
                let text = text.trim_end();

                if marker_cell == marker_token {
                    chunks.push(text.to_string());
                    continue;
                }

                if marker_cell == "  " && !text.is_empty() {
                    chunks.push(text.to_string());
                    continue;
                }

                break;
            }

            chunks.concat()
        };
        let removed_text = collect_stacked_cell_text(&rendered, '-');
        let added_text = collect_stacked_cell_text(&rendered, '+');

        assert!(
            removed_text
                == "session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows",
            "removed continuation should preserve the full text across wrapped rows: {rendered:#?}"
        );
        assert!(
            added_text
                == "session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells",
            "added continuation should preserve the full text across wrapped rows: {rendered:#?}"
        );
        assert!(
            rendered.iter().all(|line| !line.contains('…')),
            "stacked renderer should keep the full text without ellipsis: {rendered:#?}"
        );
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
