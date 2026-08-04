use serde::{Deserialize, Serialize};

use super::grapheme::GraphemeCluster;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AtomId(u64);

impl AtomId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileMentionId(u64);

impl FileMentionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttachmentId(u64);

impl AttachmentId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorBoundary {
    pub before: bool,
    pub after: bool,
}

impl CursorBoundary {
    pub const fn atomic() -> Self {
        Self {
            before: true,
            after: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum AtomKind {
    Text(GraphemeCluster),
    Newline,
    FileMention(FileMentionId),
    Attachment(AttachmentId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposerAtom {
    pub id: AtomId,
    pub kind: AtomKind,
    pub display_width: u16,
    pub cursor_boundary: CursorBoundary,
}

impl ComposerAtom {
    pub fn text(id: u64, cluster: GraphemeCluster) -> Self {
        Self::from_kind(AtomId::new(id), AtomKind::Text(cluster))
    }

    pub fn newline(id: u64) -> Self {
        Self::from_kind(AtomId::new(id), AtomKind::Newline)
    }

    pub fn file_mention(id: u64, mention: FileMentionId) -> Self {
        Self::from_kind(AtomId::new(id), AtomKind::FileMention(mention))
    }

    pub fn attachment(id: u64, attachment: AttachmentId) -> Self {
        Self::from_kind(AtomId::new(id), AtomKind::Attachment(attachment))
    }

    pub(crate) fn from_kind(id: AtomId, kind: AtomKind) -> Self {
        let display_width = match &kind {
            AtomKind::Text(cluster) => cluster.display_width(),
            AtomKind::Newline | AtomKind::FileMention(_) | AtomKind::Attachment(_) => 0,
        };
        Self {
            id,
            kind,
            display_width,
            cursor_boundary: CursorBoundary::atomic(),
        }
    }
}
