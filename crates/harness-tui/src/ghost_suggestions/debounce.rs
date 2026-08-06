use crate::scheduling::DualClock;

use super::SuggestionGeneration;

pub const DEFAULT_DEBOUNCE_MS: u64 = 100;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SuggestionContext(String);

impl SuggestionContext {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SuggestionContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SuggestionContext")
            .field(&"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub generation: SuggestionGeneration,
    pub context: SuggestionContext,
    deadline_ms: u64,
}

impl Request {
    pub const fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debouncer {
    delay_ms: u64,
}

impl Debouncer {
    pub const fn new(delay_ms: u64) -> Self {
        Self { delay_ms }
    }

    pub const fn delay_ms(self) -> u64 {
        self.delay_ms
    }

    pub fn schedule(
        self,
        clock: &DualClock,
        generation: SuggestionGeneration,
        context: SuggestionContext,
    ) -> Request {
        Request {
            generation,
            context,
            deadline_ms: clock.flush_now().saturating_add(self.delay_ms),
        }
    }

    pub fn is_due(self, clock: &DualClock, request: &Request) -> bool {
        clock.flush_now() >= request.deadline_ms
    }
}

impl Default for Debouncer {
    fn default() -> Self {
        Self::new(DEFAULT_DEBOUNCE_MS)
    }
}
