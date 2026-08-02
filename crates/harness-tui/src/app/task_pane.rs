use super::AppState;
use crate::app::OrchestrationTaskState;

impl AppState {
    pub fn task_pane_visible_rows(&self) -> Vec<crate::app::OrchestrationTaskRow> {
        self.orchestration_visible_rows()
    }

    pub fn task_pane_active_count(&self) -> usize {
        let summary = self.orchestration_summary();
        summary.queued + summary.running
    }

    pub fn task_pane_stale_count(&self) -> usize {
        self.orchestration_summary().stale
    }

    pub fn task_pane_has_terminal_tasks(&self) -> bool {
        self.orchestration_visible_rows()
            .iter()
            .any(|row| row.state.is_terminal())
    }

    pub fn task_pane_cancelled_count(&self) -> usize {
        self.orchestration_visible_rows()
            .iter()
            .filter(|row| row.state == OrchestrationTaskState::Cancelled)
            .count()
    }
}
