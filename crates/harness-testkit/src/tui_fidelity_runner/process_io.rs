use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use portable_pty::{CommandBuilder, PtySize};

use crate::tui_fidelity::{TerminalType, Viewport};

pub(super) fn configure_environment(command: &mut CommandBuilder, terminal_type: TerminalType) {
    let term = terminal_type.as_str();
    for (key, value) in [
        ("TERM", term),
        ("COLORTERM", "truecolor"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C.UTF-8"),
        ("TZ", "UTC"),
        ("HARNESS_DETERMINISTIC", "1"),
        ("HARNESS_SEED", "42"),
    ] {
        command.env(key, value);
    }
}

pub(super) const fn pty_size(viewport: Viewport) -> PtySize {
    PtySize {
        rows: viewport.rows,
        cols: viewport.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyRead {
    pub completed_at_micros: u64,
    pub bytes: Vec<u8>,
}

pub(super) type PtyReadLog = Arc<Mutex<Vec<PtyRead>>>;

pub(super) fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    epoch: Instant,
) -> (Receiver<PtyRead>, PtyReadLog) {
    let (sender, receiver) = mpsc::channel();
    let history = Arc::new(Mutex::new(Vec::new()));
    let writer_history = Arc::clone(&history);
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            let read = PtyRead {
                completed_at_micros: u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX),
                bytes: buffer[..count].to_vec(),
            };
            if let Ok(mut entries) = writer_history.lock() {
                entries.push(read.clone());
            } else {
                break;
            }
            if count == 0 || sender.send(read).is_err() {
                break;
            }
        }
    });
    (receiver, history)
}
