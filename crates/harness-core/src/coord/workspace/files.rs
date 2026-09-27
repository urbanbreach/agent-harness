use super::*;
use std::{
    io::Read,
    path::{Component, PathBuf},
};
pub(super) struct File {
    pub bytes: Vec<u8>,
    pub mode: u32,
}
pub(super) struct Change {
    pub relative: String,
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub mode: u32,
    pub current: Option<File>,
}
pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(super) fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
pub(super) fn relative_path(value: &str) -> Result<&Path, CoordinatorError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || !path.components().all(|p| matches!(p, Component::Normal(_)))
    {
        return Err(CoordinatorError::Invalid(
            "invalid workspace snapshot path".into(),
        ));
    }
    Ok(path)
}
pub(super) fn read(path: &Path) -> Result<Option<File>, CoordinatorError> {
    let file = match crate::store::open_private_file(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let metadata = file.metadata()?;
    if metadata.len() > 8 * 1024 * 1024 {
        return Err(CoordinatorError::Invalid(
            "snapshot file exceeds 8 MiB".into(),
        ));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = u32::from(metadata.permissions().readonly());
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(CoordinatorError::Invalid(
            "snapshot file exceeds 8 MiB".into(),
        ));
    }
    Ok(Some(File { bytes, mode }))
}
pub(super) fn artifact(
    root: &Path,
    reference: &ArtifactRef,
    extension: &str,
    max: u64,
) -> Result<Vec<u8>, CoordinatorError> {
    if !valid_digest(&reference.digest)
        || reference.path != format!("artifacts/{}.{extension}", reference.digest)
    {
        return Err(CoordinatorError::Invalid(
            "invalid snapshot artifact reference".into(),
        ));
    }
    let bytes = crate::store::read_private_bytes(&root.join(&reference.path), max)?
        .ok_or_else(|| CoordinatorError::Invalid("snapshot artifact is missing".into()))?;
    if digest(&bytes) != reference.digest {
        return Err(CoordinatorError::Invalid(
            "snapshot artifact digest mismatch".into(),
        ));
    }
    Ok(bytes)
}
fn apply(path: &Path, bytes: Option<&[u8]>, mode: u32) -> Result<(), CoordinatorError> {
    crate::store::validate_private_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| CoordinatorError::Invalid("file has no parent".into()))?;
    if let Some(bytes) = bytes {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(mode & 0o777)
        };
        #[cfg(not(unix))]
        let permissions = {
            let mut p = fs::metadata(parent)?.permissions();
            p.set_readonly(mode != 0);
            p
        };
        crate::edit_attribution::replace_file(path, bytes, permissions)?;
    } else {
        match fs::remove_file(path) {
            Ok(()) => {
                #[cfg(unix)]
                fs::File::open(parent)?.sync_all()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
impl Change {
    pub fn restore(&self) -> Result<(), CoordinatorError> {
        let current = read(&self.path)?;
        if current.as_ref().map(|f| &f.bytes) != self.current.as_ref().map(|f| &f.bytes) {
            return Err(CoordinatorError::Invalid(format!(
                "revert conflict: {} changed during the revert",
                self.relative
            )));
        }
        apply(&self.path, self.before.as_deref(), self.mode)
    }
}
pub(super) fn rollback(changes: &[Change], error: CoordinatorError) -> CoordinatorError {
    let mut failure = None;
    for change in changes.iter().rev() {
        let result = read(&change.path).and_then(|observed| {
            let observed = observed.as_ref().map(|f| f.bytes.as_slice());
            let original = change.current.as_ref().map(|f| f.bytes.as_slice());
            if observed == original {
                return Ok(());
            }
            if observed != change.before.as_deref() {
                return Err(CoordinatorError::Invalid(
                    "file changed outside the revert; left unchanged".into(),
                ));
            }
            apply(
                &change.path,
                original,
                change.current.as_ref().map_or(0, |f| f.mode),
            )
        });
        if let Err(rollback) = result {
            failure.get_or_insert_with(|| {
                format!(
                    "{error}; revert rollback failed for {}: {rollback}",
                    change.relative
                )
            });
        }
    }
    failure.map_or(error, CoordinatorError::Invalid)
}
