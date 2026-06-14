use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};

use super::AppState;

impl AppState {
    pub(in crate::app) fn handle_leader_key_event(&mut self, key: KeyEvent) -> bool {
        let now = Instant::now();
        self.key_sequence_state.clear_expired_leader_sequence(now);

        if self.key_sequence_state.leader_sequence_pending(now) {
            self.key_sequence_state.clear_leader_sequence();
            if key.code == KeyCode::Esc {
                return true;
            }
            if let Some(action) = self.keymap.get_leader_action(&key) {
                self.execute_action(action);
            }
            return true;
        }

        if self.keymap.is_leader(&key) {
            self.key_sequence_state.begin_leader_sequence(now);
            return true;
        }

        false
    }

    #[cfg(test)]
    pub(crate) fn leader_key_pending_for_test(&self) -> bool {
        self.key_sequence_state
            .leader_sequence_pending(Instant::now())
    }

    #[cfg(test)]
    pub(crate) fn force_leader_key_timeout_for_test(&mut self) {
        self.key_sequence_state.force_leader_timeout();
    }
}
