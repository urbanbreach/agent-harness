use super::{Suggestion, SuggestionError};

pub trait SecretSuggestionSink {
    fn write_suggestion(&mut self, text: &str);
}

impl Suggestion {
    pub fn try_persist_to<S: SecretSuggestionSink>(
        &self,
        _sink: &mut S,
    ) -> Result<(), SuggestionError> {
        Err(SuggestionError::PersistenceForbidden)
    }
}
