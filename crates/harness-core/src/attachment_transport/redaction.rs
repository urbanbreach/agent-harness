use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RedactedContentRef(String);

impl RedactedContentRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn redacted_content_ref(path: Option<&Path>, bytes: &[u8]) -> RedactedContentRef {
    let path_hash = hash_hex(path_bytes(path));
    let content_hash = hash_hex(bytes);
    RedactedContentRef(format!(
        "attachment:v1:path-{path_hash}:content-{content_hash}"
    ))
}

fn path_bytes(path: Option<&Path>) -> Vec<u8> {
    match path {
        Some(path) => path.to_string_lossy().as_bytes().to_vec(),
        None => b"<no-source-path>".to_vec(),
    }
}

fn hash_hex(bytes: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
