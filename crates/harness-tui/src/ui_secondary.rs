use imara_diff::{Algorithm, Diff, InternedInput};
use std::cmp::max;

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
    pinned_summary: OperatorRailPinnedSummary,
    body: OperatorRailBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailPinnedSummary {
    run_identity: String,
    profile_provider_model: String,
    state_label: String,
    active_todo_count: usize,
    modified_file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorRailBody {
    sections: Vec<OperatorRailBodySection>,
    empty_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OperatorRailBodySection {
    Todo { count: usize, items: Vec<String> },
    ModifiedFiles { count: usize, items: Vec<String> },
}

impl OperatorRailBodySection {
    fn heading(&self) -> String {
        match self {
            Self::Todo { count, .. } => format!("Todo · {count}"),
            Self::ModifiedFiles { count, .. } => format!("Modified Files · {count}"),
        }
    }

    fn items(&self) -> &[String] {
        match self {
            Self::Todo { items, .. } | Self::ModifiedFiles { items, .. } => items,
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

pub(super) fn render_diff_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let body = render_secondary_surface_shell(frame, area, theme, diff_summary_line(app, theme));
    let [event_list_area, diff_area] = split_secondary_surface(
        body,
        crate::layout::REVIEW_SURFACE_SPLIT_PERCENT,
        theme.live_shell.rhythm.surface_gap,
    );

    render_event_list(frame, app, event_list_area, theme);

    let is_focused = app.focus == Focus::Details;
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, "Diff", is_focused, surface);

    let content = if let Some(path) = &app.session_path {
        if let Some((event, _)) = diff_surface_event(app) {
            if let Some(diff_content) = load_diff_for_event(path, event) {
                diff_content
            } else if let Some(diff_path) = diff_artifact_path(path, event) {
                format!("diff artifact missing:\n{}", diff_path.display())
            } else {
                "Select an edit event to view diff".to_string()
            }
        } else {
            "Select an edit event to view diff".to_string()
        }
    } else {
        "No session loaded".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, diff_area);
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
    let app = operator_rail_test_app();
    let model = build_operator_rail_model(&app);

    assert_eq!(model.pinned_summary.run_identity, "run run_fixture");
    assert_eq!(
        model.pinned_summary.profile_provider_model,
        "worker/mock/model-1"
    );
    assert_eq!(model.pinned_summary.state_label, "Demo");
    assert_eq!(model.pinned_summary.active_todo_count, 1);
    assert_eq!(model.pinned_summary.modified_file_count, 1);
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order() {
    let populated_model = build_operator_rail_model(&operator_rail_test_app());

    assert_eq!(populated_model.body.empty_state, None);
    assert_eq!(
        populated_model
            .body
            .sections
            .iter()
            .map(OperatorRailBodySection::heading)
            .collect::<Vec<_>>(),
        vec!["Todo · 1".to_string(), "Modified Files · 1".to_string()]
    );

    let empty_model = build_operator_rail_model(&AppState::new_live(None, false, None));
    assert!(empty_model.body.sections.is_empty());
    assert_eq!(
        empty_model.body.empty_state.as_deref(),
        Some("No operator activity yet")
    );
}

#[cfg(test)]
fn operator_rail_test_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(None, false, None);
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
            diff_rel_path: None,
            diff_digest: None,
        }),
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
pub(crate) fn structured_diff_text_for_test(
    diff_content: &str,
    fallback_path: &str,
    width: u16,
) -> Vec<String> {
    render_structured_diff_lines(
        diff_content,
        Some(fallback_path),
        "",
        width,
        &Theme::default(),
    )
    .unwrap_or_else(|| vec![Line::from(diff_content.to_string())])
    .into_iter()
    .map(line_to_plain_text)
    .collect()
}

fn help_row(app: &AppState, action: Action, label: &str) -> String {
    format!("  {:<12} {label}", app.keymap.get_binding_str(action))
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
            help_row(app, Action::InsertNewline, "Insert newline"),
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
        OperatorSidebarChrome::Persistent => ui_chrome::chromeless_shell_surface(theme),
        OperatorSidebarChrome::Overlay => ui_chrome::divided_shell_surface(theme),
    };
    let block = match chrome {
        OperatorSidebarChrome::Persistent => {
            ui_chrome::subtle_divider_block(theme, Borders::LEFT, surface)
        }
        OperatorSidebarChrome::Overlay => {
            ui_chrome::secondary_pane_block(theme, Line::default(), is_focused, surface)
        }
    };
    let inner = block.inner(area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let rail = build_operator_rail_model(app);
    let summary_text = build_operator_rail_summary_text(&rail.pinned_summary, theme);
    let body_text = build_operator_rail_body_text(&rail.body, theme);
    let compact_mode = rail.body.sections.is_empty();
    let summary_height = summary_text.lines.len().min(usize::from(u16::MAX)) as u16;
    let body_height = body_text.lines.len().min(usize::from(u16::MAX)) as u16;

    if compact_mode {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(summary_height),
                Constraint::Length(body_height),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(inner);
        let summary_area = sections[0];
        let body_area = sections[1];
        let footer_area = sections[3];

        frame.render_widget(
            Paragraph::new(summary_text).style(panel_style(surface, theme.text.primary)),
            summary_area,
        );
        frame.render_widget(
            Paragraph::new(body_text)
                .style(panel_style(surface, theme.text.primary))
                .wrap(Wrap { trim: true }),
            body_area,
        );
        frame.render_widget(
            Paragraph::new(operator_sidebar_footer_line())
                .style(panel_style(surface, theme.text.tertiary))
                .wrap(Wrap { trim: true }),
            footer_area,
        );
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(summary_height),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);
    let summary_area = sections[0];
    let body_area = sections[1];
    let footer_area = sections[2];

    frame.render_widget(
        Paragraph::new(summary_text).style(panel_style(surface, theme.text.primary)),
        summary_area,
    );
    frame.render_widget(
        Paragraph::new(body_text)
            .style(panel_style(surface, theme.text.primary))
            .scroll((app.details_scroll, 0))
            .wrap(Wrap { trim: true }),
        body_area,
    );
    frame.render_widget(
        Paragraph::new(operator_sidebar_footer_line())
            .style(panel_style(surface, theme.text.tertiary))
            .wrap(Wrap { trim: true }),
        footer_area,
    );
}

#[allow(dead_code)]
fn build_operator_sidebar_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let rail = build_operator_rail_model(app);
    let mut lines = build_operator_rail_summary_text(&rail.pinned_summary, theme).lines;
    append_operator_rail_body_lines(&mut lines, &rail.body, theme);

    Text::from(lines)
}

fn build_operator_rail_summary_text(
    summary: &OperatorRailPinnedSummary,
    theme: &Theme,
) -> Text<'static> {
    let mut lines = build_operator_rail_summary_lines(summary, theme);
    lines.push(Line::from(""));
    Text::from(lines)
}

fn build_operator_rail_body_text(body: &OperatorRailBody, theme: &Theme) -> Text<'static> {
    let mut lines = Vec::new();
    append_operator_rail_body_lines(&mut lines, body, theme);
    Text::from(lines)
}

fn build_operator_rail_model(app: &AppState) -> OperatorRailModel {
    let todo_lines = app.operator_sidebar_todo_lines();
    let modified_files = app.operator_sidebar_modified_files();

    let mut sections = Vec::new();
    if !todo_lines.is_empty() {
        sections.push(OperatorRailBodySection::Todo {
            count: todo_lines.len(),
            items: todo_lines.clone(),
        });
    }
    if !modified_files.is_empty() {
        sections.push(OperatorRailBodySection::ModifiedFiles {
            count: modified_files.len(),
            items: modified_files.clone(),
        });
    }

    let empty_state = sections
        .is_empty()
        .then(|| "No operator activity yet".to_string());

    OperatorRailModel {
        pinned_summary: OperatorRailPinnedSummary {
            run_identity: app.operator_sidebar_run_identity(),
            profile_provider_model: format!(
                "{}/{}/{}",
                app.active_profile(),
                app.active_provider(),
                app.current_model_label()
            ),
            state_label: app.operator_sidebar_state_label(),
            active_todo_count: todo_lines.len(),
            modified_file_count: modified_files.len(),
        },
        body: OperatorRailBody {
            sections,
            empty_state,
        },
    }
}

fn build_operator_rail_summary_lines(
    summary: &OperatorRailPinnedSummary,
    theme: &Theme,
) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            format!("{} · {}", summary.state_label, summary.run_identity),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            summary.profile_provider_model.clone(),
            Style::default().fg(theme.text.secondary),
        )),
        Line::from(Span::styled(
            format!(
                "{} active todo{} · {} modified file{}",
                summary.active_todo_count,
                if summary.active_todo_count == 1 {
                    ""
                } else {
                    "s"
                },
                summary.modified_file_count,
                if summary.modified_file_count == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            muted_meta_style(theme),
        )),
    ]
}

fn append_operator_rail_body_lines(
    lines: &mut Vec<Line<'static>>,
    body: &OperatorRailBody,
    theme: &Theme,
) {
    if let Some(empty_state) = &body.empty_state {
        lines.push(Line::from(Span::styled(
            empty_state.clone(),
            muted_meta_style(theme),
        )));
        return;
    }

    for (index, section) in body.sections.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        append_operator_rail_section(lines, theme, section);
    }
}

fn append_operator_rail_section(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    section: &OperatorRailBodySection,
) {
    lines.push(Line::from(Span::styled(
        section.heading(),
        Style::default()
            .fg(theme.text.secondary)
            .add_modifier(Modifier::BOLD),
    )));

    for item in section.items() {
        append_text_block(lines, item, theme.text.primary, "  ");
    }
}
fn operator_sidebar_footer_line() -> String {
    if harness_core::clock::Determinism::enabled(false) {
        return format!("workspace · v{}", env!("CARGO_PKG_VERSION"));
    }

    let folder = std::env::current_dir()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ".".to_string());
    format!("{folder} · v{}", env!("CARGO_PKG_VERSION"))
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
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let model = structured_diff_model_from_patch(diff_content, fallback_path)?;
    Some(render_structured_diff_model(&model, prefix, width, theme))
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
    theme: &Theme,
) -> Vec<Line<'static>> {
    let prefix_width = prefix.chars().count();
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wide = content_width >= usize::from(DIFF_SIDE_BY_SIDE_MIN_WIDTH);
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

fn diff_surface_event(app: &AppState) -> Option<(&harness_core::event::EventEnvelopeV1, bool)> {
    app.selected_event()
        .filter(|event| event_has_diff_artifact(event))
        .map(|event| (event, false))
        .or_else(|| {
            app.events
                .iter()
                .rev()
                .find(|event| event_has_diff_artifact(event))
                .map(|event| (event, true))
        })
}

fn event_has_diff_artifact(event: &harness_core::event::EventEnvelopeV1) -> bool {
    use harness_core::event::EventV1;

    matches!(
        &event.payload,
        EventV1::EditApplied(edit)
            if edit
                .diff_rel_path
                .as_ref()
                .is_some_and(|path| !path.trim().is_empty())
    )
}

fn diff_artifact_path(
    session_path: &std::path::Path,
    event: &harness_core::event::EventEnvelopeV1,
) -> Option<std::path::PathBuf> {
    use harness_core::event::EventV1;

    match &event.payload {
        EventV1::EditApplied(edit) => edit
            .diff_rel_path
            .as_ref()
            .filter(|path| !path.trim().is_empty())
            .map(|path| session_path.join(path)),
        _ => None,
    }
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

fn load_diff_for_event(
    session_path: &std::path::Path,
    event: &harness_core::event::EventEnvelopeV1,
) -> Option<String> {
    diff_artifact_path(session_path, event).and_then(|path| std::fs::read_to_string(path).ok())
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

fn diff_summary_line(app: &AppState, theme: &Theme) -> Line<'static> {
    let detail = if let Some((event, fallback_to_latest_edit)) = diff_surface_event(app) {
        if fallback_to_latest_edit {
            format!("artifact view · latest edit seq {}", event.seq)
        } else {
            format!("artifact view · seq {}", event.seq)
        }
    } else {
        "artifact view · select an edit event".to_string()
    };
    secondary_summary_line(app, "diff", theme.status.info, detail, theme)
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
