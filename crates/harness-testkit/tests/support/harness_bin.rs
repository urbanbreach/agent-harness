use std::ffi::OsStr;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use harness_testkit::parity::{semantic_frame_from_vt100_screen, SemanticFrame};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

pub fn tui_fidelity_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tui-fidelity"))
}

pub fn connect(address: impl ToSocketAddrs) -> std::io::Result<TcpStream> {
    TcpStream::connect(address)
}

pub fn capture_semantic_frame(path: &Path) -> Option<SemanticFrame> {
    let rows = 40;
    let cols = 120;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .ok()?;
    let mut command = CommandBuilder::new(path);
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    let _child = pair.slave.spawn_command(command).ok()?;
    let mut reader = pair.master.try_clone_reader().ok()?;
    drop(pair.slave);

    let start = Instant::now();
    let dwell = Duration::from_millis(2_500);
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4_096];
    while start.elapsed() < dwell {
        let read = reader.read(&mut buffer).unwrap_or(0);
        bytes.extend_from_slice(&buffer[..read]);
    }
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes);
    Some(semantic_frame_from_vt100_screen(parser.screen()))
}
