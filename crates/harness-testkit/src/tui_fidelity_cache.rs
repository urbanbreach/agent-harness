mod files;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tui_fidelity_compare::hash_bytes;
use crate::tui_fidelity_obligation::CaptureKey;
use crate::tui_fidelity_runner::{AdapterReceipt, ArtifactDigest};

const CACHE_SCHEMA: &str = "harness.tui-fidelity.reference-cache.v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCacheInputs {
    pub capture_key: CaptureKey,
    pub reference_source_digest: String,
    pub reference_binary_digest: String,
    pub scenario_digest: String,
    pub font_family: String,
    pub device_pixel_ratio: f64,
    pub terminal_capability: String,
    pub locale: String,
    pub browser_version: String,
    pub xterm_version: String,
    pub node_pty_version: String,
    pub comparator_schema: String,
}

impl ReferenceCacheInputs {
    pub fn digest(&self) -> Result<String, CacheError> {
        let bytes =
            serde_json::to_vec(self).map_err(|error| CacheError::Json(error.to_string()))?;
        hash_bytes(&bytes).map_err(|error| CacheError::Invalid(error.to_string()))
    }

    pub fn synthetic(seed: &str) -> Self {
        Self {
            capture_key: CaptureKey {
                scenario: seed.to_owned(),
                action: "synthetic".to_owned(),
                viewport: crate::tui_fidelity::Viewport { cols: 80, rows: 24 },
                terminal_tier: "truecolor".to_owned(),
                persona: "keyboard-first".to_owned(),
                theme: "default".to_owned(),
                media_mode: "none".to_owned(),
                failure_path: "none".to_owned(),
            },
            reference_source_digest: "a".repeat(64),
            reference_binary_digest: "b".repeat(64),
            scenario_digest: "c".repeat(64),
            font_family: "DejaVu Sans Mono".to_owned(),
            device_pixel_ratio: 1.0,
            terminal_capability: "xterm-256color".to_owned(),
            locale: "C.UTF-8".to_owned(),
            browser_version: "synthetic-browser".to_owned(),
            xterm_version: "synthetic-xterm".to_owned(),
            node_pty_version: "synthetic-node-pty".to_owned(),
            comparator_schema: "synthetic-comparator".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    pub path: PathBuf,
    pub artifact_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheError {
    Invalid(String),
    Io { path: PathBuf, detail: String },
    Json(String),
    Lock(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "reference cache: {detail}"),
            Self::Io { path, detail } => {
                write!(
                    formatter,
                    "reference cache I/O {}: {detail}",
                    path.display()
                )
            }
            Self::Json(detail) => write!(formatter, "reference cache JSON: {detail}"),
            Self::Lock(detail) => write!(formatter, "reference cache lock: {detail}"),
        }
    }
}

impl std::error::Error for CacheError {}

#[derive(Clone, Debug)]
pub struct ReferenceCache {
    root: PathBuf,
}

impl ReferenceCache {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self, key: &str) -> Result<Option<CacheEntry>, CacheError> {
        files::load(&self.root, key)
    }

    pub fn publish(&self, key: &str, source: &Path) -> Result<CacheEntry, CacheError> {
        files::publish(&self.root, key, source)
    }

    pub fn load_reference(&self, key: &str) -> Result<Option<AdapterReceipt>, CacheError> {
        let Some(entry) = self.load(key)? else {
            return Ok(None);
        };
        let receipt_path = entry.path.join("reference-receipt.json");
        let receipt: AdapterReceipt =
            serde_json::from_slice(&fs::read(&receipt_path).map_err(|error| CacheError::Io {
                path: receipt_path.clone(),
                detail: error.to_string(),
            })?)
            .map_err(|error| CacheError::Json(error.to_string()))?;
        for checkpoint in &receipt.checkpoints {
            for artifact in &checkpoint.artifacts {
                let path = Path::new(&artifact.path);
                if !path.starts_with(&entry.path) {
                    return Err(CacheError::Invalid(
                        "cached receipt artifact escapes cache entry".to_owned(),
                    ));
                }
            }
        }
        Ok(Some(receipt))
    }

    pub fn publish_reference(
        &self,
        key: &str,
        receipt: &AdapterReceipt,
    ) -> Result<CacheEntry, CacheError> {
        if let Some(entry) = self.load(key)? {
            return Ok(entry);
        }
        let temporary = tempfile::tempdir().map_err(|error| CacheError::Io {
            path: std::env::temp_dir(),
            detail: error.to_string(),
        })?;
        let mut cached = receipt.clone();
        for checkpoint in &mut cached.checkpoints {
            let relative_root = PathBuf::from("grok").join(checkpoint.name.as_str());
            let target_root = temporary.path().join(&relative_root);
            fs::create_dir_all(&target_root).map_err(|error| CacheError::Io {
                path: target_root.clone(),
                detail: error.to_string(),
            })?;
            let mut artifacts = Vec::with_capacity(checkpoint.artifacts.len());
            for artifact in &checkpoint.artifacts {
                let source = Path::new(&artifact.path);
                let name = source.file_name().ok_or_else(|| {
                    CacheError::Invalid("reference artifact has no file name".to_owned())
                })?;
                let target = target_root.join(name);
                fs::copy(source, &target).map_err(|error| CacheError::Io {
                    path: target.clone(),
                    detail: error.to_string(),
                })?;
                artifacts.push(ArtifactDigest {
                    path: self
                        .root
                        .join(key)
                        .join(&relative_root)
                        .join(name)
                        .display()
                        .to_string(),
                    sha256: artifact.sha256.clone(),
                });
            }
            checkpoint.artifacts = artifacts;
        }
        let receipt_path = temporary.path().join("reference-receipt.json");
        let bytes = serde_json::to_vec_pretty(&cached)
            .map_err(|error| CacheError::Json(error.to_string()))?;
        fs::write(&receipt_path, bytes).map_err(|error| CacheError::Io {
            path: receipt_path,
            detail: error.to_string(),
        })?;
        self.publish(key, temporary.path())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheManifest {
    schema_version: String,
    key: String,
    artifacts: Vec<CachedArtifact>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedArtifact {
    path: String,
    sha256: String,
}
