use deno_runtime::deno_io::{Stdio, StdioPipe};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek},
};

pub(super) struct Capture {
    _directory: tempfile::TempDir,
    streams: [(File, &'static str, Vec<u8>); 2],
}

impl Capture {
    pub fn new(root: &std::path::Path) -> std::io::Result<(Self, Stdio)> {
        let directory = tempfile::Builder::new()
            .prefix("harness-js-output-")
            .tempdir_in(root)?;
        let open = |name| -> std::io::Result<(File, File)> {
            let path = directory.path().join(name);
            let writer = OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(&path)?;
            let reader = OpenOptions::new().read(true).write(true).open(path)?;
            Ok((reader, writer))
        };
        let (stdout, output) = open("stdout")?;
        let (stderr, errors) = open("stderr")?;
        let stdio = Stdio {
            stdin: StdioPipe::file(tempfile::tempfile()?),
            stdout: StdioPipe::file(output),
            stderr: StdioPipe::file(errors),
        };
        Ok((
            Self {
                _directory: directory,
                streams: [
                    (stdout, "stdout", Vec::new()),
                    (stderr, "stderr", Vec::new()),
                ],
            },
            stdio,
        ))
    }

    pub fn drain(
        &mut self,
        mut emit: impl FnMut(&str, String) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut bytes = [0; 8192];
        for (reader, name, pending) in &mut self.streams {
            loop {
                let count = reader.read(&mut bytes)?;
                if count == 0 {
                    break;
                }
                pending.extend_from_slice(&bytes[..count]);
                let end = match std::str::from_utf8(pending) {
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    _ => pending.len(),
                };
                if end > 0 {
                    emit(name, String::from_utf8_lossy(&pending[..end]).into_owned())?;
                    pending.drain(..end);
                }
            }
        }
        Ok(())
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        for (reader, _, pending) in &mut self.streams {
            reader.set_len(0)?;
            reader.rewind()?;
            pending.clear();
        }
        Ok(())
    }
}
