use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use super::cleanup::PartialPayload;
use super::limits::{AttachmentLimits, READ_CHUNK_BYTES};
use super::mime::{resolve, MimeKind};
use super::preview::{build_preview, Preview};
use super::{io_error, AttachmentError, CancellationToken};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentSource {
    File,
    Clipboard,
}

pub struct Attachment {
    source: AttachmentSource,
    canonical_path: Option<PathBuf>,
    mime: MimeKind,
    bytes: Vec<u8>,
    preview: Preview,
}

impl Attachment {
    pub fn source(&self) -> AttachmentSource {
        self.source
    }

    pub fn canonical_path(&self) -> Option<&Path> {
        self.canonical_path.as_deref()
    }

    pub fn mime(&self) -> MimeKind {
        self.mime
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }
}

#[derive(Clone, Debug)]
pub struct AttachmentPolicy {
    workspace_root: PathBuf,
    allowed_roots: Vec<PathBuf>,
    limits: AttachmentLimits,
}

impl AttachmentPolicy {
    pub fn new(workspace_root: &Path) -> Result<Self, AttachmentError> {
        let workspace_root =
            fs::canonicalize(workspace_root).map_err(|_| AttachmentError::RootUnavailable)?;
        Ok(Self {
            allowed_roots: vec![workspace_root.clone()],
            workspace_root,
            limits: AttachmentLimits::default(),
        })
    }

    pub fn with_limits(self, limits: AttachmentLimits) -> Self {
        Self { limits, ..self }
    }

    pub fn allow_root(mut self, root: &Path) -> Result<Self, AttachmentError> {
        let root = fs::canonicalize(root).map_err(|_| AttachmentError::RootUnavailable)?;
        self.allowed_roots.push(root);
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct AttachmentIngestor {
    policy: AttachmentPolicy,
}

impl AttachmentIngestor {
    pub fn new(policy: AttachmentPolicy) -> Self {
        Self { policy }
    }

    pub fn ingest_file(
        &self,
        path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Attachment, AttachmentError> {
        self.ingest_file_with_observer(path, cancellation, |_, _| {})
    }

    pub fn ingest_file_with_observer<F>(
        &self,
        path: &Path,
        cancellation: &CancellationToken,
        mut observer: F,
    ) -> Result<Attachment, AttachmentError>
    where
        F: FnMut(usize, &CancellationToken),
    {
        cancellation.check()?;
        let canonical_path = self.canonical_file(path)?;
        let bytes = read_bounded(
            &canonical_path,
            &self.policy.limits,
            cancellation,
            &mut observer,
        )?;
        self.finish(
            AttachmentSource::File,
            Some(canonical_path),
            bytes,
            cancellation,
            None,
        )
    }

    pub fn ingest_clipboard(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Attachment, AttachmentError> {
        self.ingest_clipboard_with_mime(bytes, cancellation, None)
    }

    pub fn ingest_clipboard_with_mime(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
        mime_hint: Option<MimeKind>,
    ) -> Result<Attachment, AttachmentError> {
        let mut observer = |_: usize, _: &CancellationToken| {};
        let bytes = copy_bounded(bytes, &self.policy.limits, cancellation, &mut observer)?;
        self.finish(
            AttachmentSource::Clipboard,
            None,
            bytes,
            cancellation,
            mime_hint,
        )
    }

    fn canonical_file(&self, path: &Path) -> Result<PathBuf, AttachmentError> {
        let canonical = fs::canonicalize(path).map_err(|_| AttachmentError::PathEscape)?;
        let inside_workspace = canonical.starts_with(&self.policy.workspace_root);
        let inside_allowed = self
            .policy
            .allowed_roots
            .iter()
            .any(|root| canonical.starts_with(root));
        if !inside_workspace || !inside_allowed {
            return Err(AttachmentError::PathEscape);
        }
        let metadata = fs::metadata(&canonical)
            .map_err(|error| io_error("reading source metadata", &error))?;
        if !metadata.is_file() {
            return Err(AttachmentError::NotRegularFile);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o444 == 0 {
                return Err(AttachmentError::Unreadable);
            }
        }
        Ok(canonical)
    }

    fn finish(
        &self,
        source: AttachmentSource,
        canonical_path: Option<PathBuf>,
        bytes: Vec<u8>,
        cancellation: &CancellationToken,
        mime_hint: Option<MimeKind>,
    ) -> Result<Attachment, AttachmentError> {
        cancellation.check()?;
        let mime = resolve(&bytes, mime_hint)?;
        self.policy.limits.validate(mime, &bytes)?;
        if !mime.is_allowed() {
            return Err(AttachmentError::MimeRejected { mime });
        }
        let preview = build_preview(&bytes, mime, &self.policy.limits)?;
        Ok(Attachment {
            source,
            canonical_path,
            mime,
            bytes,
            preview,
        })
    }
}

fn read_bounded<F>(
    path: &Path,
    limits: &AttachmentLimits,
    cancellation: &CancellationToken,
    observer: &mut F,
) -> Result<Vec<u8>, AttachmentError>
where
    F: FnMut(usize, &CancellationToken),
{
    let mut file = File::open(path).map_err(|error| io_error("opening source", &error))?;
    let size = file
        .metadata()
        .map_err(|error| io_error("reading source metadata", &error))?
        .len();
    if size > limits.max_bytes {
        return Err(AttachmentError::SizeLimit {
            observed: size,
            limit: limits.max_bytes,
        });
    }
    let capacity = usize::try_from(size).map_err(|_| AttachmentError::AllocationLimit)?;
    let mut partial = PartialPayload::new();
    partial
        .bytes_mut()
        .try_reserve(capacity)
        .map_err(|_| AttachmentError::AllocationLimit)?;
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        cancellation.check()?;
        let read = file
            .read(&mut chunk)
            .map_err(|error| io_error("reading source", &error))?;
        if read == 0 {
            break;
        }
        partial.bytes_mut().extend_from_slice(&chunk[..read]);
        if u64::try_from(partial.len()).map_err(|_| AttachmentError::AllocationLimit)?
            > limits.max_bytes
        {
            let observed = u64::try_from(partial.len()).unwrap_or(u64::MAX);
            return Err(AttachmentError::SizeLimit {
                observed,
                limit: limits.max_bytes,
            });
        }
        observer(partial.len(), cancellation);
    }
    cancellation.check()?;
    Ok(partial.into_bytes())
}

fn copy_bounded<F>(
    bytes: &[u8],
    limits: &AttachmentLimits,
    cancellation: &CancellationToken,
    observer: &mut F,
) -> Result<Vec<u8>, AttachmentError>
where
    F: FnMut(usize, &CancellationToken),
{
    let size = u64::try_from(bytes.len()).map_err(|_| AttachmentError::AllocationLimit)?;
    if size > limits.max_bytes {
        return Err(AttachmentError::SizeLimit {
            observed: size,
            limit: limits.max_bytes,
        });
    }
    let mut partial = PartialPayload::new();
    partial
        .bytes_mut()
        .try_reserve(bytes.len())
        .map_err(|_| AttachmentError::AllocationLimit)?;
    for chunk in bytes.chunks(READ_CHUNK_BYTES) {
        cancellation.check()?;
        partial.bytes_mut().extend_from_slice(chunk);
        observer(partial.len(), cancellation);
    }
    cancellation.check()?;
    Ok(partial.into_bytes())
}
