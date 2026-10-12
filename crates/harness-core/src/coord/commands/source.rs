//! What a background task runs: a process group on pipes or a pseudo-terminal, or a
//! polled file.
use super::*;
use rustix::fd::OwnedFd;
use std::os::unix::fs::MetadataExt;

pub(super) enum Source {
    Process {
        child: tokio::process::Child,
        group: crate::process::Group,
        pty: Option<OwnedFd>,
    },
    File {
        path: PathBuf,
        event: super::FileEvent,
    },
}

/// Starts a launch and returns it with its working directory and process id.
pub(super) fn spawn(
    launch: super::Launch,
    root: &Path,
) -> Result<(Source, String, Option<u32>), CoordinatorError> {
    let (mut command, pty) = match launch {
        super::Launch::File { path, event } => {
            return Ok((
                Source::File { path, event },
                root.to_string_lossy().into_owned(),
                None,
            ));
        }
        super::Launch::Pipes(mut command) => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            (command, None)
        }
        super::Launch::Pty(mut command) => {
            let (master, terminal) = open_pty()?;
            command
                .stdout(Stdio::from(terminal.try_clone()?))
                .stderr(Stdio::from(terminal));
            (command, Some(master))
        }
    };
    command.stdin(Stdio::null()).kill_on_drop(true);
    let cwd = command
        .as_std()
        .get_current_dir()
        .unwrap_or(root)
        .to_string_lossy()
        .into_owned();
    // The command, and with it this process's copy of the terminal, closes on return.
    let (child, group) = crate::process::spawn_group(command)
        .map_err(|error| CoordinatorError::Invalid(error.to_string()))?;
    let pid = child.id();
    Ok((Source::Process { child, group, pty }, cwd, pid))
}

pub(super) async fn run(
    handle: CoordinatorHandle,
    id: String,
    source: Source,
    timeout: Duration,
    cancellation: CancellationToken,
) -> Result<CommandExit, CoordinatorError> {
    match source {
        Source::Process { child, group, pty } => {
            super::run_command(handle, id, child, group, pty, timeout, cancellation).await
        }
        Source::File { path, event } => {
            watch_file(handle, id, path, event, timeout, cancellation).await
        }
    }
}

fn open_pty() -> std::io::Result<(OwnedFd, OwnedFd)> {
    use rustix::{
        fs::{Mode, OFlags},
        pty::{self, OpenptFlags},
        termios,
    };
    let master = pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)?;
    pty::grantpt(&master)?;
    pty::unlockpt(&master)?;
    let name = pty::ptsname(&master, Vec::new())?;
    let terminal = rustix::fs::open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let mut settings = termios::tcgetattr(&terminal)?;
    // Keep output bytes as written instead of turning each newline into CRLF.
    settings.output_modes.remove(termios::OutputModes::OPOST);
    termios::tcsetattr(&terminal, termios::OptionalActions::Now, &settings)?;
    termios::tcsetwinsize(
        &terminal,
        termios::Winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        },
    )?;
    rustix::io::ioctl_fionbio(&master, true)?;
    Ok((master, terminal))
}

pub(super) async fn read_pty(
    handle: CoordinatorHandle,
    id: String,
    master: OwnedFd,
) -> Result<(), CoordinatorError> {
    let master = tokio::io::unix::AsyncFd::new(master)?;
    let mut buffer = [0; 8192];
    loop {
        let mut ready = master.readable().await?;
        let read = ready.try_io(|fd| {
            rustix::io::read(fd.get_ref(), &mut buffer[..]).map_err(std::io::Error::from)
        });
        let count = match read {
            Err(_would_block) => continue,
            Ok(Ok(count)) => count,
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            // Linux reports EIO once every terminal descriptor is closed.
            Ok(Err(error))
                if error.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) =>
            {
                0
            }
            Ok(Err(error)) => return Err(error.into()),
        };
        let bytes = buffer[..count].to_vec();
        let id = id.clone();
        handle
            .call(move |s| s.command_bytes(&id, 0, &bytes, count == 0))
            .await?;
        if count == 0 {
            return Ok(());
        }
    }
}

type Stamp = (u64, u64, i64, i64, i64, i64);

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    metadata.is_file().then(|| {
        (
            metadata.ino(),
            metadata.size(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        )
    })
}

/// Polls one path four times a second and reports its first create or modify.
async fn watch_file(
    handle: CoordinatorHandle,
    id: String,
    path: PathBuf,
    event: super::FileEvent,
    timeout: Duration,
    cancellation: CancellationToken,
) -> Result<CommandExit, CoordinatorError> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let start = stamp(&path);
    let mut previous = start;
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return Ok(CommandExit { exit_code: None, timed_out: false }),
            () = &mut deadline => return Ok(CommandExit { exit_code: None, timed_out: true }),
            _ = tick.tick() => {}
        }
        let now = stamp(&path);
        let hit = match event {
            super::FileEvent::Create => previous.is_none() && now.is_some(),
            super::FileEvent::Modify => now.is_some() && now != start,
        };
        previous = now;
        if hit {
            let line = format!("{} {}\n", event.as_str(), path.display());
            handle
                .call(move |s| s.command_bytes(&id, 0, line.as_bytes(), false))
                .await?;
            return Ok(CommandExit {
                exit_code: Some(0),
                timed_out: false,
            });
        }
    }
}
