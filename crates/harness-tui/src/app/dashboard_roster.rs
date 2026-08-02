use super::AppState;

impl AppState {
    pub fn dashboard_roster_session_count(&self) -> usize {
        self.session_history_entries.len()
    }

    pub fn dashboard_roster_pinned_count(&self) -> usize {
        self.session_pins.len()
    }

    pub fn dashboard_roster_is_pinned(&self, run_id: &str) -> bool {
        self.session_pins.contains(run_id)
    }

    pub fn dashboard_roster_toggle_pin(&mut self, run_id: &str) {
        if !self.session_pins.insert(run_id.to_string()) {
            self.session_pins.remove(run_id);
        }
    }

    pub fn dashboard_roster_active_task_count(&self) -> usize {
        let summary = self.orchestration_summary();
        summary.queued + summary.running + summary.stale
    }

    pub fn dashboard_roster_has_worktree(&self) -> bool {
        self.session_path.is_some()
    }
}
