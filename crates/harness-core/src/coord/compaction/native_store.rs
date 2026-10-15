//! Private storage for provider-native compaction payloads.
//!
//! A payload carries opaque provider state, so it is written as a 0600 artifact outside
//! `events.jsonl`; the compaction event holds only a [`NativePayloadRef`]. Exports skip these
//! artifacts, and a payload is reused after a restart only for the provider and model that
//! produced it.
use crate::{
    coord::{runtime::Runtime, CoordinatorError},
    event::{ArtifactWrittenEvent, EventActor, EventV1, NativePayloadRef, NATIVE_ARTIFACT_KIND},
};
use harness_providers::{
    CompletionRequest, ContinuationExport, ProviderContinuation, COMPACTION_PAYLOAD_BYTE_LIMIT,
};
use std::{collections::BTreeMap, io::Read, path::Path};

impl Runtime {
    /// Stores `payload` and records the artifact in the journal. The returned reference
    /// goes on the compaction event; the payload never does.
    pub fn write_native_payload(
        &mut self,
        actor: &EventActor,
        payload: &ProviderContinuation,
    ) -> Result<NativePayloadRef, CoordinatorError> {
        let bytes = serde_json::to_vec(&payload.export())?;
        if bytes.len() > COMPACTION_PAYLOAD_BYTE_LIMIT {
            return Err(CoordinatorError::Invalid(
                "native compaction payload exceeds 32 MiB".into(),
            ));
        }
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let path = format!("artifacts/native-{digest}.bin");
        crate::store::create_private_dir(&self.info()?.artifacts_dir)?;
        crate::store::write_private_atomic(&self.info()?.run_dir.join(&path), &bytes)?;
        let reference = NativePayloadRef {
            path: path.clone(),
            digest: digest.clone(),
            bytes: bytes.len() as u64,
        };
        self.emit(
            actor.clone(),
            None,
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path,
                digest,
                bytes: reference.bytes,
                tool_call_id: None,
                tool_metadata: None,
                metadata: BTreeMap::from([("kind".to_owned(), NATIVE_ARTIFACT_KIND.to_owned())]),
            }),
        )?;
        Ok(reference)
    }
}

/// Loads a stored payload. Returns `None` for anything unreadable: a path that is not the
/// canonical artifact name, a size or digest mismatch, malformed content, or a payload that
/// belongs to another provider or model than `request`'s.
pub fn read_native_payload(
    run_dir: &Path,
    reference: &NativePayloadRef,
    request: &CompletionRequest,
) -> Option<ProviderContinuation> {
    let canonical = format!("artifacts/native-{}.bin", reference.digest);
    if reference.path != canonical
        || reference.digest.len() != 64
        || !reference.digest.bytes().all(|b| b.is_ascii_hexdigit())
        || usize::try_from(reference.bytes).ok()? > COMPACTION_PAYLOAD_BYTE_LIMIT
    {
        return None;
    }
    let file = crate::store::open_private_file(&run_dir.join(&reference.path)).ok()?;
    let mut bytes = Vec::new();
    file.take(reference.bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 != reference.bytes
        || blake3::hash(&bytes).to_hex().as_str() != reference.digest
    {
        return None;
    }
    let export: ContinuationExport = serde_json::from_slice(&bytes).ok()?;
    let payload = ProviderContinuation::import(export).ok()?;
    payload.is_for(request).then_some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stored(
        directory: &Path,
        model: &str,
    ) -> Result<(NativePayloadRef, ProviderContinuation), Box<dyn std::error::Error>> {
        let payload = ProviderContinuation::import(ContinuationExport {
            protocol: "responses".into(),
            provider_id: Some("openai".into()),
            model_id: model.into(),
            data: json!([{"type": "compaction", "encrypted_content": "opaque"}]),
        })?;
        let bytes = serde_json::to_vec(&payload.export())?;
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let path = format!("artifacts/native-{digest}.bin");
        std::fs::create_dir_all(directory.join("artifacts"))?;
        std::fs::write(directory.join(&path), &bytes)?;
        Ok((
            NativePayloadRef {
                path,
                digest,
                bytes: bytes.len() as u64,
            },
            payload,
        ))
    }

    #[test]
    fn payload_reloads_only_for_its_model_and_only_when_intact(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let (reference, payload) = stored(directory.path(), "gpt-5")?;
        let request = |model: &str| CompletionRequest {
            provider_id: Some("openai".into()),
            model_id: model.into(),
            ..Default::default()
        };
        assert!(
            read_native_payload(directory.path(), &reference, &request("gpt-5"))
                .is_some_and(|loaded| loaded == payload)
        );
        assert!(read_native_payload(directory.path(), &reference, &request("gpt-4")).is_none());
        std::fs::write(directory.path().join(&reference.path), b"tampered")?;
        assert!(read_native_payload(directory.path(), &reference, &request("gpt-5")).is_none());
        Ok(())
    }
}
