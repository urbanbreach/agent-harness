use super::{Event, StreamDecoder, Value};
use crate::{envelope, ProviderErrorCategory as Category, ProviderErrorEnvelope};

impl StreamDecoder {
    /// The classified error a failed stream reported, once, for the transport to prefer
    /// over the decoder's generic error string.
    pub fn take_failure(&mut self) -> Option<Event> {
        self.failure.take()
    }

    /// The usage seen so far in the shape a finished response reports it: split counts
    /// filled and, for Anthropic, the prompt including cache reads and writes.
    fn reported_usage(&self) -> Option<crate::CompletionUsage> {
        let mut usage = self.usage.clone()?;
        let read = self.metadata.cache_read_tokens.unwrap_or(0);
        let write = self.metadata.cache_write_tokens.unwrap_or(0);
        usage.cache_read_tokens = read;
        usage.cache_write_tokens = write;
        usage.output_tokens = usage.completion_tokens;
        if self.protocol == super::Protocol::Anthropic {
            usage.input_tokens = usage.prompt_tokens;
            usage.prompt_tokens = usage
                .prompt_tokens
                .saturating_add(read)
                .saturating_add(write);
            usage.total_tokens = usage.prompt_tokens.saturating_add(usage.completion_tokens);
        } else {
            usage.input_tokens = usage
                .prompt_tokens
                .saturating_sub(read.saturating_add(write));
        }
        Some(usage)
    }

    /// Remembers a provider error frame: its status, code and message classify it, and the
    /// redacted frame is the envelope body.
    pub(super) fn record_failure(&mut self, frame: &Value, generic: &'static str) -> &'static str {
        let error = frame
            .get("error")
            .or_else(|| frame["response"].get("error"))
            .unwrap_or(frame);
        let word = |key: &str| error[key].as_str().unwrap_or("");
        let (code, kind, message) = (word("code"), word("type"), word("message"));
        let status = [&frame["status"], &error["status"]]
            .into_iter()
            .find_map(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok());
        let text = format!("{code} {kind} {message}").to_lowercase();
        let category = if status.is_some_and(|s| matches!(s, 401 | 403))
            || ["invalid_api_key", "authentication", "permission"]
                .iter()
                .any(|k| text.contains(k))
        {
            Category::InvalidCredentials
        } else if status == Some(429) || text.contains("rate_limit") {
            Category::RateLimited
        } else if text.contains("context_length") || text.contains("prompt is too long") {
            Category::ContextWindowExceeded
        } else if status.is_some_and(|s| s >= 500)
            || ["overloaded", "server_error", "internal_error"]
                .iter()
                .any(|k| text.contains(k))
        {
            Category::TransportFailure
        } else {
            Category::MalformedStream
        };
        self.failure = Some(Event::categorized_error(generic, category).with_envelope(
            ProviderErrorEnvelope {
                status,
                headers: Default::default(),
                body: Some(envelope::redact(&frame.to_string(), &[])),
                usage: self.reported_usage(),
            },
        ));
        generic
    }
}
