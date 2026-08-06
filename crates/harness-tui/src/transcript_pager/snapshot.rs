use harness_core::redact::{DefaultRedactor, Redactor};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptSnapshot {
    text: String,
}

impl TranscriptSnapshot {
    pub fn from_text(text: &str) -> Self {
        let redactor = DefaultRedactor::default();
        Self {
            text: redactor.redact_text(text),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}
