use harness::UnwrapOrAbort;
use std::path::PathBuf;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_abort()
        .parent()
        .unwrap_or_abort()
        .to_path_buf()
}
