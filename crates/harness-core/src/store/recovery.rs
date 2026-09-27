use super::*;
use std::io::{Seek, SeekFrom};

pub(super) fn preserve_tail(
    path: &Path,
    file: &File,
    valid_length: u64,
) -> Result<(), EventStoreError> {
    let prefix = JournalReader::open(path, valid_length)?.collect::<Result<Vec<_>, _>>()?;
    crate::proj::checked_history(&prefix).map_err(|_| {
        EventStoreError::Invalid("complete journal records are invalid; no repair was made")
    })?;
    let parent = path
        .parent()
        .ok_or(EventStoreError::Invalid("journal has no parent"))?;
    let mut source = File::open(path)?;
    source.seek(SeekFrom::Start(valid_length))?;
    let mut backup = tempfile::Builder::new()
        .prefix(".recovery-tail-")
        .tempfile_in(parent)?;
    std::io::copy(&mut source, &mut backup)?;
    backup.as_file().sync_all()?;
    let (_file, _path) = backup.keep().map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    file.set_len(valid_length)?;
    file.sync_all()?;
    Ok(())
}
