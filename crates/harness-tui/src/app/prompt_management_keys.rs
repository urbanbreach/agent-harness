use super::*;

impl AppState {
    pub(in crate::app) fn handle_prompt_stash_key(&mut self, key: &KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.close_prompt_stash();
                true
            }
            (KeyCode::Enter, _) => {
                self.pop_selected_prompt_stash();
                true
            }
            (KeyCode::Backspace | KeyCode::Delete, _) => {
                self.delete_selected_prompt_stash();
                true
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_prompt_stash_selection(-1);
                true
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_prompt_stash_selection(1);
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn handle_queued_prompts_key(&mut self, key: &KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.close_queued_prompts();
                true
            }
            (KeyCode::Enter, _) => true,
            (KeyCode::Backspace | KeyCode::Delete, _) => {
                self.delete_selected_queued_prompt_preview();
                true
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_queued_prompt_selection(-1);
                true
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_queued_prompt_selection(1);
                true
            }
            _ => false,
        }
    }
}
