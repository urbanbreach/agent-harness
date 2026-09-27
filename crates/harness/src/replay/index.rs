use super::*;
use harness_core::redact::{redact_in_place, Redactor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, io::Read, time::SystemTime};

const FILE: &str = "session-index.json";
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    entries: BTreeMap<String, Cached>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Cached {
    pub fingerprint: Fingerprint,
    pub catalog: SessionCatalogEntry,
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Fingerprint {
    journal: FileStamp,
    metadata: Option<FileStamp>,
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl FileStamp {
    fn read(path: &Path) -> std::io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::other("session files must be regular files"));
        }
        Ok(Self {
            bytes: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
}
impl Fingerprint {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let metadata = match FileStamp::read(&path.join(harness_core::proj::META_FILE_NAME)) {
            Ok(stamp) => Some(stamp),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            journal: FileStamp::read(&path.join("events.jsonl"))?,
            metadata,
        })
    }
    pub fn updated_ms(&self) -> u128 {
        self.journal
            .modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_millis())
    }
}

pub(super) fn load(root: &Path) -> BTreeMap<String, Cached> {
    let load = || -> Result<Index, Box<dyn std::error::Error>> {
        let file = harness_core::store::open_private_file(&root.join(FILE))?;
        if file.metadata()?.len() > MAX_BYTES {
            return Err("session index exceeds 16 MiB".into());
        }
        let index: Index = serde_json::from_reader(file.take(MAX_BYTES + 1))?;
        if index.version != 1 || index.entries.len() > 10_000 {
            return Err("unsupported or oversized session index".into());
        }
        Ok(index)
    };
    // An index is disposable. Only explicit rebuild writes; readers fall back to journals.
    load().map(|index| index.entries).unwrap_or_default()
}

pub(crate) fn rebuild(root: &Path, redactor: &dyn Redactor) -> Result<Value, String> {
    let mut entries = BTreeMap::new();
    let directories = fs::read_dir(root).map_err(|e| e.to_string())?;
    let mut scanned = 0;
    for dir in directories {
        let dir = dir.map_err(|e| e.to_string())?;
        if !dir.file_type().map_err(|e| e.to_string())?.is_dir()
            || !dir.path().join("events.jsonl").is_file()
        {
            continue;
        }
        if scanned == 10_000 {
            return Err("session catalog exceeds 10,000 entries".into());
        }
        scanned += 1;
        let before = Fingerprint::read(&dir.path()).map_err(|e| e.to_string())?;
        let row = inspect_session(&dir.path())?;
        let after = Fingerprint::read(&dir.path()).map_err(|e| e.to_string())?;
        if before != after {
            continue;
        }
        let name = dir
            .file_name()
            .into_string()
            .map_err(|_| "session directory name must be UTF-8")?;
        let mut catalog = serde_json::to_value(row.catalog).map_err(|e| e.to_string())?;
        redact_in_place(redactor, &mut catalog);
        entries.insert(
            name,
            Cached {
                fingerprint: after,
                catalog: serde_json::from_value(catalog).map_err(|e| e.to_string())?,
            },
        );
    }
    let count = entries.len();
    let value = serde_json::to_value(Index {
        version: 1,
        entries,
    })
    .map_err(|e| e.to_string())?;
    if harness_core::redact::has_unredacted_secret(redactor, &value) {
        return Err("session index secret scan failed".into());
    }
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("session index exceeds 16 MiB".into());
    }
    let path = root.join(FILE);
    harness_core::store::write_private_atomic(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(
        json!({"entry_count":count,"journals_scanned":scanned,"journals_opened":scanned,"index_path":path,"skipped_changing_journals":scanned-count}),
    )
}
