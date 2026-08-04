use crate::attachment_lifecycle::Attachment;
use crate::completion_controller::{
    insert_completion, CompletionAcceptance, CompletionItem, CompletionRequest, CompletionTrigger,
    SelectionDirection,
};
use crate::composer_atoms::{AtomId, AttachmentId};
use crate::composer_editing::{ComposerEditor, DeleteKind};
use crate::ghost_suggestions::Request as SuggestionRequest;

use super::slice::{AttachmentEntry, ComposerSlice, ComposerSliceError};

impl ComposerSlice {
    pub fn replace_text(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.editor = ComposerEditor::from_text(text);
        self.after_edit()
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.editor.insert_text(text)?;
        self.after_edit()
    }

    pub fn paste(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.editor.paste(text)?;
        self.after_edit()
    }

    pub fn backspace(&mut self) -> Result<(), ComposerSliceError> {
        self.editor.backspace()?;
        self.after_edit()
    }

    pub fn delete(&mut self, kind: DeleteKind) -> Result<(), ComposerSliceError> {
        self.editor.delete(kind)?;
        self.after_edit()
    }

    pub fn move_left(&mut self) -> Result<(), ComposerSliceError> {
        self.editor.move_left();
        self.after_edit()
    }

    pub fn move_right(&mut self) -> Result<(), ComposerSliceError> {
        self.editor.move_right();
        self.after_edit()
    }

    pub fn undo(&mut self) -> Result<bool, ComposerSliceError> {
        let changed = self.editor.undo();
        changed
            .then(|| self.after_edit())
            .transpose()
            .map(|_| changed)
    }

    pub fn redo(&mut self) -> Result<bool, ComposerSliceError> {
        let changed = self.editor.redo();
        changed
            .then(|| self.after_edit())
            .transpose()
            .map(|_| changed)
    }

    pub fn attach(
        &mut self,
        id: AttachmentId,
        attachment: Attachment,
    ) -> Result<(), ComposerSliceError> {
        if self.attachments.iter().any(|entry| entry.id == id) {
            return Err(ComposerSliceError::DuplicateAttachment(id));
        }
        let atom_id = AtomId::new(
            self.editor
                .buffer()
                .atoms()
                .iter()
                .map(|atom| atom.id.get())
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        self.editor.insert_attachment(id)?;
        self.attachments.push(AttachmentEntry {
            id,
            atom_id: atom_id.get(),
            attachment,
        });
        self.after_edit()
    }

    pub fn begin_completion(&mut self, trigger: CompletionTrigger) -> CompletionRequest {
        self.completion_trigger = Some(trigger.clone());
        self.completion_items.clear();
        self.completion.begin(trigger)
    }

    pub fn apply_completion_results(
        &mut self,
        request: &CompletionRequest,
        results: Vec<CompletionItem>,
    ) -> Result<(), ComposerSliceError> {
        self.completion.apply_results(request, results.clone())?;
        self.completion_items = results;
        Ok(())
    }

    pub fn cancel_completion(&mut self) {
        self.completion.cancel();
        self.completion_trigger = None;
        self.completion_items.clear();
    }

    pub fn move_completion(&mut self, direction: SelectionDirection) {
        self.completion.move_selection(direction);
    }

    pub fn accept_completion_keyboard(
        &mut self,
    ) -> Result<CompletionAcceptance, ComposerSliceError> {
        let acceptance = self.completion.accept_keyboard()?;
        self.editor = insert_completion(
            &self.editor,
            &acceptance.trigger,
            &acceptance.item.insert_text,
        )?;
        self.cancel_completion();
        self.after_edit()?;
        Ok(acceptance)
    }

    pub fn accept_completion_mouse(
        &mut self,
        index: usize,
    ) -> Result<CompletionAcceptance, ComposerSliceError> {
        let acceptance = self.completion.accept_mouse(index)?;
        self.editor = insert_completion(
            &self.editor,
            &acceptance.trigger,
            &acceptance.item.insert_text,
        )?;
        self.cancel_completion();
        self.after_edit()?;
        Ok(acceptance)
    }

    pub fn request_suggestion(
        &mut self,
        context: impl Into<String>,
    ) -> Result<SuggestionRequest, ComposerSliceError> {
        let (suggestions, clock) = (&mut self.suggestions, &self.clock);
        Ok(suggestions.on_edit(clock, context)?)
    }

    pub fn advance_flush(&self, milliseconds: u64) -> u64 {
        self.clock().advance_flush(milliseconds)
    }

    pub fn ready_suggestion(&self) -> Option<SuggestionRequest> {
        self.suggestions.take_ready_request(self.clock())
    }

    pub fn apply_suggestion_response(
        &mut self,
        request: &SuggestionRequest,
        text: impl Into<String>,
    ) -> Result<(), ComposerSliceError> {
        self.suggestions.apply_response(request, text)?;
        Ok(())
    }

    pub fn accept_next_suggestion(&mut self) -> Result<(), ComposerSliceError> {
        self.editor = self.suggestions.accept_next_grapheme(&self.editor)?;
        self.queue.draft = self.editor.text();
        Ok(())
    }

    pub fn accept_full_suggestion(&mut self) -> Result<(), ComposerSliceError> {
        self.editor = self.suggestions.accept_full(&self.editor)?;
        self.queue.draft = self.editor.text();
        Ok(())
    }

    fn after_edit(&mut self) -> Result<(), ComposerSliceError> {
        self.queue.draft = self.editor.text();
        self.cancel_completion();
        let context = self.editor.text();
        let (suggestions, clock) = (&mut self.suggestions, &self.clock);
        suggestions.on_edit(clock, context)?;
        Ok(())
    }
}
