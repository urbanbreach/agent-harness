use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{RunStatus, SessionCatalogEntry};
use harness_core::session_lineage::{
    project_lineage_tree, validate_tui_fork_stable_prefix, SessionLineageNode, StableSessionPrefix,
};

use super::AppState;
use crate::text::non_empty_trimmed;
use crate::view_model::{
    ForkSelectorRowViewModel, ForkSelectorViewModel, LineageBrowserRowViewModel,
    LineageBrowserViewModel,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageBrowserState {
    nodes: Vec<LineageBrowserNode>,
    visible: Vec<usize>,
    selected: usize,
    expanded: BTreeSet<String>,
    current_run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LineageBrowserNode {
    catalog: SessionCatalogEntry,
    depth: usize,
    parent_index: Option<usize>,
    child_indices: Vec<usize>,
}

impl LineageBrowserState {
    pub(in crate::app) fn rebuild(
        &mut self,
        entries: impl IntoIterator<Item = SessionCatalogEntry>,
        current_run_id: Option<String>,
        filter_input: &str,
    ) {
        let previous_selected = self.selected_run_id().map(str::to_string);
        let previous_expanded = self.expanded.clone();
        self.nodes.clear();
        self.visible.clear();
        self.expanded.clear();
        self.current_run_id = current_run_id;

        let tree = project_lineage_tree(entries);
        for root in tree.roots {
            self.push_node(root, 0, None);
        }

        self.expanded = self
            .nodes
            .iter()
            .filter(|node| !node.child_indices.is_empty())
            .map(|node| node.catalog.run_id.clone())
            .filter(|run_id| previous_expanded.is_empty() || previous_expanded.contains(run_id))
            .collect();
        self.update_filter(filter_input);

        if let Some(previous_selected) = previous_selected {
            if let Some(index) = self
                .visible
                .iter()
                .position(|node_index| self.nodes[*node_index].catalog.run_id == previous_selected)
            {
                self.selected = index;
            }
        }
    }

    fn push_node(
        &mut self,
        node: SessionLineageNode,
        depth: usize,
        parent_index: Option<usize>,
    ) -> usize {
        let index = self.nodes.len();
        self.nodes.push(LineageBrowserNode {
            catalog: node.entry,
            depth,
            parent_index,
            child_indices: Vec::new(),
        });

        for child in node.children {
            let child_index = self.push_node(child, depth + 1, Some(index));
            self.nodes[index].child_indices.push(child_index);
        }

        index
    }

    fn update_filter(&mut self, filter_input: &str) {
        let input = normalized_filter(filter_input);
        self.visible = if input.is_empty() {
            self.nodes
                .iter()
                .enumerate()
                .filter_map(|(index, _)| self.node_expansion_visible(index).then_some(index))
                .collect()
        } else {
            let mut included = BTreeSet::new();
            for (index, node) in self.nodes.iter().enumerate() {
                if lineage_node_matches(node, &input) {
                    included.insert(index);
                    let mut parent = node.parent_index;
                    while let Some(parent_index) = parent {
                        included.insert(parent_index);
                        parent = self.nodes[parent_index].parent_index;
                    }
                }
            }
            self.nodes
                .iter()
                .enumerate()
                .filter_map(|(index, _)| included.contains(&index).then_some(index))
                .collect()
        };
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
    }

    fn node_expansion_visible(&self, index: usize) -> bool {
        let mut parent = self.nodes[index].parent_index;
        while let Some(parent_index) = parent {
            let parent_node = &self.nodes[parent_index];
            if !self.expanded.contains(&parent_node.catalog.run_id) {
                return false;
            }
            parent = parent_node.parent_index;
        }
        true
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = moved_selection_index(self.selected, self.visible.len(), delta);
    }

    pub fn toggle_selected_fold(&mut self, filter_input: &str) {
        let Some(node) = self.selected_node() else {
            return;
        };
        if node.child_indices.is_empty() {
            return;
        }
        let run_id = node.catalog.run_id.clone();
        if !self.expanded.remove(&run_id) {
            self.expanded.insert(run_id);
        }
        self.update_filter(filter_input);
    }

    pub fn selected_run_id(&self) -> Option<&str> {
        self.selected_node()
            .map(|node| node.catalog.run_id.as_str())
    }

    fn selected_node(&self) -> Option<&LineageBrowserNode> {
        self.visible
            .get(self.selected)
            .and_then(|node_index| self.nodes.get(*node_index))
    }

    pub fn view_model(&self, filter_input: &str) -> LineageBrowserViewModel {
        let rows = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(visible_index, node_index)| {
                let node = self.nodes.get(*node_index)?;
                Some(LineageBrowserRowViewModel {
                    run_id: node.catalog.run_id.clone(),
                    title: lineage_row_title(&node.catalog),
                    depth: node.depth,
                    parent_run_id: node.catalog.parent_session_id.clone(),
                    status: node.catalog.status,
                    updated_at: node.catalog.last_updated_at.clone(),
                    profile: node.catalog.profile_preset.clone(),
                    provider_model: node.catalog.provider_model.clone(),
                    child_count: node.child_indices.len(),
                    expanded: self.expanded.contains(&node.catalog.run_id),
                    selected: visible_index == self.selected && !self.visible.is_empty(),
                    current: self.current_run_id.as_deref() == Some(node.catalog.run_id.as_str()),
                })
            })
            .collect::<Vec<_>>();

        let empty_message = if self.nodes.is_empty() {
            Some("No saved sessions".to_string())
        } else if rows.is_empty() {
            Some(format!("No sessions match `{}`", filter_input.trim()))
        } else {
            None
        };

        LineageBrowserViewModel {
            filter_input: filter_input.to_string(),
            rows,
            empty_message,
            selected_run_id: self.selected_run_id().map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForkSelectorState {
    rows: Vec<ForkSelectorRow>,
    filtered: Vec<usize>,
    selected: usize,
    confirmed: Option<StableSessionPrefix>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForkSelectorRow {
    prefix: StableSessionPrefix,
    event_id: Option<String>,
    event_kind: &'static str,
    prompt_text: String,
    restore_prompt_text: Option<String>,
    timestamp: Option<String>,
}

impl ForkSelectorState {
    fn rebuild(&mut self, events: &[EventEnvelopeV1], filter_input: &str) {
        let full_session = events
            .last()
            .and_then(|event| validate_tui_fork_stable_prefix(events, event.seq).ok())
            .filter(|prefix| prefix.event_count > 0)
            .map(|prefix| ForkSelectorRow {
                prefix,
                event_id: None,
                event_kind: "full_session",
                prompt_text: "Full session".to_string(),
                restore_prompt_text: None,
                timestamp: None,
            });

        let mut prompt_rows = events
            .iter()
            .filter_map(|event| {
                let EventV1::UserMessageSubmitted(payload) = &event.payload else {
                    return None;
                };
                let prefix =
                    validate_tui_fork_stable_prefix(events, event.seq.saturating_sub(1)).ok()?;
                (prefix.event_count > 0).then(|| ForkSelectorRow {
                    prefix,
                    event_id: Some(event.event_id.clone()),
                    event_kind: event_kind_label(&event.payload),
                    prompt_text: payload.text.clone(),
                    restore_prompt_text: Some(payload.text.clone()),
                    timestamp: event.ts.clone(),
                })
            })
            .collect::<Vec<_>>();
        prompt_rows.reverse();

        self.rows = full_session
            .into_iter()
            .chain(prompt_rows)
            .collect::<Vec<_>>();
        self.confirmed = None;
        self.update_filter(filter_input);
    }

    fn update_filter(&mut self, filter_input: &str) {
        let input = normalized_filter(filter_input);
        self.filtered = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| fork_row_matches(row, &input).then_some(index))
            .collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = moved_selection_index(self.selected, self.filtered.len(), delta);
    }

    pub fn confirm_selected(&mut self) -> Option<StableSessionPrefix> {
        let prefix = self
            .filtered
            .get(self.selected)
            .and_then(|row_index| self.rows.get(*row_index))
            .map(|row| row.prefix.clone())?;
        self.confirmed = Some(prefix.clone());
        Some(prefix)
    }

    pub fn confirmed_prefix(&self) -> Option<&StableSessionPrefix> {
        self.confirmed.as_ref()
    }

    fn confirmed_row(&self) -> Option<&ForkSelectorRow> {
        let confirmed = self.confirmed.as_ref()?;
        self.rows.iter().find(|row| &row.prefix == confirmed)
    }

    fn confirmed_prompt_text(&self) -> String {
        self.confirmed_row()
            .and_then(|row| row.restore_prompt_text.clone())
            .unwrap_or_default()
    }

    pub fn selected_prefix(&self) -> Option<&StableSessionPrefix> {
        self.filtered
            .get(self.selected)
            .and_then(|row_index| self.rows.get(*row_index))
            .map(|row| &row.prefix)
    }

    pub fn view_model(&self, filter_input: &str) -> ForkSelectorViewModel {
        let rows = self
            .filtered
            .iter()
            .enumerate()
            .filter_map(|(filtered_index, row_index)| {
                let row = self.rows.get(*row_index)?;
                Some(ForkSelectorRowViewModel {
                    cutoff_seq: row.prefix.cutoff_seq,
                    event_count: row.prefix.event_count,
                    run_id: row.prefix.run_id.clone(),
                    status: row.prefix.status,
                    event_id: row.event_id.clone(),
                    event_kind: row.event_kind,
                    prompt_text: row.prompt_text.clone(),
                    timestamp: row.timestamp.clone(),
                    selected: filtered_index == self.selected && !self.filtered.is_empty(),
                })
            })
            .collect::<Vec<_>>();
        let empty_message = if self.rows.is_empty() {
            Some("No stable fork points".to_string())
        } else if rows.is_empty() {
            Some(format!("No fork points match `{}`", filter_input.trim()))
        } else {
            None
        };

        ForkSelectorViewModel {
            filter_input: filter_input.to_string(),
            rows,
            empty_message,
            selected_cutoff_seq: self.selected_prefix().map(|prefix| prefix.cutoff_seq),
            confirmed_cutoff_seq: self.confirmed.as_ref().map(|prefix| prefix.cutoff_seq),
        }
    }
}

impl AppState {
    pub fn open_lineage_browser(&mut self) {
        self.close_palette();
        self.palette_focus_return.get_or_insert(self.focus);
        self.palette_input.clear();
        self.palette_cursor = 0;
        let current_run_id = self.current_session_id().map(str::to_string);
        let entries = self
            .session_history_entries
            .iter()
            .map(|entry| entry.catalog.clone())
            .collect::<Vec<_>>();
        self.lineage_browser
            .rebuild(entries, current_run_id, &self.palette_input);
        self.lineage_browser_visible = true;
        self.fork_selector_visible = false;
    }

    pub fn open_fork_selector(&mut self) {
        self.close_palette();
        self.palette_focus_return.get_or_insert(self.focus);
        self.palette_input.clear();
        self.palette_cursor = 0;
        let events = self.events.clone();
        let filter_input = self.palette_input.clone();
        self.fork_selector.rebuild(&events, &filter_input);
        self.fork_selector_visible = true;
        self.lineage_browser_visible = false;
    }

    pub fn close_lineage_surfaces(&mut self) {
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
    }

    pub fn lineage_browser_view_model(&self) -> LineageBrowserViewModel {
        self.lineage_browser.view_model(&self.palette_input)
    }

    pub fn fork_selector_view_model(&self) -> ForkSelectorViewModel {
        self.fork_selector.view_model(&self.palette_input)
    }

    pub fn selected_fork_prefix(&self) -> Option<&StableSessionPrefix> {
        self.fork_selector.selected_prefix()
    }

    pub fn confirmed_fork_prefix(&self) -> Option<&StableSessionPrefix> {
        self.fork_selector.confirmed_prefix()
    }

    pub(in crate::app) fn handle_lineage_browser_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_lineage_surfaces();
                true
            }
            KeyCode::Enter => true,
            KeyCode::PageUp => {
                self.lineage_browser.move_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.lineage_browser.move_selection(10);
                true
            }
            KeyCode::Home => {
                self.lineage_browser.selected = 0;
                true
            }
            KeyCode::End => {
                self.lineage_browser.selected =
                    self.lineage_browser.visible.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.lineage_browser.move_selection(-1);
                true
            }
            KeyCode::Down => {
                self.lineage_browser.move_selection(1);
                true
            }
            KeyCode::Left | KeyCode::Right => {
                self.lineage_browser
                    .toggle_selected_fold(&self.palette_input);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_lineage_browser_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_lineage_browser_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.lineage_browser.move_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.lineage_browser.move_selection(1);
                true
            }
            KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE => {
                self.lineage_browser
                    .toggle_selected_fold(&self.palette_input);
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_lineage_browser_filter);
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn handle_fork_selector_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_lineage_surfaces();
                true
            }
            KeyCode::Enter => {
                if let Some(prefix) = self.fork_selector.confirm_selected() {
                    let prompt_text = self.fork_selector.confirmed_prompt_text();
                    match self.emit_fork_session_intent(prefix, prompt_text) {
                        Ok(()) => {
                            self.fork_selector_visible = false;
                        }
                        Err(err) => self.set_status_banner(Some(err)),
                    }
                }
                true
            }
            KeyCode::PageUp => {
                self.fork_selector.move_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.fork_selector.move_selection(10);
                true
            }
            KeyCode::Home => {
                self.fork_selector.selected = 0;
                true
            }
            KeyCode::End => {
                self.fork_selector.selected = self.fork_selector.filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.fork_selector.move_selection(-1);
                true
            }
            KeyCode::Down => {
                self.fork_selector.move_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_fork_selector_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_fork_selector_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.fork_selector.move_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.fork_selector.move_selection(1);
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_fork_selector_filter);
                true
            }
            _ => false,
        }
    }

    fn update_lineage_browser_filter(&mut self) {
        self.lineage_browser.update_filter(&self.palette_input);
    }

    fn update_fork_selector_filter(&mut self) {
        self.fork_selector.update_filter(&self.palette_input);
    }
}

fn normalized_filter(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn moved_selection_index(selected: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }

    if delta == -1 {
        return if selected == 0 { len - 1 } else { selected - 1 };
    }

    if delta == 1 {
        return (selected + 1) % len;
    }

    let current = selected.min(len.saturating_sub(1)) as isize;
    let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
    usize::try_from(next).unwrap_or(0)
}

fn lineage_node_matches(node: &LineageBrowserNode, input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    lineage_catalog_candidates(&node.catalog)
        .iter()
        .any(|candidate| candidate.contains(input))
}

fn lineage_catalog_candidates(catalog: &SessionCatalogEntry) -> Vec<String> {
    [
        Some(lineage_row_title(catalog)),
        Some(catalog.run_id.clone()),
        catalog.status.map(status_label).map(str::to_string),
        catalog.last_updated_at.clone(),
        catalog.profile_preset.clone(),
        catalog.provider_model.clone(),
        Some(format!("{:?}", catalog.mode_source)),
        catalog.parent_session_id.clone(),
        Some(format!("{} artifacts", catalog.artifact_count)),
        Some(format!("{} children", catalog.child_session_count)),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .collect()
}

fn fork_row_matches(row: &ForkSelectorRow, input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    [
        Some(row.prefix.cutoff_seq.to_string()),
        row.prefix.run_id.clone(),
        row.prefix.status.map(status_label).map(str::to_string),
        row.event_id.clone(),
        Some(row.event_kind.to_string()),
        Some(row.prompt_text.clone()),
        row.timestamp.clone(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .any(|candidate| candidate.contains(input))
}

fn lineage_row_title(catalog: &SessionCatalogEntry) -> String {
    catalog
        .run_name
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(catalog.run_id.as_str())
        .to_string()
}

fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Finished => "finished",
        RunStatus::Failed => "failed",
    }
}

fn event_kind_label(event: &EventV1) -> &'static str {
    match event {
        EventV1::RunStarted(_) => "run_started",
        EventV1::SessionTitleUpdated(_) => "session_title_updated",
        EventV1::RunFinished(_) => "run_finished",
        EventV1::RunFailed(_) => "run_failed",
        EventV1::AgentSpawned(_) => "agent_spawned",
        EventV1::AgentStopped(_) => "agent_stopped",
        EventV1::TaskScheduled(_) => "task_scheduled",
        EventV1::TaskCancelled(_) => "task_cancelled",
        EventV1::TaskCompleted(_) => "task_completed",
        EventV1::TaskResultLate(_) => "task_result_late",
        EventV1::BackgroundTaskNotification(_) => "background_task_notification",
        EventV1::StaleDetected(_) => "stale_detected",
        EventV1::UserMessageSubmitted(_) => "user_message_submitted",
        EventV1::ProviderRequestStarted(_) => "provider_request_started",
        EventV1::ProviderRequestFinished(_) => "provider_request_finished",
        EventV1::ProviderStreamDelta(_) => "provider_stream_delta",
        EventV1::ProviderReasoningDelta(_) => "provider_reasoning_delta",
        EventV1::AssistantMessageFinished(_) => "assistant_message_finished",
        EventV1::CompactionRequested(_) => "compaction_requested",
        EventV1::CompactionWritten(_) => "compaction_written",
        EventV1::CompactionApplied(_) => "compaction_applied",
        EventV1::CompactionFailed(_) => "compaction_failed",
        EventV1::ToolCallRequested(_) => "tool_call_requested",
        EventV1::ToolCallStarted(_) => "tool_call_started",
        EventV1::ToolCallFinished(_) => "tool_call_finished",
        EventV1::PermissionRequested(_) => "permission_requested",
        EventV1::PermissionGrantRecorded(_) => "permission_grant_recorded",
        EventV1::PermissionResolved(_) => "permission_resolved",
        EventV1::EditProposed(_) => "edit_proposed",
        EventV1::EditApplied(_) => "edit_applied",
        EventV1::EditRejected(_) => "edit_rejected",
        EventV1::ArtifactWritten(_) => "artifact_written",
        EventV1::PolicyViolationDetected(_) => "policy_violation_detected",
        EventV1::TeamCreated(_) => "team_created",
        EventV1::TeamMemberSpawned(_) => "team_member_spawned",
        EventV1::TeamMessageSent(_) => "team_message_sent",
        EventV1::TeamTaskCreated(_) => "team_task_created",
        EventV1::TeamTaskUpdated(_) => "team_task_updated",
        EventV1::TeamShutdownRequested(_) => "team_shutdown_requested",
        EventV1::TeamShutdownApproved(_) => "team_shutdown_approved",
        EventV1::TeamShutdownRejected(_) => "team_shutdown_rejected",
        EventV1::TeamDeleted(_) => "team_deleted",
        EventV1::UiIntentReceived(_) => "ui_intent_received",
    }
}
