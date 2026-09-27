use super::*;
use std::{
    fs,
    io::Write,
    path::{Component, Path},
};

pub(crate) fn copy(
    events: &[EventEnvelopeV1],
    source: &Path,
    child: &Path,
) -> Result<usize, SessionLineageError> {
    let mut references: BTreeMap<String, (Option<String>, Option<u64>)> = BTreeMap::new();
    let mut add = |path: &str, digest: Option<&str>, bytes| -> Result<(), SessionLineageError> {
        if path.len() > 4096
            || path.chars().any(char::is_control)
            || references.len() >= 1024 && !references.contains_key(path)
        {
            return Err(SessionLineageError::Invalid(
                "artifact inventory exceeds its bounds".into(),
            ));
        }
        let entry = references.entry(path.into()).or_default();
        if entry.0.as_deref().zip(digest).is_some_and(|(a, b)| a != b)
            || entry.1.zip(bytes).is_some_and(|(a, b)| a != b)
        {
            return Err(SessionLineageError::Invalid(format!(
                "conflicting artifact reference: {path}"
            )));
        }
        if let Some(digest) = digest {
            entry.0 = Some(digest.into());
        }
        if bytes.is_some() {
            entry.1 = bytes;
        }
        Ok(())
    };
    for event in events {
        match &event.payload {
            EventV1::ArtifactWritten(e) => add(&e.path, Some(&e.digest), Some(e.bytes))?,
            EventV1::ToolCallFinished(e) => {
                for artifact in e.metadata.iter().flat_map(|m| &m.artifact_refs) {
                    add(&artifact.path, artifact.digest.as_deref(), None)?;
                }
            }
            EventV1::EditApplied(e) => {
                if let Some(path) = &e.diff_rel_path {
                    add(path, e.diff_digest.as_deref(), None)?;
                }
            }
            EventV1::CompactionWritten(e) => add(
                &e.artifact_path,
                e.artifact_digest.as_deref(),
                Some(e.artifact_bytes),
            )?,
            EventV1::WorkspaceSnapshot(e) => add(&e.artifact_path, Some(&e.artifact_digest), None)?,
            _ => {}
        }
    }
    let mut total = 0;
    for (path, (digest, bytes)) in &references {
        let relative = Path::new(path);
        let mut parts = relative.components();
        if parts.next() != Some(Component::Normal("artifacts".as_ref()))
            || parts.clone().next().is_none()
            || !parts.all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(SessionLineageError::Invalid(
                "artifact path must stay under artifacts/".into(),
            ));
        }
        let body = crate::store::read_private_bytes(&source.join(relative), 16 * 1024 * 1024)?
            .ok_or_else(|| SessionLineageError::Invalid("referenced artifact is missing".into()))?;
        let count = body.len() as u64;
        total += count;
        if total > 64 * 1024 * 1024 {
            return Err(SessionLineageError::Invalid(
                "artifacts exceed 64 MiB in total".into(),
            ));
        }
        if crate::redact::DefaultRedactor::default()
            .secret_finding_count(&String::from_utf8_lossy(&body))
            > 0
        {
            return Err(SessionLineageError::Invalid(
                "artifact contains a credential; no session was published".into(),
            ));
        }
        let actual = blake3::hash(&body).to_hex().to_string();
        if bytes.is_some_and(|n| n != count)
            || digest.as_ref().is_some_and(|d| {
                d.len() < 12
                    || d.len() > 64
                    || !d.bytes().all(|c| c.is_ascii_hexdigit())
                    || !actual.starts_with(&d.to_ascii_lowercase())
            })
        {
            return Err(SessionLineageError::Invalid(format!(
                "artifact byte count or digest mismatch: {path}"
            )));
        }
        let destination = child.join(relative);
        if let Some(existing) = crate::store::read_private_bytes(&destination, 16 * 1024 * 1024)? {
            if existing != body {
                return Err(SessionLineageError::Invalid(format!(
                    "destination artifact differs: {path}"
                )));
            }
            continue;
        }
        let parent = destination
            .parent()
            .ok_or_else(|| SessionLineageError::Invalid("artifact has no parent".into()))?;
        crate::store::create_private_dir(parent)?;
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        output.write_all(&body)?;
        output.as_file().sync_all()?;
        output
            .persist_noclobber(&destination)
            .map_err(|e| e.error)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(references.len())
}
