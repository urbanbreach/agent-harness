use std::collections::BTreeMap;
use std::fs;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::app::{AppState, Focus, Tab};
use harness_core::event::{EditAppliedEvent, EventEnvelopeV1, EventV1};

pub fn render_app(frame: &mut Frame, app: &AppState) {
    let header_text = header_text(app);
    let root = if header_text.is_some() {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(frame.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(frame.area())
    };

    let (tabs_area, content_area) = if let Some(text) = header_text {
        render_header(frame, root[0], &text);
        (root[1], root[2])
    } else {
        (root[0], root[1])
    };

    render_tabs(frame, app, tabs_area);

    match app.active_tab {
        Tab::Events => render_events_tab(frame, app, content_area),
        Tab::Output => render_output_tab(frame, app, content_area),
        Tab::Diff => render_diff_tab(frame, app, content_area),
        Tab::Help => render_help_tab(frame, app, content_area),
    }

    render_permission_modal(frame, app);
}

fn header_text(app: &AppState) -> Option<String> {
    let replay_header = if app.replay_mode {
        let session_path = app
            .session_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let run_id = app.run_id().unwrap_or("unknown");
        Some(format!(
            "REPLAY: {session_path} (run={run_id}, r to reload)"
        ))
    } else {
        None
    };

    match (replay_header, app.status_banner.as_ref()) {
        (Some(replay), Some(status)) => Some(format!("{replay}  |  STATUS: {status}")),
        (Some(replay), None) => Some(replay),
        (None, Some(status)) => Some(format!("STATUS: {status}")),
        (None, None) => None,
    }
}

fn render_header(frame: &mut Frame, area: Rect, text: &str) {
    frame.render_widget(Paragraph::new(text), area);
}

fn render_tabs(frame: &mut Frame, app: &AppState, area: Rect) {
    let titles: Vec<Line> = vec!["Events", "Output", "Diff", "Help"]
        .into_iter()
        .map(Line::from)
        .collect();

    let tab_index = match app.active_tab {
        Tab::Events => 0,
        Tab::Output => 1,
        Tab::Diff => 2,
        Tab::Help => 3,
    };

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .select(tab_index)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

pub fn event_variant_name(event: &EventV1) -> &'static str {
    match event {
        EventV1::RunStarted(_) => "RunStarted",
        EventV1::RunFinished(_) => "RunFinished",
        EventV1::RunFailed(_) => "RunFailed",
        EventV1::AgentSpawned(_) => "AgentSpawned",
        EventV1::AgentStopped(_) => "AgentStopped",
        EventV1::TaskScheduled(_) => "TaskScheduled",
        EventV1::TaskCancelled(_) => "TaskCancelled",
        EventV1::TaskCompleted(_) => "TaskCompleted",
        EventV1::TaskResultLate(_) => "TaskResultLate",
        EventV1::StaleDetected(_) => "StaleDetected",
        EventV1::ProviderRequestStarted(_) => "ProviderRequestStarted",
        EventV1::ProviderStreamDelta(_) => "ProviderStreamDelta",
        EventV1::ProviderRequestFinished(_) => "ProviderRequestFinished",
        EventV1::ToolCallRequested(_) => "ToolCallRequested",
        EventV1::ToolCallStarted(_) => "ToolCallStarted",
        EventV1::ToolCallFinished(_) => "ToolCallFinished",
        EventV1::PermissionRequested(_) => "PermissionRequested",
        EventV1::PermissionResolved(_) => "PermissionResolved",
        EventV1::EditProposed(_) => "EditProposed",
        EventV1::EditApplied(_) => "EditApplied",
        EventV1::EditRejected(_) => "EditRejected",
        EventV1::ArtifactWritten(_) => "ArtifactWritten",
        EventV1::PolicyViolationDetected(_) => "PolicyViolationDetected",
        EventV1::UiIntentReceived(_) => "UiIntentReceived",
    }
}

fn render_events_tab(frame: &mut Frame, app: &AppState, area: Rect) {
    let panes = split_panes(area);
    render_event_timeline(frame, app, panes[0], "Events");

    let details = app
        .selected_event()
        .map(pretty_json)
        .unwrap_or_else(|| "No event selected.".to_string());
    render_scrollable_text(
        frame,
        panes[1],
        details_title("Event details", app.focus == Focus::Details),
        &details,
        app.details_scroll,
    );
}

fn render_output_tab(frame: &mut Frame, app: &AppState, area: Rect) {
    let panes = split_panes(area);
    let groups = correlation_groups(&app.events);
    let active_group_id = active_group_id(app, &groups);

    render_group_list(frame, panes[0], &groups, active_group_id.as_deref());

    let details = if let Some(group_id) = active_group_id {
        render_group_details(&app.events, &group_id)
    } else {
        "No correlation groups yet.".to_string()
    };
    render_scrollable_text(
        frame,
        panes[1],
        details_title("Group details", app.focus == Focus::Details),
        &details,
        app.details_scroll,
    );
}

fn render_diff_tab(frame: &mut Frame, app: &AppState, area: Rect) {
    let panes = split_panes(area);
    render_event_timeline(frame, app, panes[0], "Events");

    let details = diff_text(app);
    render_scrollable_text(
        frame,
        panes[1],
        details_title("Diff", app.focus == Focus::Details),
        &details,
        app.details_scroll,
    );
}

fn render_help_tab(frame: &mut Frame, app: &AppState, area: Rect) {
    let mut lines = vec![
        Line::from("Keybindings:"),
        Line::from("  q     : Quit"),
        Line::from("  ?     : Help"),
        Line::from("  Tab   : Cycle Focus"),
        Line::from("  1/2/3 : Switch Tabs"),
        Line::from("  j/k   : Navigate list or scroll details"),
        Line::from("  Space : Toggle Follow Mode"),
        Line::from("  a/d   : Allow/Deny active permission"),
        Line::from("  Esc   : Dismiss permission modal"),
    ];
    if app.replay_mode {
        lines.push(Line::from("  r     : Reload from disk"));
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_permission_modal(frame: &mut Frame, app: &AppState) {
    let Some((permission_id, summary)) = app.active_permission() else {
        return;
    };

    let area = centered_rect(70, 9, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(format!("Permission ID: {permission_id}")),
        Line::from(""),
        Line::from(summary),
        Line::from(""),
        Line::from("a = allow   d = deny   Esc = dismiss"),
    ];

    if app.permission_submission_pending(&permission_id) {
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Decision sent. Waiting for PermissionResolved...",
        ));
    }

    let modal = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("PermissionRequested")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(modal, area);
}

fn split_panes(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area)
        .to_vec()
}

fn render_event_timeline(frame: &mut Frame, app: &AppState, area: Rect, title: &str) {
    let mut lines = Vec::new();

    let visible_rows = usize::from(area.height.saturating_sub(2)).max(1);
    let selected_index = app
        .selected_event_index
        .min(app.events.len().saturating_sub(1));
    let start_index = selected_index.saturating_sub(visible_rows.saturating_sub(1));
    let end_index = (start_index + visible_rows).min(app.events.len());

    for index in start_index..end_index {
        let env = &app.events[index];
        let marker = if index == app.selected_event_index {
            ">"
        } else {
            " "
        };
        let style = if index == app.selected_event_index {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} {:>4} ", env.seq), style),
            Span::styled(event_variant_name(&env.payload), style),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from("No events yet."));
    }

    let block_title = format!(
        "{title} (j/k {}{})",
        if app.focus == Focus::List {
            "active"
        } else {
            "inactive"
        },
        if app.follow_mode { ", follow" } else { "" }
    );

    let widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(block_title))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn render_scrollable_text(frame: &mut Frame, area: Rect, title: String, text: &str, scroll: u16) {
    let widget = Paragraph::new(text.to_string())
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(widget, area);
}

fn details_title(base: &str, details_focused: bool) -> String {
    if details_focused {
        format!("{base} (j/k active)")
    } else {
        format!("{base} (Tab to focus)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    Provider,
    Tool,
    Other,
}

#[derive(Debug, Clone)]
struct CorrelationGroup {
    id: String,
    kind: GroupKind,
    first_seq: u64,
    last_seq: u64,
}

fn correlation_groups(events: &[EventEnvelopeV1]) -> Vec<CorrelationGroup> {
    let mut groups = Vec::<CorrelationGroup>::new();
    let mut index_by_id = BTreeMap::<String, usize>::new();

    for event in events {
        let Some(correlation_id) = event.correlation_id.clone() else {
            continue;
        };

        let kind = group_kind_for_event(&event.payload);
        if let Some(index) = index_by_id.get(&correlation_id).copied() {
            let group = &mut groups[index];
            group.last_seq = event.seq;
            group.kind = merge_group_kind(group.kind, kind);
        } else {
            let index = groups.len();
            groups.push(CorrelationGroup {
                id: correlation_id.clone(),
                kind,
                first_seq: event.seq,
                last_seq: event.seq,
            });
            index_by_id.insert(correlation_id, index);
        }
    }

    groups
}

fn group_kind_for_event(event: &EventV1) -> GroupKind {
    match event {
        EventV1::ProviderRequestStarted(_)
        | EventV1::ProviderStreamDelta(_)
        | EventV1::ProviderRequestFinished(_) => GroupKind::Provider,
        EventV1::ToolCallRequested(_)
        | EventV1::ToolCallStarted(_)
        | EventV1::ToolCallFinished(_)
        | EventV1::PermissionRequested(_)
        | EventV1::PermissionResolved(_)
        | EventV1::EditProposed(_)
        | EventV1::EditApplied(_)
        | EventV1::EditRejected(_)
        | EventV1::ArtifactWritten(_) => GroupKind::Tool,
        _ => GroupKind::Other,
    }
}

fn merge_group_kind(existing: GroupKind, incoming: GroupKind) -> GroupKind {
    match (existing, incoming) {
        (GroupKind::Provider, _) | (_, GroupKind::Provider) => GroupKind::Provider,
        (GroupKind::Tool, _) | (_, GroupKind::Tool) => GroupKind::Tool,
        _ => GroupKind::Other,
    }
}

fn active_group_id(app: &AppState, groups: &[CorrelationGroup]) -> Option<String> {
    if let Some(selected) = app.selected_event() {
        if let Some(correlation_id) = selected.correlation_id.clone() {
            return Some(correlation_id);
        }
    }

    groups
        .iter()
        .rev()
        .find(|group| group.kind == GroupKind::Provider)
        .map(|group| group.id.clone())
        .or_else(|| groups.last().map(|group| group.id.clone()))
}

fn render_group_list(
    frame: &mut Frame,
    area: Rect,
    groups: &[CorrelationGroup],
    active_group_id: Option<&str>,
) {
    let mut lines = Vec::new();

    for group in groups {
        let selected = active_group_id == Some(group.id.as_str());
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(group_label(group), style),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from("No groups yet."));
    }

    let widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Groups"))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn group_label(group: &CorrelationGroup) -> String {
    let kind = match group.kind {
        GroupKind::Provider => "provider",
        GroupKind::Tool => "tool",
        GroupKind::Other => "corr",
    };

    format!(
        "{kind}:{} [{}..{}]",
        group.id, group.first_seq, group.last_seq
    )
}

fn render_group_details(events: &[EventEnvelopeV1], group_id: &str) -> String {
    let mut lines = Vec::<String>::new();
    let mut provider_text = String::new();

    for event in events
        .iter()
        .filter(|event| event.correlation_id.as_deref() == Some(group_id))
    {
        match &event.payload {
            EventV1::ProviderRequestStarted(data) => {
                lines.push(format!(
                    "# Provider request {} [{} / {}]",
                    data.request_id, data.provider_id, data.model_id
                ));
            }
            EventV1::ProviderStreamDelta(data) => {
                provider_text.push_str(&data.delta);
            }
            EventV1::ProviderRequestFinished(data) => {
                lines.push(format!(
                    "# Provider finished: {} (digest={})",
                    data.finish_reason,
                    data.output_digest.as_deref().unwrap_or("none")
                ));
            }
            EventV1::ToolCallRequested(data) => {
                lines.push(format!(
                    "tool requested: {} ({})",
                    data.tool_id, data.tool_call_id
                ));
            }
            EventV1::ToolCallStarted(data) => {
                lines.push(format!("tool started: {}", data.tool_call_id));
            }
            EventV1::ToolCallFinished(data) => {
                lines.push(format!(
                    "tool finished: {} ({:?})",
                    data.tool_call_id, data.status
                ));
                if let Some(summary) = &data.output_summary {
                    lines.push(format!("  summary: {summary}"));
                }
            }
            EventV1::PermissionRequested(data) => {
                lines.push(format!("permission requested: {}", data.permission_id));
            }
            EventV1::PermissionResolved(data) => {
                lines.push(format!(
                    "permission resolved: {} ({:?})",
                    data.permission_id, data.decision
                ));
            }
            EventV1::EditProposed(data) => {
                lines.push(format!("edit proposed: {} ({})", data.edit_id, data.path));
            }
            EventV1::EditApplied(data) => {
                lines.push(format!(
                    "edit applied: {} ({})",
                    data.edit_id, data.new_file_digest
                ));
            }
            EventV1::EditRejected(data) => {
                lines.push(format!("edit rejected: {} ({})", data.edit_id, data.reason));
            }
            EventV1::ArtifactWritten(data) => {
                lines.push(format!("artifact: {} [{}]", data.path, data.digest));
            }
            _ => {
                lines.push(format!("event: {}", event_variant_name(&event.payload)));
            }
        }
    }

    if !provider_text.is_empty() {
        lines.push(String::new());
        lines.push("--- provider output ---".to_string());
        lines.push(provider_text);
    }

    if lines.is_empty() {
        "No events in this group.".to_string()
    } else {
        lines.join("\n")
    }
}

fn diff_text(app: &AppState) -> String {
    let Some(edit) = selected_or_latest_edit_applied(app) else {
        return "No EditApplied events available.".to_string();
    };

    let Some(diff_rel_path) = edit.diff_rel_path.as_deref() else {
        return "Selected edit has no diff artifact reference.".to_string();
    };

    let Some(run_dir) = app.session_path.as_ref() else {
        return "Session path is unavailable; cannot load diff artifact.".to_string();
    };

    let diff_path = run_dir.join(diff_rel_path);
    match fs::read_to_string(&diff_path) {
        Ok(diff) => diff,
        Err(_) => format!("diff artifact missing: {}", diff_path.display()),
    }
}

fn selected_or_latest_edit_applied(app: &AppState) -> Option<&EditAppliedEvent> {
    if app.events.is_empty() {
        return None;
    }

    let selected_index = app
        .selected_event_index
        .min(app.events.len().saturating_sub(1));
    for index in (0..=selected_index).rev() {
        if let EventV1::EditApplied(data) = &app.events[index].payload {
            return Some(data);
        }
    }

    app.events.iter().rev().find_map(|event| {
        if let EventV1::EditApplied(data) = &event.payload {
            Some(data)
        } else {
            None
        }
    })
}

fn pretty_json(event: &EventEnvelopeV1) -> String {
    serde_json::to_string_pretty(event).unwrap_or_else(|_| format!("{:?}", event))
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
