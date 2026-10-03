use super::{AppState, Focus, InterruptReason, OrchestrationTaskState, UiIntent};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use std::collections::BTreeSet;

mod input;
use super::pane_query::{PaneQuery, PaneQueryMode};

#[derive(Debug, Default, Clone)]
pub(crate) struct TasksPaneState {
    pub visible: bool,
    pub focused: bool,
    pub fullscreen: bool,
    pub show_done: bool,
    pub selected: usize,
    pub scroll: usize,
    pub hovered: bool,
    pub query: PaneQuery,
    selected_id: Option<String>,
    pub collapsed: BTreeSet<&'static str>,
    auto_opened: bool,
    running: usize,
}

pub(crate) struct TaskPaneRow {
    pub id: String,
    pub group: &'static str,
    pub title: String,
    pub activity: Option<String>,
    pub model: String,
    pub badge: &'static str,
    pub label: String,
    pub started_ms: u64,
    pub started_seq: u64,
    pub state: OrchestrationTaskState,
    pub elapsed_ms: u64,
    pub child: bool,
    pub header: bool,
}

pub(crate) struct TaskPaneLayout {
    pub rows: Rect,
    pub query: Option<Rect>,
    pub offset: usize,
    pub top: Option<u16>,
    pub bottom: Option<u16>,
}

impl TaskPaneRow {
    pub(crate) fn search_label(&self) -> String {
        if self.header {
            return self.group.to_owned();
        }
        if self.model.is_empty() {
            self.title.clone()
        } else {
            format!("{} {}", self.title, self.model)
        }
    }
}

impl AppState {
    pub(crate) fn task_pane_layout(&self, area: Rect) -> TaskPaneLayout {
        let query = self.tasks_pane.query.has_bar();
        let height = area.height.saturating_sub(u16::from(query));
        let count = self.task_pane_rows().len();
        let offset = self.tasks_pane.scroll.min(count.saturating_sub(1));
        let scrollable = count > usize::from(height) && height >= 3;
        let top = scrollable && offset > 0;
        let bottom = scrollable && offset + usize::from(height - u16::from(top)) < count;
        TaskPaneLayout {
            rows: Rect::new(
                area.x,
                area.y + u16::from(top),
                area.width,
                height.saturating_sub(u16::from(top) + u16::from(bottom)),
            ),
            query: query.then(|| Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1)),
            offset,
            top: top.then_some(area.y),
            bottom: bottom.then(|| area.y + height - 1),
        }
    }

    fn reconcile_task_selection(&mut self) {
        let rows = self.task_pane_rows();
        self.tasks_pane.selected = self
            .tasks_pane
            .selected_id
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or(self.tasks_pane.selected.min(rows.len().saturating_sub(1)));
        self.tasks_pane.selected_id = rows.get(self.tasks_pane.selected).map(|row| row.id.clone());
    }

    fn remember_task_selection(&mut self) {
        self.tasks_pane.selected_id = self
            .task_pane_rows()
            .get(self.tasks_pane.selected)
            .map(|row| row.id.clone());
    }

    pub(crate) fn task_pane_rows(&self) -> Vec<TaskPaneRow> {
        let mut rows = self.task_pane_all_rows();
        rows.retain(|row| self.tasks_pane.query.permits(&row.search_label()));
        rows
    }

    pub(crate) fn task_pane_all_rows(&self) -> Vec<TaskPaneRow> {
        let mut items = Vec::new();
        for child in self.subagents.rows.values() {
            let Some(task) = self.native_subagent_task(&child.parent_tool) else {
                continue;
            };
            if self.tasks_pane.show_done || !task.state.is_terminal() {
                items.push(TaskPaneRow {
                    id: child.id.clone(),
                    group: "Subagents",
                    title: format!("{} {}", child.label, child.description),
                    activity: self
                        .activities
                        .iter()
                        .find(|entry| Some(&entry.request_id) == task.child_request_id.as_ref())
                        .map(|activity| self.child_activity(activity)),
                    model: child.model.clone(),
                    badge: child.context_badge(),
                    label: child.label.clone(),
                    started_ms: child.started_ms,
                    started_seq: child.first_seq,
                    state: task.state,
                    elapsed_ms: self.subagent_elapsed_ms(child),
                    child: true,
                    header: false,
                });
            }
        }
        for task in self
            .orchestration_visible_rows()
            .into_iter()
            .filter(|task| task.queue_key.as_deref() == Some("command"))
        {
            if self.tasks_pane.show_done || !task.state.is_terminal() {
                let title = task
                    .parent_tool_call_id
                    .as_deref()
                    .and_then(|id| {
                        self.activities
                            .iter()
                            .flat_map(|activity| &activity.tool_calls)
                            .find(|tool| tool.tool_call_id == id)
                    })
                    .and_then(|tool| {
                        serde_json::from_str::<serde_json::Value>(&tool.args_summary).ok()
                    })
                    .and_then(|args| {
                        args.get("description")
                            .or_else(|| args.get("command"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| task.task_id.clone());
                items.push(TaskPaneRow {
                    id: task.task_id,
                    group: "Tasks",
                    title,
                    activity: None,
                    model: String::new(),
                    badge: "",
                    label: String::new(),
                    started_ms: task.first_mono_ms,
                    started_seq: task.first_seq,
                    state: task.state,
                    elapsed_ms: task
                        .timing_elapsed_ms
                        .unwrap_or_else(|| task.last_mono_ms.saturating_sub(task.first_mono_ms)),
                    child: false,
                    header: false,
                });
            }
        }
        items.sort_by(|a, b| {
            a.group
                .cmp(b.group)
                .then_with(|| a.state.is_terminal().cmp(&b.state.is_terminal()))
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| b.started_ms.cmp(&a.started_ms))
                .then_with(|| b.started_seq.cmp(&a.started_seq))
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut rows = Vec::new();
        for group in ["Subagents", "Tasks"] {
            let count = items.iter().filter(|item| item.group == group).count();
            if count == 0 {
                continue;
            }
            let collapsed = self.tasks_pane.collapsed.contains(group);
            rows.push(TaskPaneRow {
                id: group.into(),
                group,
                title: format!("{} {group} {count}", if collapsed { "▸" } else { "▾" }),
                activity: None,
                model: String::new(),
                badge: "",
                label: String::new(),
                started_ms: 0,
                started_seq: 0,
                state: OrchestrationTaskState::Completed,
                elapsed_ms: 0,
                child: false,
                header: true,
            });
            if !collapsed {
                rows.extend(items.extract_if(.., |item| item.group == group));
            }
        }
        rows
    }

    pub(crate) fn task_pane_height(&self, height: u16) -> u16 {
        if !self.tasks_pane.visible || self.current_subagent_session_present() || height < 12 {
            return 0;
        }
        u16::try_from(self.task_pane_all_rows().len())
            .unwrap_or(u16::MAX)
            .clamp(1, 8)
            .min((height.saturating_mul(15) / 100).max(1))
            + u16::from(self.tasks_pane.query.has_bar())
    }

    pub(super) fn sync_tasks_pane(&mut self, historical: bool) {
        self.pending_child_cancels.retain(|id, (generation, _)| {
            self.projection
                .subagents
                .history
                .records
                .get(id)
                .is_some_and(|record| record.generation == *generation && record.outcome.is_none())
        });
        if !historical {
            self.projection.subagents.observed_at = Some(self.now());
        }
        if self.session_navigation_stack.is_empty() {
            self.child_transcript_views.retain(|id, _| {
                self.projection
                    .subagents
                    .history
                    .records
                    .get(id)
                    .is_some_and(|record| !record.lifecycle.is_finished())
            });
        }
        let running = self
            .subagents
            .rows
            .values()
            .filter(|row| {
                self.native_subagent_task(&row.parent_tool)
                    .is_some_and(|task| !task.state.is_terminal())
            })
            .count()
            + self
                .orchestration_visible_rows()
                .iter()
                .filter(|row| {
                    row.queue_key.as_deref() == Some("command") && !row.state.is_terminal()
                })
                .count();
        if !historical && running > 0 && self.tasks_pane.running == 0 {
            self.tasks_pane.visible = true;
            self.tasks_pane.auto_opened = true;
        }
        if running == 0
            && self.tasks_pane.auto_opened
            && !self.tasks_pane.focused
            && !self.tasks_pane.show_done
        {
            self.tasks_pane.visible = false;
            self.tasks_pane.auto_opened = false;
        }
        self.tasks_pane.running = running;
        self.reconcile_task_selection();
    }

    pub(super) fn toggle_tasks_pane(&mut self) {
        if !self.tasks_pane.visible {
            self.tasks_pane.visible = true;
            self.tasks_pane.focused = false;
        } else if !self.tasks_pane.focused {
            self.tasks_pane.focused = true;
        } else {
            self.tasks_pane.visible = false;
            self.tasks_pane.focused = false;
        }
        self.tasks_pane.auto_opened = false;
        if !self.tasks_pane.visible {
            self.tasks_pane.query.close_unaccepted();
        }
        self.focus = if self.tasks_pane.focused {
            Focus::List
        } else if self.replay_mode {
            Focus::Details
        } else {
            Focus::Prompt
        };
    }

    fn activate_task_pane_row(&mut self, kill: bool) {
        if self.task_pane_rows().is_empty() {
            return;
        }
        // Native Tasks actions index the unfiltered entries, including headers.
        let Some(row) = self
            .task_pane_all_rows()
            .into_iter()
            .nth(self.tasks_pane.selected)
        else {
            return;
        };
        if row.header {
            if !kill && !self.tasks_pane.collapsed.remove(row.group) {
                self.tasks_pane.collapsed.insert(row.group);
            }
        } else if kill && !self.replay_mode && !row.state.is_terminal() {
            if row.child {
                self.request_child_cancel(row.id);
            } else {
                self.emit_ui_intent(UiIntent::InterruptSession {
                    task_ids: vec![row.id],
                    reason: InterruptReason::User,
                });
            }
        } else if !kill && row.child {
            self.navigate_to_child_session_id(row.id);
        } else if !kill {
            self.open_command_output(&row.id);
        }
    }

    fn open_command_output(&mut self, id: &str) {
        let tool = self
            .orchestration_visible_rows()
            .into_iter()
            .find(|row| row.task_id == id)
            .and_then(|row| row.parent_tool_call_id);
        let area = self.last_frame_area.unwrap_or(Rect::new(0, 0, 120, 40));
        let entry = crate::ui::transcript_navigation_entries(self, area)
            .into_iter()
            .find(|entry| match &entry.target {
                Some(crate::ui::TranscriptMouseTarget::Tool { tool_call_id }) => {
                    tool.as_ref() == Some(tool_call_id)
                }
                Some(crate::ui::TranscriptMouseTarget::ToolGroup { tool_call_ids }) => tool
                    .as_ref()
                    .is_some_and(|tool| tool_call_ids.contains(tool)),
                _ => false,
            });
        if let Some(entry) = entry {
            self.select_transcript_entry(&entry);
            if self.open_selected_transcript_viewer() && !self.replay_mode {
                self.inspected_command = Some(id.into());
                self.emit_ui_intent(UiIntent::InspectCommand {
                    task_id: Some(id.into()),
                });
            }
        }
    }

    pub(crate) fn apply_command_output(
        &mut self,
        snapshot: harness_core::coord::CommandSnapshot,
    ) -> bool {
        if self.inspected_command.as_deref() != Some(snapshot.result.task_id.as_str()) {
            return false;
        }
        let Some(viewer) = &mut self.transcript_viewer else {
            return false;
        };
        let text = format!(
            "{}\n{}\n\n{}{}{}",
            snapshot.result.command,
            snapshot.result.status,
            snapshot.stdout,
            snapshot.stderr,
            if snapshot.result.truncated {
                "\n[output truncated]"
            } else {
                ""
            }
        );
        let _ = viewer.update_content(crate::transcript_block_viewer::ViewerBlockContent::new(
            &text,
            Some(&text),
        ));
        true
    }

    pub(super) fn handle_focused_pane_key(&mut self, key: KeyEvent) -> bool {
        if self.current_subagent_session_present() {
            return false;
        }
        self.handle_tasks_pane_key(key) || self.handle_todo_pane_key(key)
    }
}
