mod cleanup;
mod external_editor;
mod ingest;
mod limits;
mod mime;
mod preview;

pub use cleanup::{active_temp_artifacts, TempArtifact};
pub use external_editor::{EditorCommand, ExternalEditor};
pub use ingest::{Attachment, AttachmentIngestor, AttachmentPolicy, AttachmentSource};
pub use limits::{
    AttachmentLimits as Limits, ImageDimensions, MAX_ATTACHMENT_BYTES, MAX_COMPRESSION_RATIO,
    MAX_DECOMPRESSED_BYTES, MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS, MAX_IMAGE_WIDTH,
};
pub use mime::MimeKind;
pub use preview::{build_preview, Preview};

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentError {
    RootUnavailable,
    PathEscape,
    NotRegularFile,
    Unreadable,
    Io { operation: &'static str },
    AllocationLimit,
    SizeLimit { observed: u64, limit: u64 },
    DecompressionLimit { observed: u64, limit: u64 },
    DimensionLimit { width: u32, height: u32 },
    MimeRejected { mime: MimeKind },
    Cancelled,
    TempCreate,
    TempWrite,
    EditorSpawn,
    EditorWait,
    EditorNonZero { code: Option<i32> },
}

impl fmt::Display for AttachmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootUnavailable => formatter.write_str("attachment workspace root unavailable"),
            Self::PathEscape => formatter.write_str("attachment path rejected: <redacted-path>"),
            Self::NotRegularFile => formatter.write_str("attachment source is not a regular file"),
            Self::Unreadable => formatter.write_str("attachment source is unreadable"),
            Self::Io { operation } => write!(formatter, "attachment I/O failed while {operation}"),
            Self::AllocationLimit => formatter.write_str("attachment allocation limit exceeded"),
            Self::SizeLimit { observed, limit } => {
                write!(
                    formatter,
                    "attachment size {observed} exceeds limit {limit}"
                )
            }
            Self::DecompressionLimit { observed, limit } => write!(
                formatter,
                "attachment decompressed size {observed} exceeds limit {limit}"
            ),
            Self::DimensionLimit { width, height } => {
                write!(
                    formatter,
                    "attachment dimensions {width}x{height} exceed limits"
                )
            }
            Self::MimeRejected { mime } => {
                write!(formatter, "unsupported attachment MIME: {}", mime.as_str())
            }
            Self::Cancelled => formatter.write_str("attachment operation cancelled"),
            Self::TempCreate => {
                formatter.write_str("attachment temporary storage could not be created")
            }
            Self::TempWrite => {
                formatter.write_str("attachment temporary storage could not be written")
            }
            Self::EditorSpawn => formatter.write_str("external editor could not be started"),
            Self::EditorWait => formatter.write_str("external editor status could not be read"),
            Self::EditorNonZero { code } => match code {
                Some(code) => write!(
                    formatter,
                    "external editor exited unsuccessfully (status {code})"
                ),
                None => formatter.write_str("external editor exited unsuccessfully"),
            },
        }
    }
}

impl std::error::Error for AttachmentError {}

pub(crate) fn io_error(operation: &'static str, error: &io::Error) -> AttachmentError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        AttachmentError::Unreadable
    } else {
        AttachmentError::Io { operation }
    }
}

pub fn redact_path(_path: &Path) -> &'static str {
    "<redacted-path>"
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn check(&self) -> Result<(), AttachmentError> {
        if self.is_cancelled() {
            Err(AttachmentError::Cancelled)
        } else {
            Ok(())
        }
    }
}
