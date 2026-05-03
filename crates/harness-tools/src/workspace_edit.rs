use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use harness_core::edit::hashline::LineAnchor;
use harness_core::tool::{ToolContext, ToolError};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    content_hash: u64,
}

#[derive(Debug, Clone)]
struct FileReadStamp {
    fingerprint: FileFingerprint,
    hashline_anchors: Option<Vec<LineAnchor>>,
}

#[derive(Default)]
struct FileReadRegistry {
    reads: BTreeMap<(String, String), FileReadStamp>,
}

static FILE_READ_REGISTRY: OnceLock<Mutex<FileReadRegistry>> = OnceLock::new();

pub(crate) fn record_file_read(ctx: &ToolContext, resolved_path: &Path) -> Result<(), ToolError> {
    record_file_read_with_anchors(ctx, resolved_path, None)
}

pub(crate) fn record_file_hashline_read(
    ctx: &ToolContext,
    resolved_path: &Path,
    anchors: Vec<LineAnchor>,
) -> Result<(), ToolError> {
    record_file_read_with_anchors(ctx, resolved_path, Some(anchors))
}

pub(crate) fn recent_hashline_anchors(
    ctx: &ToolContext,
    resolved_path: &Path,
) -> Option<Vec<LineAnchor>> {
    let key = file_read_key(ctx, resolved_path);
    let prior = {
        let registry = file_read_registry();
        let state = lock_mutex(registry);
        state.reads.get(&key).cloned()
    }?;

    let anchors = prior.hashline_anchors?;
    let current = file_read_stamp_from_metadata(resolved_path).ok()?;
    (current.fingerprint == prior.fingerprint).then_some(anchors)
}

fn record_file_read_with_anchors(
    ctx: &ToolContext,
    resolved_path: &Path,
    anchors: Option<Vec<LineAnchor>>,
) -> Result<(), ToolError> {
    let stamp = file_read_stamp_from_metadata(resolved_path)?;
    let key = file_read_key(ctx, resolved_path);
    let registry = file_read_registry();
    let mut state = lock_mutex(registry);
    state.reads.insert(
        key,
        FileReadStamp {
            hashline_anchors: anchors,
            ..stamp
        },
    );
    Ok(())
}

fn file_read_registry() -> &'static Mutex<FileReadRegistry> {
    FILE_READ_REGISTRY.get_or_init(|| Mutex::new(FileReadRegistry::default()))
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn file_read_key(ctx: &ToolContext, resolved_path: &Path) -> (String, String) {
    (ctx.run_id.clone(), resolved_path.display().to_string())
}

fn file_read_stamp_from_metadata(resolved_path: &Path) -> Result<FileReadStamp, ToolError> {
    std::fs::metadata(resolved_path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            ToolError::Execution(format!("File {} not found", resolved_path.display()))
        }
        _ => ToolError::Execution(format!(
            "failed to inspect file freshness for {}: {err}",
            resolved_path.display()
        )),
    })?;
    let content = std::fs::read(resolved_path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read file freshness digest for {}: {err}",
            resolved_path.display()
        ))
    })?;

    Ok(FileReadStamp {
        fingerprint: FileFingerprint {
            content_hash: content_fingerprint(&content),
        },
        hashline_anchors: None,
    })
}

fn content_fingerprint(content: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}
