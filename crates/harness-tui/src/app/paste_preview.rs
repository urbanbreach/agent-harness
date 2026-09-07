use super::*;

impl AppState {
    pub(crate) fn collapsed_paste_presentation(&self) -> Option<(String, usize)> {
        let preview =
            self.composer.paste_preview.as_ref().filter(|preview| {
                !preview.expanded && preview.buffer == self.composer.prompt_buffer
            })?;
        let label = format!("[Pasted {} lines · Alt+P expand]", preview.line_count);
        let start = self.prompt_char_byte_index(preview.start);
        let end = self.prompt_char_byte_index(preview.end);
        let text = format!(
            "{}{}{}",
            &preview.buffer[..start],
            label,
            &preview.buffer[end..]
        );
        let label_len = label.chars().count();
        let cursor = if self.composer.prompt_cursor <= preview.start {
            self.composer.prompt_cursor
        } else if self.composer.prompt_cursor >= preview.end {
            self.composer.prompt_cursor - (preview.end - preview.start) + label_len
        } else {
            preview.start + label_len
        };
        Some((text, cursor))
    }

    pub(in crate::app) fn handle_paste_preview_key(&mut self, key: KeyEvent) -> bool {
        let Some(preview) = self
            .composer
            .paste_preview
            .as_mut()
            .filter(|preview| preview.buffer == self.composer.prompt_buffer)
        else {
            return false;
        };
        if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::ALT {
            preview.expanded = !preview.expanded;
            return true;
        }
        // Editing or moving inside the source first makes it visible. Submission
        // always uses the complete editor source, never the chip label.
        if matches!(
            key.code,
            KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Backspace
                | KeyCode::Delete
        ) {
            preview.expanded = true;
        }
        false
    }
}
