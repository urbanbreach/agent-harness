use crate::composer_atoms::{AtomBuffer, AtomKind};
use crate::composer_editing::ComposerEditor;
use crate::scheduling::DualClock;

use super::debounce::{Debouncer, Request, SuggestionContext, DEFAULT_DEBOUNCE_MS};
use super::generation::{Invalidation, SuggestionGeneration};
use super::SuggestionError;

#[derive(Clone, PartialEq, Eq)]
pub struct Suggestion {
    text: String,
    context: SuggestionContext,
    generation: SuggestionGeneration,
}

impl std::fmt::Debug for Suggestion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Suggestion")
            .field("text", &"<redacted>")
            .field("context", &"<redacted>")
            .field("generation", &self.generation)
            .finish()
    }
}

impl Suggestion {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn generation(&self) -> SuggestionGeneration {
        self.generation
    }

    pub fn context(&self) -> &SuggestionContext {
        &self.context
    }

    fn new(request: &Request, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            context: request.context.clone(),
            generation: request.generation,
        }
    }

    fn grapheme_clusters(&self) -> Vec<String> {
        AtomBuffer::from_text(&self.text)
            .atoms()
            .iter()
            .map(|atom| match &atom.kind {
                AtomKind::Text(cluster) => cluster.as_str().to_owned(),
                AtomKind::Newline => "\n".to_owned(),
                AtomKind::FileMention(_) | AtomKind::Attachment(_) => String::new(),
            })
            .collect()
    }

    fn prefix(&self, units: usize) -> Result<String, SuggestionError> {
        let clusters = self.grapheme_clusters();
        if units == 0 {
            return Err(SuggestionError::ZeroPartialUnits);
        }
        if units > clusters.len() {
            return Err(SuggestionError::TooManyPartialUnits {
                requested: units,
                available: clusters.len(),
            });
        }
        Ok(clusters[..units].concat())
    }

    fn consume(&mut self, units: usize) {
        let clusters = self.grapheme_clusters();
        self.text = clusters[units..].concat();
    }
}

#[derive(Debug)]
pub struct SuggestionController {
    generation: SuggestionGeneration,
    debouncer: Debouncer,
    pub(crate) pending: Option<Request>,
    pub(crate) current: Option<Suggestion>,
}

impl SuggestionController {
    pub const fn new(debounce_ms: u64) -> Self {
        Self {
            generation: SuggestionGeneration::new(),
            debouncer: Debouncer::new(debounce_ms),
            pending: None,
            current: None,
        }
    }

    pub const fn generation(&self) -> SuggestionGeneration {
        self.generation
    }

    pub const fn pending(&self) -> Option<&Request> {
        self.pending.as_ref()
    }

    pub const fn current(&self) -> Option<&Suggestion> {
        self.current.as_ref()
    }

    pub fn on_edit(
        &mut self,
        clock: &DualClock,
        context: impl Into<String>,
    ) -> Result<Request, SuggestionError> {
        self.invalidate(Invalidation::Edit)?;
        let request =
            self.debouncer
                .schedule(clock, self.generation, SuggestionContext::new(context));
        self.pending = Some(request.clone());
        Ok(request)
    }

    pub fn on_focus_change(&mut self) -> Result<SuggestionGeneration, SuggestionError> {
        self.invalidate(Invalidation::FocusChange)
    }

    pub fn on_state_change(&mut self) -> Result<SuggestionGeneration, SuggestionError> {
        self.invalidate(Invalidation::StateChange)
    }

    pub fn cancel(&mut self) -> Result<SuggestionGeneration, SuggestionError> {
        self.invalidate(Invalidation::Cancellation)
    }

    pub fn invalidate(
        &mut self,
        reason: Invalidation,
    ) -> Result<SuggestionGeneration, SuggestionError> {
        let generation = self.generation.bump(reason)?;
        self.pending = None;
        self.current = None;
        Ok(generation)
    }

    pub fn take_ready_request(&self, clock: &DualClock) -> Option<Request> {
        self.pending
            .as_ref()
            .filter(|request| self.debouncer.is_due(clock, request))
            .cloned()
    }

    pub fn apply_response(
        &mut self,
        request: &Request,
        text: impl Into<String>,
    ) -> Result<(), SuggestionError> {
        if request.generation != self.generation {
            return Err(SuggestionError::StaleGeneration {
                expected: self.generation,
                received: request.generation,
            });
        }
        let Some(pending) = self.pending.as_ref() else {
            return Err(SuggestionError::RequestMismatch);
        };
        if pending.generation != request.generation || pending.context != request.context {
            return Err(SuggestionError::RequestMismatch);
        }
        self.pending = None;
        let text = text.into();
        self.current = (!text.is_empty()).then(|| Suggestion::new(request, text));
        Ok(())
    }

    pub fn accept_next_grapheme(
        &mut self,
        editor: &ComposerEditor,
    ) -> Result<ComposerEditor, SuggestionError> {
        self.accept_units(editor, 1, Invalidation::PartialAcceptance)
    }

    pub fn accept_full(
        &mut self,
        editor: &ComposerEditor,
    ) -> Result<ComposerEditor, SuggestionError> {
        let units = self
            .current
            .as_ref()
            .ok_or(SuggestionError::NoCurrentSuggestion)?
            .grapheme_clusters()
            .len();
        self.accept_units(editor, units, Invalidation::FullAcceptance)
    }

    fn accept_units(
        &mut self,
        editor: &ComposerEditor,
        units: usize,
        reason: Invalidation,
    ) -> Result<ComposerEditor, SuggestionError> {
        let prefix = self
            .current
            .as_ref()
            .ok_or(SuggestionError::NoCurrentSuggestion)?
            .prefix(units)?;
        let mut next = editor.clone();
        next.insert_text(&prefix)?;
        let generation = self.generation.bump(reason)?;
        self.pending = None;
        let empty = if let Some(current) = self.current.as_mut() {
            current.consume(units);
            current.generation = generation;
            current.text.is_empty()
        } else {
            false
        };
        if empty {
            self.current = None;
        }
        Ok(next)
    }
}

impl Default for SuggestionController {
    fn default() -> Self {
        Self::new(DEFAULT_DEBOUNCE_MS)
    }
}
