//! Silent, per-prompt snapshots of files touched by native reads and edits.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::event::{ActorKind, EventEnvelopeV1, EventV1};

use crate::redact::Redactor;

#[derive(Default, Debug)]
pub(crate) struct FileCheckpoints(Mutex<BTreeMap<String, ActiveCheckpoint>>);

#[derive(Debug)]
struct ActiveCheckpoint {
    request_id: String,
    before: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SavedFile {
    pub(crate) digest: String,
    pub(crate) content: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct SavedCheckpoint {
    request_id: String,
    pub(crate) before: BTreeMap<String, Option<SavedFile>>,
    pub(crate) after: BTreeMap<String, Option<SavedFile>>,
}

fn read_file(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn safe_file(bytes: Vec<u8>, redactor: &dyn Redactor) -> SavedFile {
    SavedFile {
        digest: crate::digest::digest12(&bytes),
        content: String::from_utf8(bytes).ok().filter(|text| {
            !text.contains('\0')
                && redactor.redact_text(text) == *text
                && json5::from_str::<serde_json::Value>(text)
                    .ok()
                    .is_none_or(|value| crate::redact::redact_value(redactor, &value) == value)
        }),
    }
}

impl FileCheckpoints {
    pub(crate) fn begin(&self, agent: &str, request_id: &str) {
        if let Ok(mut active) = self.0.lock() {
            active.insert(
                agent.to_string(),
                ActiveCheckpoint {
                    request_id: request_id.to_string(),
                    before: BTreeMap::new(),
                },
            );
        }
    }

    pub(crate) fn capture(&self, agent: &str, path: &Path, root: &Path) -> std::io::Result<()> {
        let root = root.canonicalize()?;
        // The caller has already resolved and authorized the path. Never snapshot external files.
        let Ok(relative) = path.strip_prefix(&root) else {
            return Ok(());
        };
        let mut active = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("checkpoint lock poisoned"))?;
        let Some(checkpoint) = active.get_mut(agent) else {
            return Ok(());
        };
        if let std::collections::btree_map::Entry::Vacant(entry) =
            checkpoint.before.entry(relative.to_path_buf())
        {
            entry.insert(read_file(path)?);
        }
        Ok(())
    }

    pub(crate) fn finish(
        &self,
        agent: &str,
        root: &Path,
        artifacts: &Path,
        redactor: &dyn Redactor,
    ) -> std::io::Result<()> {
        let checkpoint = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("checkpoint lock poisoned"))?
            .remove(agent);
        let Some(checkpoint) = checkpoint else {
            return Ok(());
        };
        let root = root.canonicalize()?;
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();
        for (relative, bytes) in checkpoint.before {
            let name = relative.to_string_lossy().replace('\\', "/");
            // Sensitive paths are omitted as well as sensitive contents.
            if redactor.redact_text(&name) != name
                || relative
                    .components()
                    .any(|part| part.as_os_str().to_string_lossy().starts_with(".env"))
            {
                continue;
            }
            if root
                .join(&relative)
                .canonicalize()
                .is_ok_and(|path| !path.starts_with(&root))
            {
                continue;
            }
            after.insert(
                name.clone(),
                read_file(&root.join(&relative))?.map(|bytes| safe_file(bytes, redactor)),
            );
            before.insert(name, bytes.map(|bytes| safe_file(bytes, redactor)));
        }
        let path = checkpoint_path(artifacts, &checkpoint.request_id)?;
        let saved = SavedCheckpoint {
            request_id: checkpoint.request_id,
            before,
            after,
        };
        let bytes = serde_json::to_vec(&saved).map_err(std::io::Error::other)?;
        std::fs::create_dir_all(artifacts.join("rewind"))?;
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, path)
    }
}

fn checkpoint_path(artifacts: &Path, request_id: &str) -> std::io::Result<PathBuf> {
    if request_id.is_empty()
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(std::io::Error::other("invalid checkpoint request id"));
    }
    Ok(artifacts.join("rewind").join(format!("{request_id}.json")))
}

impl SavedCheckpoint {
    fn merge(&mut self, later: Self) {
        for (path, before) in later.before {
            self.before.entry(path).or_insert(before);
        }
        self.after.extend(later.after);
    }
}

/// The journal supplies atomic fold/truncate operations. Original checkpoint artifacts
/// stay immutable; replaying these operations also recovers the tracker after restart.
pub(crate) fn load_restore_checkpoint(
    artifacts: &Path,
    events: &[EventEnvelopeV1],
    request_id: &str,
) -> std::io::Result<Option<SavedCheckpoint>> {
    let mut points: Vec<(u64, SavedCheckpoint)> = Vec::new();
    for event in events {
        match &event.payload {
            EventV1::UserMessageSubmitted(prompt)
                if matches!(event.actor.kind, ActorKind::User | ActorKind::Supervisor) =>
            {
                let id = prompt.request_id.as_str();
                let checkpoint = match std::fs::read(checkpoint_path(artifacts, id)?) {
                    Ok(bytes) => {
                        let saved: SavedCheckpoint =
                            serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
                        if saved.request_id != id {
                            return Err(std::io::Error::other("checkpoint request id mismatch"));
                        }
                        saved
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => SavedCheckpoint {
                        request_id: id.to_string(),
                        ..Default::default()
                    },
                    Err(error) => return Err(error),
                };
                points.push((event.seq, checkpoint));
            }
            EventV1::ConversationRewound(rewind) => {
                let split = points.partition_point(|(seq, _)| *seq < rewind.target_seq);
                let removed = points.split_off(split);
                if let Some((_, retained)) = points.last_mut() {
                    for (_, checkpoint) in removed {
                        retained.merge(checkpoint);
                    }
                }
            }
            EventV1::WorkspaceReverted(revert) if revert.failed_paths.is_empty() => {
                if let Some(index) = points
                    .iter()
                    .position(|(_, point)| point.request_id == revert.snapshot_request_id)
                {
                    points.truncate(index);
                }
            }
            _ => {}
        }
    }
    let Some(index) = points
        .iter()
        .position(|(_, point)| point.request_id == request_id)
    else {
        return Ok(None);
    };
    let mut merged = SavedCheckpoint::default();
    for (_, checkpoint) in points.into_iter().skip(index) {
        merged.merge(checkpoint);
    }
    Ok(Some(merged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn checkpoint_keeps_first_touch_and_final_state_without_secrets_or_untouched_files() {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let root = dir.path().canonicalize().unwrap_or_abort();
        let artifacts = root.join("artifacts");
        let path = root.join("edited.txt");
        let tracker = FileCheckpoints::default();
        let redactor = crate::redact::DefaultRedactor::default();
        std::fs::write(&path, "before").unwrap_or_abort();
        std::fs::write(root.join("untouched.txt"), "untouched").unwrap_or_abort();
        std::fs::write(
            root.join("credentials.json"),
            r#"{"api_key":"checkpoint-private-value"}"#,
        )
        .unwrap_or_abort();
        std::fs::write(root.join("image.bin"), [0, 255]).unwrap_or_abort();
        tracker.begin("agent", "prompt1");
        for name in ["edited.txt", "created.txt", "credentials.json", "image.bin"] {
            tracker
                .capture("agent", &root.join(name), &root)
                .unwrap_or_abort();
        }
        std::fs::write(&path, "middle").unwrap_or_abort();
        tracker.capture("agent", &path, &root).unwrap_or_abort();
        std::fs::write(&path, "after").unwrap_or_abort();
        std::fs::write(root.join("created.txt"), "new").unwrap_or_abort();
        tracker
            .finish("agent", &root, &artifacts, &redactor)
            .unwrap_or_abort();
        let raw = std::fs::read_to_string(artifacts.join("rewind/prompt1.json")).unwrap_or_abort();
        assert!(!raw.contains("checkpoint-private-value") && !raw.contains("untouched"));
        let saved: serde_json::Value = serde_json::from_str(&raw).unwrap_or_abort();
        assert_eq!(saved["before"]["edited.txt"]["content"], "before");
        assert_eq!(saved["after"]["edited.txt"]["content"], "after");
        assert!(saved["before"]["created.txt"].is_null());
        assert_eq!(saved["after"]["created.txt"]["content"], "new");
        assert!(saved["before"]["image.bin"]["content"].is_null());
        assert!(saved["before"]["credentials.json"]["content"].is_null());
        tracker.begin("agent", "prompt2");
        tracker.capture("agent", &path, &root).unwrap_or_abort();
        tracker
            .finish("agent", &root, &artifacts, &redactor)
            .unwrap_or_abort();
        let next: serde_json::Value = serde_json::from_slice(
            &std::fs::read(artifacts.join("rewind/prompt2.json")).unwrap_or_abort(),
        )
        .unwrap_or_abort();
        assert_eq!(next["before"]["edited.txt"]["content"], "after");
    }
}
