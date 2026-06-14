use super::{AppState, ToolCallEntry};
use crate::text::non_empty_trimmed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSessionDialogRow {
    pub session_id: String,
    pub request_id: Option<String>,
    pub title: String,
    pub status: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSessionDialogViewModel {
    pub rows: Vec<ChildSessionDialogRow>,
    pub empty_message: Option<String>,
}

impl AppState {
    pub(in crate::app) fn open_child_session_dialog(&mut self) {
        self.close_palette();
        self.overlay_state.child_sessions_visible = true;
    }

    pub(in crate::app) fn close_child_session_dialog(&mut self) {
        self.overlay_state.child_sessions_visible = false;
    }

    pub(in crate::app) fn handle_child_session_dialog_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> bool {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.close_child_session_dialog();
                true
            }
            crossterm::event::KeyCode::Enter => {
                if let Some(row) = self.child_session_dialog_view_model().rows.first() {
                    let session_id = row.session_id.clone();
                    self.close_child_session_dialog();
                    self.navigate_to_child_session_id(session_id);
                }
                true
            }
            _ => true,
        }
    }

    pub fn child_session_dialog_view_model(&self) -> ChildSessionDialogViewModel {
        let rows = self
            .activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter_map(child_session_row_for_tool)
            .enumerate()
            .map(|(index, mut row)| {
                row.selected = index == 0;
                row
            })
            .collect::<Vec<_>>();
        let empty_message = rows.is_empty().then(|| "No child sessions".to_string());
        ChildSessionDialogViewModel {
            rows,
            empty_message,
        }
    }
}

fn child_session_row_for_tool(tool_call: &ToolCallEntry) -> Option<ChildSessionDialogRow> {
    let session_id = tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_session_id.as_deref())
        .and_then(non_empty_trimmed)
        .map(str::to_string)
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| json_string(value, &["child_session_id", "session_id"]))
        })?;
    let request_id = tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_request_id.as_deref())
        .and_then(non_empty_trimmed)
        .map(str::to_string)
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| json_string(value, &["child_request_id", "request_id"]))
        });
    let title = tool_call
        .output_summary
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or_else(|| tool_call.effective_tool_id())
        .to_string();
    Some(ChildSessionDialogRow {
        session_id,
        request_id,
        title,
        status: format!("{:?}", tool_call.status).to_ascii_lowercase(),
        selected: false,
    })
}

fn json_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}
