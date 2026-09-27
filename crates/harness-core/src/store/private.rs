use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

pub(crate) fn validate_private_path(path: &Path) -> io::Result<()> {
    for part in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(part) {
            Ok(m) if m.is_symlink() => {
                return Err(io::Error::other("private storage paths cannot be symlinks"))
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}
pub(crate) fn read_private_bytes(path: &Path, max: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match open_private_file(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() > max {
        return Err(io::Error::other(
            "private store is not a regular file within its size limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::other("private store exceeds its size limit"));
    }
    Ok(Some(bytes))
}
pub fn open_private_file(path: &Path) -> io::Result<File> {
    private_file(path, false)
}
pub fn open_private_append(path: &Path) -> io::Result<File> {
    private_file(path, true)
}
fn private_file(path: &Path, append: bool) -> io::Result<File> {
    validate_private_path(path)?;
    if append {
        super::create_private_dir(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
        )?;
    }
    #[cfg(unix)]
    let file = {
        use rustix::fs::{open, Mode, OFlags};
        let access = if append {
            OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND
        } else {
            OFlags::RDONLY
        };
        File::from(open(
            path,
            access | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )?)
    };
    #[cfg(not(unix))]
    let file = {
        fs::OpenOptions::new()
            .read(!append)
            .append(append)
            .create(append)
            .open(path)?
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("private store is not a regular file"));
    }
    #[cfg(unix)]
    if append {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}
/// Lock the containing directory, since atomic replacement changes the file inode.
pub(crate) fn lock_private_parent(path: &Path) -> io::Result<File> {
    validate_private_path(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    super::create_private_dir(parent)?;
    let lock = File::open(parent)?;
    lock.lock()?;
    Ok(lock)
}
