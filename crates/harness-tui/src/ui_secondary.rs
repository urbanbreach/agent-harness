use imara_diff::{Algorithm, Diff, InternedInput};
use std::cmp::max;
use std::collections::BTreeMap;
use std::path::Path;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::*;

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
    additions: usize,
    removals: usize,
    rows: Vec<StructuredDiffDisplayRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StructuredDiffDisplayRow {
    FileHeader(String),
    HunkHeader(String),
    Context(String),
    Changed {
        before: Option<DiffCell>,
        after: Option<DiffCell>,
    },
    Spacer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffCell {
    marker: char,
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
        state: RuntimeHealthState,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeHealthState {
    Healthy,
    Unhealthy,
}

#[cfg(test)]
pub(crate) fn operator_sidebar_config_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl OperatorRailItem {
    fn text(&self) -> &str {
        match self {
            Self::Plain(text) => text,
            Self::Status { label, .. } => label,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailBodySection {
    Context { items: Vec<String> },
    Mcp { items: Vec<OperatorRailItem> },
    Lsp { items: Vec<OperatorRailItem> },
    ModifiedFiles { items: Vec<String>, collapsed: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorRailBodyPresentation {
    Regular,
}

impl OperatorRailBodySection {
    fn heading(&self) -> String {
        match self {
            Self::Context { .. } => "Context".to_string(),
            Self::Mcp { .. } => "MCP".to_string(),
            Self::Lsp { .. } => "LSP".to_string(),
            Self::ModifiedFiles { collapsed, .. } => {
                if *collapsed {
                    "▶ Modified Files".to_string()
                } else {
                    "▼ Modified Files".to_string()
                }
            }
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

    #[cfg(test)]
    fn item_texts(&self) -> Vec<String> {
        match self {
            Self::Context { items } | Self::ModifiedFiles { items, .. } => items.clone(),
            Self::Mcp { items } | Self::Lsp { items } => {
                items.iter().map(|item| item.text().to_string()).collect()
            }
        }
    }
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
    assert_eq!(model.body.sections[1].heading(), "MCP");
    assert_eq!(model.body.sections[2].heading(), "LSP");
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
        modified_files.as_slice(),
        ["src/ui_secondary.rs Modified Files · 99".to_string()]
    );
    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Modified Files"));
    assert!(sidebar.contains("src/ui_secondary.rs Modified Files · 99"));
    assert!(!sidebar.contains("rows.diff\nLSP"));
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
            "MCP".to_string(),
            "LSP".to_string(),
            "▼ Modified Files".to_string(),
        ]
    );
    assert_eq!(
        empty_model.body.sections[0].item_texts(),
        ["0 tokens".to_string()]
    );
}

#[cfg(test)]
#[allow(dead_code)]
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
    assert_eq!(model.body.sections[1].heading(), "MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.text().to_string())
            .collect::<Vec<_>>(),
        ["websearch Connected".to_string()]
    );
    assert_eq!(
        operator_sidebar_lsp_items(&app)
            .into_iter()
            .map(|item| item.text().to_string())
            .collect::<Vec<_>>(),
        ["rust".to_string()]
    );

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("\n  ● websearch Connected"));
    assert!(sidebar.contains("\n  ● rust"));
    assert!(!sidebar.contains("▼ MCP"));
    assert!(!sidebar.contains("▼ LSP"));
    assert!(!sidebar.contains("\n• ● websearch Connected"));
    assert!(!sidebar.contains("\n• ● rust"));
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
            "MCP".to_string(),
            "LSP".to_string(),
            "▼ Modified Files".to_string(),
        ]
    );
    let mcp_items = model.body.sections[1].item_texts();
    assert!(
        matches!(
            mcp_items.as_slice(),
            [item]
                if item == "websearch Disconnected"
                    || item == "No MCP integrations configured"
        ),
        "unexpected MCP items: {mcp_items:?}"
    );

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(
        sidebar.contains("● websearch Disconnected")
            || sidebar.contains("No MCP integrations configured")
    );
    assert!(!sidebar.contains("edit.hashline_apply"));
    assert!(!sidebar.contains("hashline_apply"));
}

#[cfg(test)]
#[allow(dead_code)]
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
        assert!(rendered.contains("MCP"), "missing MCP section\n{rendered}");
        assert!(rendered.contains("LSP"), "missing LSP section\n{rendered}");
        assert!(
            rendered.contains("▼ Modified Files"),
            "missing modified files section\n{rendered}"
        );
        assert!(
            rendered.contains("  demo.txt"),
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
            .map(|item| item.text().to_string())
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

    assert_eq!(model.body.sections[1].heading(), "MCP");
    assert_eq!(
        operator_sidebar_mcp_items(&app)
            .into_iter()
            .map(|item| item.text().to_string())
            .collect::<Vec<_>>(),
        [
            "fixture Connected".to_string(),
            "websearch Disconnected".to_string(),
        ]
    );
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
#[allow(dead_code)]
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
#[allow(dead_code)]
fn operator_rail_tool_activity_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    let request_id = "req_operator_rail_tools";
    let system = harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None);

    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect operator rail taxonomy".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_search_web".to_string(),
                tool_id: "search.web".to_string(),
                args_summary: serde_json::json!({"query": "operator rail taxonomy"}).to_string(),
                args_digest: "digest-search-web".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_search_web".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_search_web".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Fetched operator rail examples".to_string()),
                output_digest: Some("digest-search-web-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        5,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_search_code".to_string(),
                tool_id: "search.code".to_string(),
                args_summary: serde_json::json!({"query": "operator rail section"}).to_string(),
                args_digest: "digest-search-code".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.code".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        6,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_search_code".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        7,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_web_fetch".to_string(),
                tool_id: "web.fetch".to_string(),
                args_summary: serde_json::json!({"url": "https://example.com"}).to_string(),
                args_digest: "digest-web-fetch".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("web.fetch".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        8,
        system.clone(),
        request_id,
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_web_fetch".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        9,
        system,
        request_id,
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_web_fetch".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Fetched https://example.com".to_string()),
                output_digest: Some("digest-web-fetch-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("web.fetch".to_string()),
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
#[allow(dead_code)]
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
        help_row(app, Action::DismissModal, "Dismiss modal"),
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
    let body_text = build_operator_rail_body_text(&rail.body, theme, inner.width);
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
        Paragraph::new(body_text)
            .style(panel_style(surface, theme.text.primary))
            .scroll((app.details_scroll, 0))
            .wrap(Wrap { trim: true }),
        body_area,
    );
}

#[allow(dead_code)]
fn build_operator_sidebar_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let rail = build_operator_rail_model(app);
    let mut lines = build_operator_rail_title_text(rail.title.as_deref(), theme, 80).lines;
    append_operator_rail_body_lines(
        &mut lines,
        &rail.body,
        theme,
        80,
        OperatorRailBodyPresentation::Regular,
    );

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

fn build_operator_rail_body_text(
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
) -> Text<'static> {
    let mut lines = Vec::new();
    append_operator_rail_body_lines(
        &mut lines,
        body,
        theme,
        width,
        OperatorRailBodyPresentation::Regular,
    );
    Text::from(lines)
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
                },
                OperatorRailBodySection::Lsp {
                    items: operator_sidebar_lsp_items(app),
                },
                OperatorRailBodySection::ModifiedFiles {
                    items: operator_sidebar_modified_file_items(app),
                    collapsed: app.operator_sidebar_modified_files_collapsed(),
                },
            ],
        },
    }
}

fn append_operator_rail_body_lines(
    lines: &mut Vec<Line<'static>>,
    body: &OperatorRailBody,
    theme: &Theme,
    width: u16,
    presentation: OperatorRailBodyPresentation,
) {
    match presentation {
        OperatorRailBodyPresentation::Regular => {
            for section in &body.sections {
                append_operator_rail_section(lines, theme, section, width);
            }
        }
    }
}

fn append_operator_rail_section(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    section: &OperatorRailBodySection,
    width: u16,
) {
    lines.push(Line::from(Span::styled(
        truncate_plain_text(&section.heading(), usize::from(width)),
        section.heading_style(theme),
    )));

    let items = match section {
        OperatorRailBodySection::Context { items } => items
            .iter()
            .cloned()
            .map(OperatorRailItem::Plain)
            .collect::<Vec<_>>(),
        OperatorRailBodySection::Mcp { items } | OperatorRailBodySection::Lsp { items } => {
            items.clone()
        }
        OperatorRailBodySection::ModifiedFiles { items, collapsed } => {
            if *collapsed {
                Vec::new()
            } else {
                items
                    .iter()
                    .cloned()
                    .map(OperatorRailItem::Plain)
                    .collect::<Vec<_>>()
            }
        }
    };

    for item in &items {
        append_operator_rail_item(lines, theme, section, item);
    }
}

fn append_operator_rail_item(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    section: &OperatorRailBodySection,
    item: &OperatorRailItem,
) {
    let line_prefix = "  ";
    let continuation_prefix = match item {
        OperatorRailItem::Plain(_) => line_prefix,
        OperatorRailItem::Status { .. } => "    ",
    };
    let mut raw_lines = item.text().lines();
    let Some(first_line) = raw_lines.next() else {
        return;
    };

    let primary_color = if matches!(section, OperatorRailBodySection::Context { .. }) {
        theme.text.secondary
    } else {
        theme.text.primary
    };
    let secondary_color = theme.text.secondary;

    match item {
        OperatorRailItem::Plain(_) => lines.push(Line::from(Span::styled(
            format!("{line_prefix}{first_line}"),
            Style::default().fg(primary_color),
        ))),
        OperatorRailItem::Status { state, .. } => {
            let dot_color = match state {
                RuntimeHealthState::Healthy => theme.status.success,
                RuntimeHealthState::Unhealthy => theme.status.error,
            };
            lines.push(Line::from(vec![
                Span::styled(line_prefix.to_string(), Style::default().fg(primary_color)),
                Span::styled("● ".to_string(), Style::default().fg(dot_color)),
                Span::styled(first_line.to_string(), Style::default().fg(primary_color)),
            ]));
        }
    }

    for line in raw_lines {
        lines.push(Line::from(Span::styled(
            format!("{continuation_prefix}{line}"),
            Style::default().fg(secondary_color),
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

fn operator_sidebar_total_tokens(app: &AppState) -> u64 {
    app.activities
        .iter()
        .filter_map(|activity| activity.usage)
        .map(|usage| u64::from(usage.total_tokens))
        .sum()
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
    let total_tokens = operator_sidebar_total_tokens(app);
    let mut items = vec![format!("{} tokens", format_token_count(total_tokens))];
    if let Some(context_window_tokens) =
        operator_sidebar_context_window_tokens(app).filter(|value| *value > 0)
    {
        let percent = ((total_tokens as f64 / f64::from(context_window_tokens)) * 100.0).round();
        items.push(format!("{}% used", percent.clamp(0.0, 999.0) as u64));
    }
    items
}

fn operator_sidebar_mcp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let Some(integrations) = harness_core::config::registered_integrations_config() else {
        return vec![OperatorRailItem::Plain(
            "No MCP integrations configured".to_string(),
        )];
    };

    let mut items = BTreeMap::new();
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

    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            let Some(server_name) = runtime_mcp_server_name(tool_call) else {
                continue;
            };
            let Some(state) = runtime_health_state_for_tool_call(tool_call) else {
                continue;
            };
            if let Some(entry) = items.get_mut(&server_name) {
                *entry = state;
            }
        }
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain(
            "No MCP integrations configured".to_string(),
        )]
    } else {
        items
            .into_iter()
            .map(|(name, state)| OperatorRailItem::Status {
                label: format!(
                    "{name} {}",
                    match state {
                        RuntimeHealthState::Healthy => "Connected",
                        RuntimeHealthState::Unhealthy => "Disconnected",
                    }
                ),
                state,
            })
            .collect()
    }
}

fn operator_sidebar_lsp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let config = harness_core::config::registered_lsp_config();
    if config.disabled {
        return vec![OperatorRailItem::Plain("LSP disabled".to_string())];
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
            .map(|(name, state)| OperatorRailItem::Status { label: name, state })
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
        "code.lsp" | "code.lsp.rename" => {
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

fn operator_sidebar_modified_file_items(app: &AppState) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();

    for event in app.events.iter().rev() {
        let harness_core::event::EventV1::EditApplied(edit) = &event.payload else {
            continue;
        };
        if !seen.insert(edit.path.clone()) {
            continue;
        }

        let display_path = sanitize_operator_sidebar_line(&edit.path);
        let counts = edit.diff_rel_path.as_deref().and_then(|diff_rel_path| {
            app.session_path.as_ref().and_then(|session_path| {
                std::fs::read_to_string(session_path.join(diff_rel_path))
                    .ok()
                    .and_then(|diff_content| {
                        structured_diff_model_from_patch(&diff_content, Some(&edit.path)).map(
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

        items.push(match counts {
            Some((additions, removals)) => format!("{display_path} · +{additions} -{removals}"),
            None => display_path,
        });
    }

    if items.is_empty() {
        vec!["No modified files".to_string()]
    } else {
        items
    }
}

fn activity_surface_visible(app: &AppState) -> bool {
    (app.replay_mode && app.active_tab == Tab::Run)
        || (!app.replay_mode && app.review_surface().is_none())
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn orchestration_warning_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let warning = app.orchestration_latest_warning().unwrap_or("none");
    let text = format!("watch · {warning}");
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        Style::default().fg(theme.status.warning),
    ))
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn orchestration_overflow_line(hidden_count: usize, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("+{hidden_count} more"),
        Style::default()
            .fg(theme.text.tertiary)
            .add_modifier(Modifier::BOLD),
    ))
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

pub(super) fn render_structured_diff_lines(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let model = structured_diff_model_from_patch(diff_content, fallback_path)?;
    Some(render_structured_diff_model(
        &model,
        prefix,
        width,
        force_stacked,
        theme,
    ))
}

fn structured_diff_model_from_patch(
    diff_content: &str,
    fallback_path: Option<&str>,
) -> Option<StructuredDiffModel> {
    let files = parse_unified_diff_files(diff_content, fallback_path)?;
    Some(StructuredDiffModel {
        files: files.into_iter().map(build_structured_diff_file).collect(),
    })
}

fn build_structured_diff_file(file: ParsedPatchFile) -> StructuredDiffFile {
    let mut rows = Vec::new();
    let mut additions = 0;
    let mut removals = 0;

    for (index, hunk) in file.hunks.into_iter().enumerate() {
        if index == 0 {
            rows.push(StructuredDiffDisplayRow::FileHeader(
                file.display_path.clone(),
            ));
        } else {
            rows.push(StructuredDiffDisplayRow::Spacer);
        }

        let aligned = align_patch_hunk(&hunk);
        additions += aligned.additions;
        removals += aligned.removals;
        rows.extend(aligned.rows);
    }

    if let Some(StructuredDiffDisplayRow::FileHeader(header)) = rows.first_mut() {
        *header = format!("{} · +{} -{}", file.display_path, additions, removals);
    }

    StructuredDiffFile {
        display_path: file.display_path,
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

fn align_patch_hunk(hunk: &ParsedPatchHunk) -> AlignedHunk {
    let before_text = hunk.before_lines.join("\n");
    let after_text = hunk.after_lines.join("\n");
    let input = InternedInput::new(
        imara_diff::sources::lines(&before_text),
        imara_diff::sources::lines(&after_text),
    );
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let mut rows = vec![StructuredDiffDisplayRow::HunkHeader(hunk.header.clone())];
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
            rows.push(StructuredDiffDisplayRow::Context(line));
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
                    rows.push(StructuredDiffDisplayRow::Context(before.clone()));
                }
                (Some(before), Some(after)) => {
                    let (before_segments, after_segments) = word_diff_segments(before, after);
                    rows.push(StructuredDiffDisplayRow::Changed {
                        before: Some(DiffCell {
                            marker: '-',
                            text: before.clone(),
                            segments: before_segments,
                        }),
                        after: Some(DiffCell {
                            marker: '+',
                            text: after.clone(),
                            segments: after_segments,
                        }),
                    });
                }
                (Some(before), None) => rows.push(StructuredDiffDisplayRow::Changed {
                    before: Some(DiffCell {
                        marker: '-',
                        text: before.clone(),
                        segments: vec![DiffSegment {
                            kind: DiffSegmentKind::Removed,
                            text: before.clone(),
                        }],
                    }),
                    after: None,
                }),
                (None, Some(after)) => rows.push(StructuredDiffDisplayRow::Changed {
                    before: None,
                    after: Some(DiffCell {
                        marker: '+',
                        text: after.clone(),
                        segments: vec![DiffSegment {
                            kind: DiffSegmentKind::Added,
                            text: after.clone(),
                        }],
                    }),
                }),
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
        rows.push(StructuredDiffDisplayRow::Context(line));
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

fn render_structured_diff_model(
    model: &StructuredDiffModel,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let prefix_width = prefix.chars().count();
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wide = !force_stacked && content_width >= usize::from(DIFF_SIDE_BY_SIDE_MIN_WIDTH);
    let mut lines = Vec::new();

    for (file_index, file) in model.files.iter().enumerate() {
        if file_index > 0 {
            lines.push(Line::from(""));
        }

        for row in &file.rows {
            match row {
                StructuredDiffDisplayRow::FileHeader(header) => lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
                    Span::styled(
                        header.clone(),
                        Style::default()
                            .fg(theme.text.secondary)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])),
                StructuredDiffDisplayRow::HunkHeader(header) => lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
                    Span::styled(header.clone(), muted_meta_style(theme)),
                ])),
                StructuredDiffDisplayRow::Context(text) => {
                    if wide {
                        lines.push(render_wide_diff_row(
                            prefix,
                            Some(&DiffCell {
                                marker: ' ',
                                text: text.clone(),
                                segments: vec![DiffSegment {
                                    kind: DiffSegmentKind::Unchanged,
                                    text: text.clone(),
                                }],
                            }),
                            Some(&DiffCell {
                                marker: ' ',
                                text: text.clone(),
                                segments: vec![DiffSegment {
                                    kind: DiffSegmentKind::Unchanged,
                                    text: text.clone(),
                                }],
                            }),
                            content_width,
                            theme,
                        ));
                    } else {
                        lines.push(render_stacked_diff_line(prefix, ' ', text, theme));
                    }
                }
                StructuredDiffDisplayRow::Changed { before, after } => {
                    if wide {
                        lines.push(render_wide_diff_row(
                            prefix,
                            before.as_ref(),
                            after.as_ref(),
                            content_width,
                            theme,
                        ));
                    } else {
                        if let Some(before) = before {
                            lines.push(render_stacked_diff_cell(prefix, before, theme));
                        }
                        if let Some(after) = after {
                            lines.push(render_stacked_diff_cell(prefix, after, theme));
                        }
                    }
                }
                StructuredDiffDisplayRow::Spacer => lines.push(Line::from("")),
            }
        }
    }

    lines
}

fn render_wide_diff_row(
    prefix: &str,
    before: Option<&DiffCell>,
    after: Option<&DiffCell>,
    content_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let separator = " │ ";
    let column_width = content_width.saturating_sub(separator.chars().count()) / 2;
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.extend(render_diff_cell(before, column_width, true, theme));
    spans.push(Span::styled(separator.to_string(), muted_meta_style(theme)));
    spans.extend(render_diff_cell(after, column_width, false, theme));
    Line::from(spans)
}

fn render_diff_cell(
    cell: Option<&DiffCell>,
    width: usize,
    is_before: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let Some(cell) = cell else {
        return vec![Span::raw(" ".repeat(width))];
    };

    let marker_width = 2usize;
    let text_width = width.saturating_sub(marker_width);
    let accent_kind = if is_before {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };

    let mut spans = vec![Span::styled(
        format!("{} ", cell.marker),
        diff_marker_style(cell.marker, theme),
    )];
    spans.extend(truncate_diff_segments(
        &cell.segments,
        text_width,
        accent_kind,
        theme,
    ));
    let used_width = spans.iter().map(Span::width).sum::<usize>();
    if used_width < width {
        spans.push(Span::raw(" ".repeat(width - used_width)));
    }
    spans
}

fn truncate_diff_segments(
    segments: &[DiffSegment],
    max_width: usize,
    accent_kind: DiffSegmentKind,
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
        used += text.chars().count();
        rendered.push(Span::styled(
            text,
            diff_segment_style(segment.kind, accent_kind, theme),
        ));
    }

    rendered
}

fn render_stacked_diff_line(
    prefix: &str,
    marker: char,
    text: &str,
    theme: &Theme,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
        Span::styled(format!("{marker} "), diff_marker_style(marker, theme)),
        Span::styled(
            text.to_string(),
            diff_segment_style(
                DiffSegmentKind::Unchanged,
                DiffSegmentKind::Unchanged,
                theme,
            ),
        ),
    ])
}

fn render_stacked_diff_cell(prefix: &str, cell: &DiffCell, theme: &Theme) -> Line<'static> {
    let accent_kind = if cell.marker == '-' {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };
    let mut spans = vec![
        Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
        Span::styled(
            format!("{} ", cell.marker),
            diff_marker_style(cell.marker, theme),
        ),
    ];
    spans.extend(cell.segments.iter().map(|segment| {
        Span::styled(
            segment.text.clone(),
            diff_segment_style(segment.kind, accent_kind, theme),
        )
    }));
    Line::from(spans)
}

fn diff_marker_style(marker: char, theme: &Theme) -> Style {
    match marker {
        '+' => Style::default()
            .fg(theme.status.success)
            .add_modifier(Modifier::BOLD),
        '-' => Style::default()
            .fg(theme.status.error)
            .add_modifier(Modifier::BOLD),
        _ => muted_meta_style(theme),
    }
}

fn diff_segment_style(kind: DiffSegmentKind, accent_kind: DiffSegmentKind, theme: &Theme) -> Style {
    match kind {
        DiffSegmentKind::Unchanged => match accent_kind {
            DiffSegmentKind::Removed => Style::default().fg(theme.text.secondary),
            DiffSegmentKind::Added => Style::default().fg(theme.text.primary),
            DiffSegmentKind::Unchanged => Style::default().fg(theme.text.secondary),
        },
        DiffSegmentKind::Removed => Style::default()
            .fg(theme.status.error)
            .add_modifier(Modifier::BOLD),
        DiffSegmentKind::Added => Style::default()
            .fg(theme.status.success)
            .add_modifier(Modifier::BOLD),
    }
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
#[allow(dead_code)]
fn line_to_plain_text(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
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
