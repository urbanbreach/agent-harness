use super::AppState;

impl AppState {
    pub fn todo_pane_has_tool_calls(&self) -> bool {
        self.orchestration_visible_rows()
            .iter()
            .any(|row| row.child_tool_call_count > 0)
    }

    pub fn todo_pane_total_tool_calls(&self) -> usize {
        self.orchestration_visible_rows()
            .iter()
            .map(|row| row.child_tool_call_count)
            .sum()
    }
}
