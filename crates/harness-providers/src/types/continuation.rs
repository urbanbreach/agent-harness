use super::*;
use crate::Protocol;

/// Signed provider state is scoped to its origin and never serializable or logged.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderContinuation {
    pub(crate) provider_id: Option<String>,
    pub(crate) model_id: String,
    pub(crate) protocol: Protocol,
    pub(crate) data: Value,
    bytes: usize,
}

impl ProviderContinuation {
    pub(crate) fn new(protocol: Protocol, data: Value) -> Result<Self, &'static str> {
        let bytes = data.to_string().len();
        if bytes > 4 * 1024 * 1024 {
            return Err("provider continuation exceeds 4 MiB");
        }
        Ok(Self {
            provider_id: None,
            model_id: String::new(),
            protocol,
            data,
            bytes,
        })
    }

    pub(crate) fn matches(&self, request: &CompletionRequest, protocol: Protocol) -> bool {
        self.protocol == protocol
            && self.provider_id == request.provider_id
            && self.model_id == request.model_id
    }

    pub fn byte_len(&self) -> usize {
        self.bytes
    }
}

impl std::fmt::Debug for ProviderContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProviderContinuation { transient }")
    }
}
