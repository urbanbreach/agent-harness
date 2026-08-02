use super::AppState;

impl AppState {
    pub fn queue_pane_queued_count(&self) -> usize {
        self.queued_prompt_count
    }

    pub fn queue_pane_has_queued(&self) -> bool {
        self.queued_prompt_count > 0
    }
}
