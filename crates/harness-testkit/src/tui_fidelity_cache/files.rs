use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::tui_fidelity_compare::hash_bytes;

use super::{CacheEntry, CacheError, CacheManifest, CachedArtifact, CACHE_SCHEMA};

static TEMP_ID: AtomicU64 = AtomicU64::new(1);

pub(super) fn load(root: &Path, key: &str) -> Result<Option<CacheEntry>, CacheError> {
    validate_key(key)?;
    let entry = root.join(key);
    if !entry.is_dir() {
        return Ok(None);
    }
    let manifest_path = entry.join("cache-manifest.json");
    let manifest: CacheManifest = serde_json::from_slice(&read(&manifest_path)?)
        .map_err(|error| CacheError::Json(error.to_string()))?;
    if manifest.schema_version != CACHE_SCHEMA || manifest.key != key {
        return Err(CacheError::Invalid(
            "cache manifest schema or key differs".to_owned(),
        ));
    }
    for artifact in &manifest.artifacts {
        let relative = safe_relative(&artifact.path)?;
        let observed = hash_bytes(&read(&entry.join(relative))?)
            .map_err(|error| CacheError::Invalid(error.to_string()))?;
        if observed != artifact.sha256 {
            return Err(CacheError::Invalid(format!(
                "artifact {} digest differs",
                artifact.path
            )));
        }
    }
    Ok(Some(CacheEntry {
        path: entry,
        artifact_count: manifest.artifacts.len(),
    }))
}

pub(super) fn publish(root: &Path, key: &str, source: &Path) -> Result<CacheEntry, CacheError> {
    validate_key(key)?;
    fs::create_dir_all(root).map_err(|error| io_error(root, error))?;
    let locks = root.join(".locks");
    fs::create_dir_all(&locks).map_err(|error| io_error(&locks, error))?;
    let _lock = KeyLock::acquire(&locks.join(format!("{key}.lock")))?;
    if let Some(entry) = load(root, key)? {
        return Ok(entry);
    }
    if !source.is_dir() {
        return Err(CacheError::Invalid(format!(
            "capture source {} is not a directory",
            source.display()
        )));
    }
    let temporary = root.join(format!(
        ".tmp-{key}-{}-{}",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&temporary).map_err(|error| io_error(&temporary, error))?;
    let result = publish_atomic(root, key, source, &temporary);
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn publish_atomic(
    root: &Path,
    key: &str,
    source: &Path,
    temporary: &Path,
) -> Result<CacheEntry, CacheError> {
    copy_tree(source, temporary)?;
    let artifacts = artifact_manifest(temporary)?;
    let manifest = CacheManifest {
        schema_version: CACHE_SCHEMA.to_owned(),
        key: key.to_owned(),
        artifacts,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| CacheError::Json(error.to_string()))?;
    let manifest_path = temporary.join("cache-manifest.json");
    fs::write(&manifest_path, bytes).map_err(|error| io_error(&manifest_path, error))?;
    let destination = root.join(key);
    fs::rename(temporary, &destination).map_err(|error| io_error(&destination, error))?;
    Ok(CacheEntry {
        path: destination,
        artifact_count: manifest.artifacts.len(),
    })
}

struct KeyLock {
    path: PathBuf,
}

impl KeyLock {
    fn acquire(path: &Path) -> Result<Self, CacheError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Ok(Self { path: path.into() }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        return Err(CacheError::Lock(format!(
                            "timed out acquiring {}",
                            path.display()
                        )));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(io_error(path, error)),
            }
        }
    }
}

impl Drop for KeyLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), CacheError> {
    for entry in fs::read_dir(source).map_err(|error| io_error(source, error))? {
        let entry = entry.map_err(|error| io_error(source, error))?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir(&target).map_err(|error| io_error(&target, error))?;
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|error| io_error(&target, error))?;
        }
    }
    Ok(())
}

fn artifact_manifest(root: &Path) -> Result<Vec<CachedArtifact>, CacheError> {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| io_error(&directory, error))? {
            let entry = entry.map_err(|error| io_error(&directory, error))?;
            if entry.path().is_dir() {
                pending.push(entry.path());
            } else {
                paths.insert(entry.path());
            }
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| CacheError::Invalid(error.to_string()))?;
            let sha256 = hash_bytes(&read(&path)?)
                .map_err(|error| CacheError::Invalid(error.to_string()))?;
            Ok(CachedArtifact {
                path: relative.to_string_lossy().into_owned(),
                sha256,
            })
        })
        .collect()
}

fn validate_key(key: &str) -> Result<(), CacheError> {
    if key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(CacheError::Invalid("cache key is not SHA-256".to_owned()))
    }
}

fn safe_relative(path: &str) -> Result<&Path, CacheError> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|part| part == std::path::Component::ParentDir)
    {
        Err(CacheError::Invalid(
            "cache artifact path escapes entry".to_owned(),
        ))
    } else {
        Ok(path)
    }
}

fn read(path: &Path) -> Result<Vec<u8>, CacheError> {
    fs::read(path).map_err(|error| io_error(path, error))
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> CacheError {
    CacheError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}
