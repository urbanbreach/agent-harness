use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use portable_pty::{CommandBuilder, PtySize};

use crate::tui_fidelity::Viewport;

pub(super) fn configure_environment(command: &mut CommandBuilder) {
    for (key, value) in [
        ("TERM", "xterm-256color"),
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

pub(super) fn spawn_reader(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    receiver
}
