use crate::{CompletionRequest, ProviderStreamEvent};
use serde_json::Value;
mod request;
mod stream;
pub(crate) use request::{encode, encode_at};
pub(crate) use stream::StreamDecoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Chat,
    Responses,
    Anthropic,
}

#[cfg(test)]
mod tests;
