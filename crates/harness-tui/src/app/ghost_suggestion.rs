use std::time::Duration;

use super::AppState;

impl AppState {
    pub fn poll_local_ghost_suggestion(&mut self, elapsed_ms: u64) -> bool {
        self.composer.slice.advance_flush(elapsed_ms);
        let Some(request) = self.composer.slice.ready_suggestion() else {
            return false;
        };
        let context = request.context.as_str();
        let prediction = if !self.replay_mode && !self.composer.shell_mode {
            self.composer
                .prompt_history
                .iter()
                .rev()
                .map(String::as_str)
                .find(|candidate| *candidate != context && candidate.starts_with(context))
                .unwrap_or_default()
                .to_owned()
        } else {
            String::new()
        };
        let changed = !prediction.is_empty();
        self.composer
            .slice
            .apply_suggestion_response(&request, prediction)
            .is_ok()
            && changed
    }

    pub(crate) fn composer_suggestion_delay_remaining(&self) -> Option<Duration> {
        let request = self.composer.slice.suggestions().pending()?;
        Some(Duration::from_millis(
            request
                .deadline_ms()
                .saturating_sub(self.composer.slice.clock().flush_now()),
        ))
    }
}
