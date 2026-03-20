use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use vt100::Parser as VtParser;

pub(crate) struct SpawnedPtyProcess {
    pub(crate) child: Box<dyn portable_pty::Child + Send>,
    pub(crate) writer: Box<dyn Write + Send>,
    pub(crate) output_rx: Receiver<Vec<u8>>,
    pub(crate) parser: VtParser,
}

pub(crate) fn spawn_pty_process(
    pty_size: PtySize,
    command: CommandBuilder,
    spawn_label: &str,
) -> Result<SpawnedPtyProcess, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size)
        .map_err(|err| format!("failed to open {spawn_label} PTY: {err}"))?;

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|err| format!("failed to spawn {spawn_label} command: {err}"))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("failed to clone {spawn_label} PTY reader: {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("failed to take {spawn_label} PTY writer: {err}"))?;

    Ok(SpawnedPtyProcess {
        child,
        writer,
        output_rx: spawn_pty_reader_thread(reader),
        parser: VtParser::new(pty_size.rows, pty_size.cols, 0),
    })
}

pub(crate) fn spawn_pty_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}
