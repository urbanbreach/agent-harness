use super::*;
use std::io::{Read, Seek, SeekFrom};
const FORMAT: &[u8] = b"harness-kernel-lock-v1\n";

pub(super) fn check_parent(root: &Path, dir: &Path) -> Result<(), EventStoreError> {
    let Some(metadata) = crate::proj::read_metadata_value(dir)? else {
        return Ok(());
    };
    let lineage = &metadata["harness_lineage"];
    if lineage["relationship"] != "task_child_session" {
        return Ok(());
    }
    let parent = lineage["parent_run_id"]
        .as_str()
        .or_else(|| lineage["parent_session_id"].as_str())
        .ok_or(EventStoreError::Invalid(
            "child journal has no parent identity",
        ))?;
    validate_session_id(parent)?;
    let parent = root.join(parent);
    reject_symlink(&parent)?;
    // Hold the child lock first. A concurrent parent resume must then reject this writer.
    let _parent = existing_writer_lock(&parent)?;
    Ok(())
}

pub(super) fn acquire(dir: &Path) -> Result<File, EventStoreError> {
    let path = dir.join(".writer.lock");
    reject_symlink(&path)?;
    let (mut file, created) = match private_options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => (file, true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            (private_options().read(true).write(true).open(path)?, false)
        }
        Err(e) => return Err(e.into()),
    };
    file.try_lock().map_err(std::io::Error::other)?;
    if !created {
        validate_owner(&file)?;
    }
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    file.write_all(FORMAT)?;
    file.sync_data()?;
    Ok(file)
}
pub(crate) fn existing_writer_lock(dir: &Path) -> Result<Option<File>, EventStoreError> {
    let path = dir.join(".writer.lock");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.is_file() => {
            return Err(EventStoreError::Invalid(
                "writer lock is not a regular file",
            ))
        }
        Ok(_) => {}
    }
    let file = File::open(path)?;
    file.try_lock().map_err(std::io::Error::other)?;
    validate_owner(&file)?;
    Ok(Some(file))
}
fn validate_owner(file: &File) -> Result<(), EventStoreError> {
    let mut contents = Vec::new();
    file.take(4097).read_to_end(&mut contents)?;
    if contents == FORMAT {
        return Ok(());
    }
    if contents.len() > 4096 {
        return Err(EventStoreError::Invalid("writer lock exceeds 4 KiB"));
    }
    let value: Option<serde_json::Value> = serde_json::from_slice(&contents).ok();
    let pid = value
        .as_ref()
        .and_then(|v| v.get("pid").unwrap_or(v).as_u64())
        .and_then(|p| u32::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or(EventStoreError::Invalid(
            "legacy writer ownership is unknown; refusing exclusive access",
        ))?;
    #[cfg(target_os = "linux")]
    {
        if !Path::new("/proc/self").is_dir() {
            return Err(EventStoreError::Invalid(
                "legacy writer process inspection is unavailable",
            ));
        }
        if Path::new("/proc").join(pid.to_string()).try_exists()? {
            return Err(EventStoreError::Invalid(
                "legacy session writer is still alive",
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        Err(EventStoreError::Invalid(
            "legacy writer process inspection is unavailable on this platform",
        ))
    }
}
