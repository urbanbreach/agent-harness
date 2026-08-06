use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::AttachmentError;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_TEMP_ARTIFACTS: AtomicUsize = AtomicUsize::new(0);

pub struct TempArtifact {
    root: PathBuf,
    path: PathBuf,
}

pub(crate) struct PartialPayload {
    bytes: Vec<u8>,
}

impl PartialPayload {
    pub(crate) fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub(crate) fn bytes_mut(&mut self) -> &mut Vec<u8> {
        &mut self.bytes
    }

    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

impl Drop for PartialPayload {
    fn drop(&mut self) {
        self.bytes.fill(0);
        self.bytes.clear();
    }
}

impl TempArtifact {
    pub fn new(prefix: &str, contents: &[u8]) -> Result<Self, AttachmentError> {
        let root = unique_root(prefix)?;
        let path = root.join("payload");
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| AttachmentError::TempWrite)?;
            file.write_all(contents)
                .map_err(|_| AttachmentError::TempWrite)
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&root);
            return Err(error);
        }
        ACTIVE_TEMP_ARTIFACTS.fetch_add(1, Ordering::AcqRel);
        Ok(Self { root, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root_path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempArtifact {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir_all(&self.root);
        ACTIVE_TEMP_ARTIFACTS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn active_temp_artifacts() -> usize {
    ACTIVE_TEMP_ARTIFACTS.load(Ordering::Acquire)
}

fn unique_root(prefix: &str) -> Result<PathBuf, AttachmentError> {
    let safe_prefix: String = prefix
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(32)
        .collect();
    let safe_prefix = if safe_prefix.is_empty() {
        "attachment".to_owned()
    } else {
        safe_prefix
    };
    for _ in 0..16 {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("harness-{safe_prefix}-{}-{id}", std::process::id()));
        match fs::create_dir(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(AttachmentError::TempCreate),
        }
    }
    Err(AttachmentError::TempCreate)
}
