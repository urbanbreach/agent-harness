use super::{CompletionError, CompletionTrigger};

/// Monotonic identity for one trigger/query request.
pub type CompletionGeneration = u64;

/// A suggestion returned by a completion provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub id: u64,
    pub label: String,
    pub insert_text: String,
}

impl CompletionItem {
    pub fn new(id: u64, label: impl Into<String>, insert_text: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            insert_text: insert_text.into(),
        }
    }
}

/// A provider request whose results may be applied only once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRequest {
    pub generation: CompletionGeneration,
    pub trigger: CompletionTrigger,
}

/// The visible state of the dropdown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompletionStatus {
    #[default]
    Hidden,
    Loading,
    Empty,
    NoResults,
    Ready,
}

/// The shared movement event used by keyboard and pointer adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionDirection {
    Previous,
    Next,
}

/// The typed event emitted by both keyboard and mouse acceptance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionAcceptance {
    pub trigger: CompletionTrigger,
    pub item: CompletionItem,
}

/// Selection, cancellation, scrolling, and result state for one dropdown.
#[derive(Debug, Default)]
pub struct CompletionController {
    generation: CompletionGeneration,
    trigger: Option<CompletionTrigger>,
    results: Vec<CompletionItem>,
    selected: Option<usize>,
    scroll_offset: usize,
    visible_rows: usize,
    status: CompletionStatus,
}

impl CompletionController {
    pub fn new() -> Self {
        Self {
            visible_rows: 5,
            status: CompletionStatus::Hidden,
            ..Self::default()
        }
    }

    pub fn begin(&mut self, trigger: CompletionTrigger) -> CompletionRequest {
        self.generation = self.generation.saturating_add(1);
        self.trigger = Some(trigger.clone());
        self.results.clear();
        self.selected = None;
        self.scroll_offset = 0;
        self.status = CompletionStatus::Loading;
        CompletionRequest {
            generation: self.generation,
            trigger,
        }
    }

    pub fn apply_results(
        &mut self,
        request: &CompletionRequest,
        results: Vec<CompletionItem>,
    ) -> Result<(), CompletionError> {
        if request.generation != self.generation {
            return Err(CompletionError::StaleResults {
                expected: self.generation,
                received: request.generation,
            });
        }
        self.results = results;
        self.selected = (!self.results.is_empty()).then_some(0);
        self.scroll_offset = 0;
        self.status = match (self.results.is_empty(), request.trigger.query.is_empty()) {
            (false, _) => CompletionStatus::Ready,
            (true, true) => CompletionStatus::Empty,
            (true, false) => CompletionStatus::NoResults,
        };
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.trigger = None;
        self.results.clear();
        self.selected = None;
        self.scroll_offset = 0;
        self.status = CompletionStatus::Hidden;
    }

    pub const fn status(&self) -> CompletionStatus {
        self.status
    }

    pub const fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub const fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn set_visible_rows(&mut self, visible_rows: usize) {
        self.visible_rows = visible_rows.max(1);
        self.keep_selected_visible();
    }

    pub fn move_selection(&mut self, direction: SelectionDirection) {
        let Some(selected) = self.selected else {
            return;
        };
        let len = self.results.len();
        if len == 0 {
            return;
        }
        self.selected = Some(match direction {
            SelectionDirection::Previous => selected.checked_sub(1).unwrap_or(len - 1),
            SelectionDirection::Next => (selected + 1) % len,
        });
        self.keep_selected_visible();
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let max_offset = self.results.len().saturating_sub(self.visible_rows);
        self.scroll_offset = if delta.is_negative() {
            self.scroll_offset.saturating_sub(delta.unsigned_abs())
        } else {
            self.scroll_offset
                .saturating_add(delta.unsigned_abs())
                .min(max_offset)
        };
    }

    pub fn accept_keyboard(&self) -> Result<CompletionAcceptance, CompletionError> {
        self.accept_selected()
    }

    pub fn accept_mouse(&mut self, index: usize) -> Result<CompletionAcceptance, CompletionError> {
        self.select(index)?;
        self.accept_selected()
    }

    fn select(&mut self, index: usize) -> Result<(), CompletionError> {
        if index >= self.results.len() {
            return Err(CompletionError::SelectionOutOfBounds {
                index,
                len: self.results.len(),
            });
        }
        self.selected = Some(index);
        self.keep_selected_visible();
        Ok(())
    }

    fn accept_selected(&self) -> Result<CompletionAcceptance, CompletionError> {
        let trigger = self
            .trigger
            .clone()
            .ok_or(CompletionError::NoActiveCompletion)?;
        let index = self.selected.ok_or(CompletionError::NoSelection)?;
        let item =
            self.results
                .get(index)
                .cloned()
                .ok_or(CompletionError::SelectionOutOfBounds {
                    index,
                    len: self.results.len(),
                })?;
        Ok(CompletionAcceptance { trigger, item })
    }

    fn keep_selected_visible(&mut self) {
        let Some(selected) = self.selected else {
            return;
        };
        if selected < self.scroll_offset {
            self.scroll_offset = selected;
        } else if selected >= self.scroll_offset + self.visible_rows {
            self.scroll_offset = selected + 1 - self.visible_rows;
        }
    }
}
