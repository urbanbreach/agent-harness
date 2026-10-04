use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
mod diff;
mod journal;
mod records;
pub use diff::*;
pub(crate) use journal::replace_file;
pub use journal::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditSource {
    AgentTool,
    External,
}
impl EditSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentTool => "agent_tool",
            Self::External => "external",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributedEdit {
    pub path: String,
    pub source: EditSource,
    pub content_sha256: String,
    pub mtime_unix_ms: Option<u64>,
}
impl AttributedEdit {
    pub fn one_line(&self) -> String {
        format!(
            "edit attribution: `{}` source={}",
            self.path,
            self.source.as_str()
        )
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAttributionSummary {
    pub agent_tool: usize,
    pub external: usize,
    #[serde(default)]
    pub drift: usize,
    pub total: usize,
}
impl EditAttributionSummary {
    pub const fn has_agent_tool(&self) -> bool {
        self.agent_tool > 0
    }
    pub fn one_line(&self) -> String {
        format!(
            "edit attribution: {} agent-tool, {} external, {} drift ({} total)",
            self.agent_tool, self.external, self.drift, self.total
        )
    }
    pub const fn has_external(&self) -> bool {
        self.external > 0 || self.drift > 0
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditAttributionTracker {
    entries: BTreeMap<String, AttributedEdit>,
    drifted: BTreeSet<String>,
}
impl EditAttributionTracker {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn record_agent_tool_edit(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> AttributedEdit {
        let path = normalize_path(path.as_ref());
        self.drifted.remove(&path);
        self.record(path, content, mtime, EditSource::AgentTool)
    }
    pub fn observe_external(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> AttributedEdit {
        let path = normalize_path(path.as_ref());
        let hash = sha256_hex(content);
        let source = if self
            .entries
            .get(&path)
            .is_some_and(|e| e.source == EditSource::AgentTool)
        {
            if self.entries[&path].content_sha256 == hash {
                EditSource::AgentTool
            } else {
                self.drifted.insert(path.clone());
                EditSource::External
            }
        } else {
            EditSource::External
        };
        let entry = AttributedEdit {
            path: path.clone(),
            source,
            content_sha256: hash,
            mtime_unix_ms: timestamp(mtime),
        };
        self.entries.insert(path, entry.clone());
        entry
    }
    fn record(
        &mut self,
        path: String,
        content: &[u8],
        mtime: Option<SystemTime>,
        source: EditSource,
    ) -> AttributedEdit {
        let entry = AttributedEdit {
            path: path.clone(),
            source,
            content_sha256: sha256_hex(content),
            mtime_unix_ms: timestamp(mtime),
        };
        self.entries.insert(path, entry.clone());
        entry
    }
    pub fn get(&self, path: impl AsRef<Path>) -> Option<&AttributedEdit> {
        self.entries.get(&normalize_path(path.as_ref()))
    }
    pub fn list(&self) -> Vec<&AttributedEdit> {
        self.entries.values().collect()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn is_drifted(&self, path: impl AsRef<Path>) -> bool {
        self.drifted.contains(&normalize_path(path.as_ref()))
    }
    pub fn summary(&self) -> EditAttributionSummary {
        let agent_tool = self
            .entries
            .values()
            .filter(|e| e.source == EditSource::AgentTool)
            .count();
        let drift = self.drifted.len();
        EditAttributionSummary {
            agent_tool,
            external: self.entries.len() - agent_tool - drift,
            drift,
            total: self.entries.len(),
        }
    }
}
fn timestamp(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_millis()).ok())
}
fn normalize_path(path: &Path) -> String {
    let path = path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned();
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path
    }
}
fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn content_digest12(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..12].into()
}
pub fn path_content_digest12(path: &Path) -> io::Result<String> {
    let mut file = open_regular(path)?;
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(hash.finalize().to_hex()[..12].into())
}
pub fn hash_path_contents(path: &Path) -> io::Result<(Vec<u8>, String)> {
    let mut bytes = Vec::new();
    open_regular(path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other("edit content exceeds 8 MiB"));
    }
    let hash = sha256_hex(&bytes);
    Ok((bytes, hash))
}
fn open_regular(path: &Path) -> io::Result<fs::File> {
    crate::store::open_private_file(path)
}
pub fn relative_path_key(workspace_root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(workspace_root).unwrap_or(path).into()
}

#[derive(Debug, Clone)]
pub struct MultiPathEditAttribution {
    pub summary: EditAttributionSummary,
    pub first_line: Option<String>,
    pub last_line: Option<String>,
    pub entries: Vec<AttributedEdit>,
    pub journal_path: String,
}
impl MultiPathEditAttribution {
    pub fn one_line(&self) -> String {
        self.summary.one_line()
    }
}
/// Explicit diagnostic fixture used by the preserved TUI tests, never by startup.
pub fn run_multi_path_edit_attribution_product(
    root: &Path,
) -> Result<MultiPathEditAttribution, EditAttributionError> {
    let mut journal = EditAttributionJournal::open(root)?;
    for name in ["agent.rs", "drift.rs"] {
        journal.record_agent_tool_edit(name, b"agent fixture\n", None)?;
    }
    journal.observe_external("drift.rs", b"external drift\n", None)?;
    journal.observe_external("external.rs", b"external fixture\n", None)?;
    let entries: Vec<_> = journal.list().into_iter().cloned().collect();
    Ok(MultiPathEditAttribution {
        summary: journal.summary(),
        first_line: entries.first().map(AttributedEdit::one_line),
        last_line: entries.last().map(AttributedEdit::one_line),
        entries,
        journal_path: journal.journal_path().display().to_string(),
    })
}
