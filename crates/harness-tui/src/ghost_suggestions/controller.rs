use crate::composer_atoms::{AtomBuffer, AtomKind};
use crate::composer_editing::ComposerEditor;
use crate::scheduling::DualClock;

use super::debounce::{Debouncer, Request, SuggestionContext, DEFAULT_DEBOUNCE_MS};
use super::generation::{Invalidation, SuggestionGeneration};
use super::SuggestionError;

#[derive(Clone, PartialEq, Eq)]
pub struct Suggestion {
    full_text: String,
    context: SuggestionContext,
    generation: SuggestionGeneration,
}

impl std::fmt::Debug for Suggestion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Suggestion")
            .field("full_text", &"<redacted>")
            .field("context", &"<redacted>")
            .field("generation", &self.generation)
            .finish()
    }
}

impl Suggestion {
    pub fn text(&self) -> &str {
        &self.full_text
    }

    pub const fn generation(&self) -> SuggestionGeneration {
        self.generation
    }

    pub fn context(&self) -> &SuggestionContext {
        &self.context
    }

    fn new(request: &Request, text: impl Into<String>) -> Self {
        Self {
            full_text: text.into(),
            context: request.context.clone(),
            generation: request.generation,
        }
    }

    fn remainder_for(&self, text: &str) -> Option<&str> {
        self.full_text
            .strip_prefix(text)
            .filter(|remainder| !remainder.is_empty())
    }

    fn grapheme_clusters(text: &str) -> Vec<String> {
        AtomBuffer::from_text(text)
            .atoms()
            .iter()
            .map(|atom| match &atom.kind {
                AtomKind::Text(cluster) => cluster.as_str().to_owned(),
                AtomKind::Newline => "\n".to_owned(),
                AtomKind::FileMention(_) | AtomKind::Attachment(_) => String::new(),
            })
            .collect()
    }

    fn prefix(text: &str, units: usize) -> Result<String, SuggestionError> {
        let clusters = Self::grapheme_clusters(text);
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
}

#[derive(Debug)]
pub struct SuggestionController {
    generation: SuggestionGeneration,
    debouncer: Debouncer,
    pub(crate) pending: Option<Request>,
    pub(crate) current: Option<Suggestion>,
    dismissed: bool,
}

impl SuggestionController {
    pub const fn new(debounce_ms: u64) -> Self {
        Self {
            generation: SuggestionGeneration::new(),
            debouncer: Debouncer::new(debounce_ms),
            pending: None,
            current: None,
            dismissed: false,
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

    pub fn ghost_for(&self, text: &str) -> Option<&str> {
        (!self.dismissed)
            .then_some(())
            .and(self.current.as_ref())
            .and_then(|suggestion| suggestion.remainder_for(text))
    }

    pub fn on_edit(
        &mut self,
        clock: &DualClock,
        context: impl Into<String>,
    ) -> Result<Option<Request>, SuggestionError> {
        self.generation.bump(Invalidation::Edit)?;
        self.pending = None;
        if self.current.is_some() {
            return Ok(None);
        }
        Ok(Some(self.schedule(clock, context)))
    }

    pub fn request(
        &mut self,
        clock: &DualClock,
        context: impl Into<String>,
    ) -> Result<Request, SuggestionError> {
        self.generation.bump(Invalidation::Edit)?;
        self.pending = None;
        Ok(self.schedule(clock, context))
    }

    fn schedule(&mut self, clock: &DualClock, context: impl Into<String>) -> Request {
        let request =
            self.debouncer
                .schedule(clock, self.generation, SuggestionContext::new(context));
        self.pending = Some(request.clone());
        request
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

    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    pub fn invalidate(
        &mut self,
        reason: Invalidation,
    ) -> Result<SuggestionGeneration, SuggestionError> {
        let generation = self.generation.bump(reason)?;
        self.pending = None;
        self.current = None;
        self.dismissed = false;
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
        self.current = (!text.trim().is_empty() && !text.contains('\n'))
            .then(|| Suggestion::new(request, text));
        self.dismissed = false;
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
        let remainder = self
            .ghost_for(&editor.text())
            .ok_or(SuggestionError::NoCurrentSuggestion)?;
        let units = Suggestion::grapheme_clusters(remainder).len();
        self.accept_units(editor, units, Invalidation::FullAcceptance)
    }

    fn accept_units(
        &mut self,
        editor: &ComposerEditor,
        units: usize,
        reason: Invalidation,
    ) -> Result<ComposerEditor, SuggestionError> {
        let remainder = self
            .ghost_for(&editor.text())
            .ok_or(SuggestionError::NoCurrentSuggestion)?;
        let prefix = Suggestion::prefix(remainder, units)?;
        let mut next = editor.clone();
        next.insert_text(&prefix)?;
        let generation = self.generation.bump(reason)?;
        self.pending = None;
        let fully_accepted = self
            .current
            .as_ref()
            .is_some_and(|current| current.full_text == next.text());
        if fully_accepted {
            self.current = None;
        } else if let Some(current) = self.current.as_mut() {
            current.generation = generation;
        }
        Ok(next)
    }
}

impl Default for SuggestionController {
    fn default() -> Self {
        Self::new(DEFAULT_DEBOUNCE_MS)
    }
}
