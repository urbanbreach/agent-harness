use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::error::ComparatorError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactInputs {
    pub renderer: Vec<u8>,
    pub font: Vec<u8>,
    pub source: Vec<u8>,
    pub scenario: Vec<u8>,
    pub config: Vec<u8>,
}

impl ArtifactInputs {
    pub fn new(
        renderer: &[u8],
        font: &[u8],
        source: &[u8],
        scenario: &[u8],
        config: &[u8],
    ) -> Self {
        Self {
            renderer: renderer.to_vec(),
            font: font.to_vec(),
            source: source.to_vec(),
            scenario: scenario.to_vec(),
            config: config.to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactPaths {
    pub renderer: PathBuf,
    pub font: PathBuf,
    pub source: PathBuf,
    pub scenario: PathBuf,
    pub config: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactHashes {
    pub renderer: String,
    pub font: String,
    pub source: String,
    pub scenario: String,
    pub config: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleArtifact {
    pub kind: String,
    pub expected: String,
    pub observed: String,
}

pub fn hash_artifacts(inputs: &ArtifactInputs) -> Result<ArtifactHashes, ComparatorError> {
    Ok(ArtifactHashes {
        renderer: sha256(&inputs.renderer)?,
        font: sha256(&inputs.font)?,
        source: sha256(&inputs.source)?,
        scenario: sha256(&inputs.scenario)?,
        config: sha256(&inputs.config)?,
    })
}

pub fn hash_artifact_paths(paths: &ArtifactPaths) -> Result<ArtifactHashes, ComparatorError> {
    let read = |path: &Path| {
        std::fs::read(path).map_err(|error| ComparatorError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })
    };
    hash_artifacts(&ArtifactInputs {
        renderer: read(&paths.renderer)?,
        font: read(&paths.font)?,
        source: read(&paths.source)?,
        scenario: read(&paths.scenario)?,
        config: read(&paths.config)?,
    })
}

pub fn hash_bytes(bytes: &[u8]) -> Result<String, ComparatorError> {
    sha256(bytes)
}

pub fn verify_freshness(
    expected: &ArtifactHashes,
    observed: &ArtifactHashes,
) -> Result<(), ComparatorError> {
    let fields = [
        ("renderer", &expected.renderer, &observed.renderer),
        ("font", &expected.font, &observed.font),
        ("source", &expected.source, &observed.source),
        ("scenario", &expected.scenario, &observed.scenario),
        ("config", &expected.config, &observed.config),
    ];
    let stale = fields
        .into_iter()
        .filter(|(_, expected, observed)| expected != observed)
        .map(|(kind, expected, observed)| StaleArtifact {
            kind: kind.to_owned(),
            expected: expected.clone(),
            observed: observed.clone(),
        })
        .collect::<Vec<_>>();
    if stale.is_empty() {
        Ok(())
    } else {
        let stale_len = stale.len();
        Err(ComparatorError::Hashing { stale, stale_len })
    }
}

fn sha256(bytes: &[u8]) -> Result<String, ComparatorError> {
    let mut command = Command::new("sha256sum");
    command
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = command.spawn().map_err(|error| ComparatorError::Invalid {
        detail: format!("sha256sum unavailable: {error}"),
    })?;
    let Some(stdin) = child.stdin.as_mut() else {
        return Err(ComparatorError::Invalid {
            detail: "sha256sum stdin unavailable".to_owned(),
        });
    };
    stdin
        .write_all(bytes)
        .map_err(|error| ComparatorError::Invalid {
            detail: format!("sha256sum input failed: {error}"),
        })?;
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .map_err(|error| ComparatorError::Invalid {
            detail: format!("sha256sum wait failed: {error}"),
        })?;
    if !output.status.success() {
        return Err(ComparatorError::Invalid {
            detail: "sha256sum returned failure".to_owned(),
        });
    }
    let digest = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "sha256sum returned no digest".to_owned(),
        })?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ComparatorError::Invalid {
            detail: "sha256sum returned an invalid digest".to_owned(),
        });
    }
    Ok(digest)
}
