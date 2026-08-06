use std::fmt::{Display, Formatter};
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPlatform {
    Linux { wayland: bool },
    MacOs,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardCommand {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

#[derive(Debug)]
pub enum LocalClipboardError {
    Denied,
    Io(std::io::Error),
}

impl LocalClipboardError {
    pub const fn is_denied(&self) -> bool {
        matches!(self, Self::Denied)
    }
}

impl Display for LocalClipboardError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied => formatter.write_str("no permitted local clipboard helper succeeded"),
            Self::Io(error) => write!(formatter, "local clipboard failed: {error}"),
        }
    }
}

impl std::error::Error for LocalClipboardError {}

pub fn copy_local(text: &str, platform: LocalPlatform) -> Result<(), LocalClipboardError> {
    copy_local_with_runner(text, platform, |command, value| {
        let mut child = Command::new(command.program)
            .args(command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let child = match child.as_mut() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(std::io::Error::new(error.kind(), error.to_string())),
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(value.as_bytes())?;
        }
        Ok(child.wait()?.success())
    })
}

pub fn copy_local_with_runner<F>(
    text: &str,
    platform: LocalPlatform,
    mut runner: F,
) -> Result<(), LocalClipboardError>
where
    F: FnMut(&ClipboardCommand, &str) -> Result<bool, std::io::Error>,
{
    for command in commands(platform) {
        match runner(&command, text) {
            Ok(true) => return Ok(()),
            Ok(false) => continue,
            Err(error) => return Err(LocalClipboardError::Io(error)),
        }
    }
    Err(LocalClipboardError::Denied)
}

fn commands(platform: LocalPlatform) -> Vec<ClipboardCommand> {
    const WL_COPY: ClipboardCommand = ClipboardCommand {
        program: "wl-copy",
        args: &[],
    };
    const XCLIP: ClipboardCommand = ClipboardCommand {
        program: "xclip",
        args: &["-selection", "clipboard"],
    };
    const PBCOPY: ClipboardCommand = ClipboardCommand {
        program: "pbcopy",
        args: &[],
    };
    match platform {
        LocalPlatform::Linux { wayland: true } => vec![WL_COPY, XCLIP],
        LocalPlatform::Linux { wayland: false } => vec![XCLIP],
        LocalPlatform::MacOs => vec![PBCOPY],
        LocalPlatform::Windows => Vec::new(),
    }
}
