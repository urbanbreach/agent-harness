use super::*;
use crate::Protocol;

/// Cap for the continuation of an ordinary turn.
pub const CONTINUATION_BYTE_LIMIT: usize = 4 * 1024 * 1024;
/// Cap for a native compaction payload, which carries opaque ciphertext. Only the
/// compaction lanes and the private payload store use it.
pub const COMPACTION_PAYLOAD_BYTE_LIMIT: usize = 32 * 1024 * 1024;

/// Signed provider state is scoped to its origin and never logged. Its only serializable
/// form is [`ContinuationExport`], which the private native payload store writes outside
/// the event journal.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderContinuation {
    pub(crate) provider_id: Option<String>,
    pub(crate) model_id: String,
    pub(crate) protocol: Protocol,
    pub(crate) data: Value,
    bytes: usize,
}

/// The persisted shape of a native compaction payload.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationExport {
    /// `chat`, `responses` or `anthropic`.
    pub protocol: String,
    pub provider_id: Option<String>,
    pub model_id: String,
    pub data: Value,
}

impl std::fmt::Debug for ContinuationExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContinuationExport { transient }")
    }
}

impl Protocol {
    /// The stable wire name used by exports and compaction events.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "chat" => Some(Self::Chat),
            "responses" => Some(Self::Responses),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }
}

impl ProviderContinuation {
    pub(crate) fn new(protocol: Protocol, data: Value) -> Result<Self, &'static str> {
        Self::with_limit(protocol, data, CONTINUATION_BYTE_LIMIT)
    }

    /// Builds a continuation under the given byte cap. Compaction lanes pass
    /// [`COMPACTION_PAYLOAD_BYTE_LIMIT`].
    pub(crate) fn with_limit(
        protocol: Protocol,
        data: Value,
        limit: usize,
    ) -> Result<Self, &'static str> {
        let bytes = data.to_string().len();
        if bytes > limit {
            return Err(if limit > CONTINUATION_BYTE_LIMIT {
                "provider continuation exceeds 32 MiB"
            } else {
                "provider continuation exceeds 4 MiB"
            });
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

    pub const fn protocol(&self) -> Protocol {
        self.protocol
    }

    pub const fn data(&self) -> &Value {
        &self.data
    }

    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Whether this payload belongs to the request's provider, model and wire protocol.
    /// A payload that does not match must never be replayed.
    pub fn is_for(&self, request: &CompletionRequest) -> bool {
        self.matches(request, self.protocol)
    }

    pub fn export(&self) -> ContinuationExport {
        ContinuationExport {
            protocol: self.protocol.as_str().to_owned(),
            provider_id: self.provider_id.clone(),
            model_id: self.model_id.clone(),
            data: self.data.clone(),
        }
    }

    /// Restores an exported payload, enforcing the compaction payload cap.
    pub fn import(export: ContinuationExport) -> Result<Self, &'static str> {
        let protocol = Protocol::from_name(&export.protocol)
            .ok_or("provider continuation names an unknown protocol")?;
        let mut restored = Self::with_limit(protocol, export.data, COMPACTION_PAYLOAD_BYTE_LIMIT)?;
        restored.provider_id = export.provider_id;
        restored.model_id = export.model_id;
        Ok(restored)
    }
}

impl std::fmt::Debug for ProviderContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProviderContinuation { transient }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn export(protocol: &str, data: Value) -> ContinuationExport {
        ContinuationExport {
            protocol: protocol.into(),
            provider_id: Some("openai".into()),
            model_id: "model".into(),
            data,
        }
    }

    #[test]
    fn export_round_trips_and_import_enforces_protocol_and_cap() -> Result<(), &'static str> {
        let payload = ProviderContinuation::import(export("responses", json!([{"id": 1}])))?;
        assert_eq!(ProviderContinuation::import(payload.export())?, payload);
        assert!(ProviderContinuation::import(export("telepathy", json!(null))).is_err());
        let oversized = Value::String("x".repeat(COMPACTION_PAYLOAD_BYTE_LIMIT));
        assert!(ProviderContinuation::import(export("responses", oversized.clone())).is_err());
        // The ordinary-turn cap stays 4 MiB while compaction payloads may be larger.
        let large = Value::String("x".repeat(CONTINUATION_BYTE_LIMIT + 1));
        assert!(ProviderContinuation::new(Protocol::Responses, large.clone()).is_err());
        assert!(ProviderContinuation::import(export("responses", large)).is_ok());
        Ok(())
    }
}
