use std::fmt;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::tui_fidelity_compare::hash_bytes;

use super::CacheError;

static BINARY_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct ReferenceBinaryCache {
    root: PathBuf,
    source: PathBuf,
    digest: String,
}

impl ReferenceBinaryCache {
    pub fn new(
        root: impl AsRef<Path>,
        source: impl AsRef<Path>,
        digest: impl Into<String>,
    ) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            source: source.as_ref().to_path_buf(),
            digest: digest.into(),
        }
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn stage_for_worker(&self, worker_root: &Path) -> Result<PathBuf, CacheError> {
        validate_digest(&self.digest)?;
        validate_binary(&self.source, &self.digest)?;
        let cached = self.ensure_cached()?;
        let destination_root = worker_root.join("reference");
        fs::create_dir_all(&destination_root)
            .map_err(|error| io_error(&destination_root, error))?;
        let destination = destination_root.join(format!("grok-{}.bin", self.digest));
        if destination.exists() {
            validate_binary(&destination, &self.digest)?;
            return Ok(destination);
        }

        let temporary = destination_root.join(format!(
            ".tmp-grok-{}-{}-{}",
            self.digest,
            std::process::id(),
            BINARY_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::copy(&cached, &temporary).map_err(|error| io_error(&temporary, error))?;
        if let Err(error) = copy_executable_permissions(&cached, &temporary) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = validate_binary(&temporary, &self.digest) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        match fs::rename(&temporary, &destination) {
            Ok(()) => Ok(destination),
            Err(_error) if destination.exists() => {
                let _ = fs::remove_file(&temporary);
                validate_binary(&destination, &self.digest).map(|()| destination)
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(io_error(&destination, error))
            }
        }
    }

    fn ensure_cached(&self) -> Result<PathBuf, CacheError> {
        let cached = self.root.join(&self.digest).join("reference.bin");
        if cached.exists() {
            validate_binary(&cached, &self.digest)?;
            return Ok(cached);
        }
        fs::create_dir_all(&self.root).map_err(|error| io_error(&self.root, error))?;
        let locks = self.root.join(".locks");
        fs::create_dir_all(&locks).map_err(|error| io_error(&locks, error))?;
        let _lock = BinaryLock::acquire(&locks.join(format!("{}.lock", self.digest)))?;
        if cached.exists() {
            validate_binary(&cached, &self.digest)?;
            return Ok(cached);
        }

        let parent = cached
            .parent()
            .ok_or_else(|| CacheError::Invalid("binary cache path has no parent".to_owned()))?;
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        let temporary = self.root.join(format!(
            ".tmp-reference-{}-{}-{}",
            self.digest,
            std::process::id(),
            BINARY_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            fs::copy(&self.source, &temporary).map_err(|error| io_error(&temporary, error))?;
            copy_executable_permissions(&self.source, &temporary)?;
            validate_binary(&temporary, &self.digest)?;
            fs::rename(&temporary, &cached).map_err(|error| io_error(&cached, error))?;
            Ok(cached.clone())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

struct BinaryLock {
    path: PathBuf,
}

impl BinaryLock {
    fn acquire(path: &Path) -> Result<Self, CacheError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => {
                    return Ok(Self {
                        path: path.to_path_buf(),
                    })
                }
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

impl Drop for BinaryLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn validate_digest(digest: &str) -> Result<(), CacheError> {
    if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(CacheError::Invalid(
            "binary digest is not SHA-256".to_owned(),
        ))
    }
}

fn validate_binary(path: &Path, expected: &str) -> Result<(), CacheError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() {
        return Err(CacheError::Invalid(format!(
            "binary path is not a file: {}",
            path.display()
        )));
    }
    if !is_executable(&metadata) {
        return Err(CacheError::Invalid(format!(
            "binary is not executable: {}",
            path.display()
        )));
    }
    let actual = hash_bytes(&fs::read(path).map_err(|error| io_error(path, error))?)
        .map_err(|error| CacheError::Invalid(error.to_string()))?;
    if actual != expected {
        return Err(CacheError::Invalid(format!(
            "binary digest mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn copy_executable_permissions(source: &Path, destination: &Path) -> Result<(), CacheError> {
    #[cfg(unix)]
    {
        let source_mode = fs::metadata(source)
            .map_err(|error| io_error(source, error))?
            .permissions()
            .mode();
        let mut permissions = fs::metadata(destination)
            .map_err(|error| io_error(destination, error))?
            .permissions();
        permissions.set_mode(source_mode | 0o111);
        fs::set_permissions(destination, permissions)
            .map_err(|error| io_error(destination, error))?;
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

fn io_error(path: &Path, error: impl fmt::Display) -> CacheError {
    CacheError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}
