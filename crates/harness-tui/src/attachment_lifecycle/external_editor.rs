use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use super::cleanup::TempArtifact;
use super::limits::{AttachmentLimits, READ_CHUNK_BYTES};
use super::{AttachmentError, CancellationToken};

#[derive(Clone, Debug)]
pub struct EditorCommand {
    program: OsString,
    args: Vec<OsString>,
}

impl EditorCommand {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.args.push(argument.as_ref().to_os_string());
        self
    }
}

#[derive(Clone, Debug)]
pub struct ExternalEditor {
    command: EditorCommand,
    limits: AttachmentLimits,
}

impl ExternalEditor {
    pub fn new(command: EditorCommand) -> Self {
        Self {
            command,
            limits: AttachmentLimits::default(),
        }
    }

    pub fn with_limits(self, limits: AttachmentLimits) -> Self {
        Self { limits, ..self }
    }

    pub fn edit(
        &self,
        initial: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, AttachmentError> {
        cancellation.check()?;
        let artifact = TempArtifact::new("attachment-editor", initial)?;
        let mut command = Command::new(&self.command.program);
        command.args(&self.command.args).arg(artifact.path());
        let mut child = command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|_| AttachmentError::EditorSpawn)?;

        loop {
            if cancellation.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AttachmentError::Cancelled);
            }
            match child.try_wait().map_err(|_| AttachmentError::EditorWait)? {
                Some(status) if status.success() => break,
                Some(status) => {
                    return Err(AttachmentError::EditorNonZero {
                        code: status.code(),
                    });
                }
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
        read_bounded(&artifact.path().to_path_buf(), self.limits.max_bytes)
    }
}

fn read_bounded(path: &PathBuf, max_bytes: u64) -> Result<Vec<u8>, AttachmentError> {
    let mut file = File::open(path).map_err(|_| AttachmentError::TempWrite)?;
    let size = file
        .metadata()
        .map_err(|_| AttachmentError::TempWrite)?
        .len();
    if size > max_bytes {
        return Err(AttachmentError::SizeLimit {
            observed: size,
            limit: max_bytes,
        });
    }
    let capacity = usize::try_from(size).map_err(|_| AttachmentError::AllocationLimit)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve(capacity)
        .map_err(|_| AttachmentError::AllocationLimit)?;
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|_| AttachmentError::TempWrite)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if u64::try_from(bytes.len()).map_err(|_| AttachmentError::AllocationLimit)? > max_bytes {
            let observed = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            return Err(AttachmentError::SizeLimit {
                observed,
                limit: max_bytes,
            });
        }
    }
}
