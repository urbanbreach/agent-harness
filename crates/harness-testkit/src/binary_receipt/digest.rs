use super::BinaryReceiptError;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn verify_binary_digest(
    field: &str,
    path: &Path,
    expected: &str,
) -> Result<(), BinaryReceiptError> {
    let actual = sha256sum(path)?;
    if actual != expected {
        return Err(BinaryReceiptError::DigestMismatch {
            field: field.to_owned(),
            path: path.to_path_buf(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

fn sha256sum(path: &Path) -> Result<String, BinaryReceiptError> {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|source| BinaryReceiptError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(BinaryReceiptError::DigestCommand {
            path: path.to_path_buf(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .filter(|digest| digest.len() == 64)
        .map(str::to_owned)
        .ok_or_else(|| BinaryReceiptError::DigestCommand {
            path: PathBuf::from(path),
            message: "sha256sum returned no 64-character digest".to_owned(),
        })
}
