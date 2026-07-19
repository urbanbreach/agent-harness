//! Atomic load/save for the durable team mailbox journal.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::team_registry::{TeamMessage, TeamRecord};

use super::{TeamMailboxJournalError, STORE_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TeamMailboxDocument {
    pub version: u32,
    pub next_seq: u64,
    pub next_message_seq: u64,
    pub teams: Vec<TeamRecord>,
    pub mailboxes: Vec<MailboxBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MailboxBucket {
    pub team_id: String,
    pub messages: Vec<TeamMessage>,
}

impl TeamMailboxDocument {
    pub(super) fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            next_seq: 0,
            next_message_seq: 0,
            teams: Vec::new(),
            mailboxes: Vec::new(),
        }
    }
}

pub(super) fn load_or_empty(path: &Path) -> Result<TeamMailboxDocument, TeamMailboxJournalError> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let doc: TeamMailboxDocument =
                serde_json::from_str(&raw).map_err(|err| TeamMailboxJournalError::Parse {
                    path: path.display().to_string(),
                    detail: err.to_string(),
                })?;
            if doc.version != STORE_VERSION {
                return Err(TeamMailboxJournalError::UnsupportedVersion {
                    path: path.display().to_string(),
                    version: doc.version,
                });
            }
            Ok(doc)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(TeamMailboxDocument::empty()),
        Err(err) => Err(TeamMailboxJournalError::Read {
            path: path.display().to_string(),
            source: err,
        }),
    }
}

pub(super) fn save(path: &Path, doc: &TeamMailboxDocument) -> Result<(), TeamMailboxJournalError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| TeamMailboxJournalError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let body = serde_json::to_vec_pretty(doc).map_err(|err| TeamMailboxJournalError::Write {
        path: path.display().to_string(),
        source: io::Error::other(err),
    })?;
    let unique = now_unix_ms();
    let temp_path = path.with_extension(format!("json.tmp.{}.{}", std::process::id(), unique));
    write_file_atomically(&temp_path, path, &body)
}

fn write_file_atomically(
    temp_path: &Path,
    final_path: &Path,
    body: &[u8],
) -> Result<(), TeamMailboxJournalError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|source| TeamMailboxJournalError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    file.write_all(body)
        .and_then(|_| file.sync_all())
        .map_err(|source| TeamMailboxJournalError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    drop(file);
    fs::rename(temp_path, final_path).map_err(|source| TeamMailboxJournalError::Replace {
        path: final_path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
