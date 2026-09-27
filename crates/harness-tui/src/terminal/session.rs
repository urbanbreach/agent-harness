//! Own terminal modes from the first setup operation through exit or an explicit handoff.
use std::io::{self, Write};
use std::sync::{Mutex, MutexGuard};

use crossterm::{
    cursor::Show,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute, queue,
    terminal::{
        disable_raw_mode, enable_raw_mode, EndSynchronizedUpdate, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};

#[derive(Clone, Copy, Default)]
struct Modes {
    keyboard: bool,
    paste: bool,
    mouse: bool,
    focus: bool,
}

// The CLI can hand one stdout terminal between startup, live and replay invocations.
static PRESERVED: Mutex<Option<Modes>> = Mutex::new(None);

fn preserved() -> MutexGuard<'static, Option<Modes>> {
    PRESERVED.lock().unwrap_or_else(|error| error.into_inner())
}

pub(crate) struct Session {
    modes: Modes,
    armed: bool,
}

impl Session {
    pub(crate) fn enter(skip_alternate_screen: bool) -> io::Result<Self> {
        let previous = preserved().take();
        let mut session = Self {
            modes: previous.unwrap_or_default(),
            armed: true,
        };
        if previous.is_none() {
            enable_raw_mode()?;
            session.enable(&mut io::stdout(), skip_alternate_screen)?;
        }
        Ok(session)
    }

    fn enable(&mut self, writer: &mut impl Write, skip_alternate_screen: bool) -> io::Result<()> {
        if !skip_alternate_screen {
            execute!(writer, EnterAlternateScreen)?;
        }
        self.modes.keyboard = queue!(
            writer,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES,
            )
        )
        .is_ok();
        let _ = writer.flush();
        self.modes.paste = true;
        execute!(writer, EnableBracketedPaste)?;
        // Mouse setup is one compound command. Restore it even if a partial write fails.
        self.modes.mouse = true;
        execute!(writer, EnableMouseCapture)?;
        self.modes.focus = true;
        let _ = execute!(writer, EnableFocusChange);
        Ok(())
    }

    pub(crate) fn preserve(mut self) {
        *preserved() = Some(self.modes);
        self.armed = false;
    }

    pub(crate) fn finish(&mut self, writer: &mut impl Write) -> io::Result<()> {
        self.armed = false;
        restore(writer, self.modes)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.finish(&mut io::stdout());
        }
    }
}

pub fn close_preserved_terminal_session() -> anyhow::Result<()> {
    if let Some(modes) = preserved().take() {
        restore(&mut io::stdout(), modes)?;
    }
    Ok(())
}

fn restore(writer: &mut impl Write, modes: Modes) -> io::Result<()> {
    let mut error = disable_raw_mode().err();
    let mut attempt = |result: io::Result<()>| {
        if let Err(failure) = result {
            error.get_or_insert(failure);
        }
    };
    attempt(execute!(writer, EndSynchronizedUpdate));
    if modes.mouse {
        attempt(execute!(writer, DisableMouseCapture));
    }
    if modes.paste {
        attempt(execute!(writer, DisableBracketedPaste));
    }
    if modes.focus {
        attempt(execute!(writer, DisableFocusChange));
    }
    if modes.keyboard {
        attempt(execute!(writer, PopKeyboardEnhancementFlags));
    }
    attempt(execute!(writer, LeaveAlternateScreen));
    attempt(execute!(writer, Show));
    error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teardown_attempts_remaining_protocols_after_writer_failure() {
        struct FailOnce {
            failed: bool,
            bytes: Vec<u8>,
        }
        impl Write for FailOnce {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !self.failed {
                    self.failed = true;
                    return Err(io::Error::other("injected restore failure"));
                }
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = FailOnce {
            failed: false,
            bytes: Vec::new(),
        };
        let modes = Modes {
            keyboard: true,
            paste: true,
            mouse: true,
            focus: true,
        };
        assert!(restore(&mut writer, modes).is_err());
        for escape in [
            b"\x1b[?2004l".as_slice(),
            b"\x1b[?1004l",
            b"\x1b[<1u",
            b"\x1b[?1049l",
            b"\x1b[?25h",
        ] {
            assert!(
                writer
                    .bytes
                    .windows(escape.len())
                    .any(|bytes| bytes == escape),
                "restoration stopped after an earlier failure: {escape:?}"
            );
        }
        let mut absent = Vec::new();
        assert!(restore(&mut absent, Modes::default()).is_ok());
        assert_eq!(absent, b"\x1b[?2026l\x1b[?1049l\x1b[?25h");
    }

    #[test]
    fn setup_flush_failure_restores_every_mode_written_to_the_terminal() {
        struct FailFlush {
            bytes: Vec<u8>,
            flushes: usize,
            fail_at: usize,
        }
        impl Write for FailFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                if self.flushes == self.fail_at {
                    Err(io::Error::other("injected setup flush failure"))
                } else {
                    Ok(())
                }
            }
        }
        for fail_at in 1..=4 {
            let mut writer = FailFlush {
                bytes: Vec::new(),
                flushes: 0,
                fail_at,
            };
            let mut session = Session {
                modes: Modes::default(),
                armed: false,
            };
            let _ = session.enable(&mut writer, true);
            let setup = String::from_utf8_lossy(&writer.bytes).into_owned();
            writer.bytes.clear();
            assert!(session.finish(&mut writer).is_ok());
            let restored = String::from_utf8_lossy(&writer.bytes);
            for (enabled, disabled) in [
                ("\x1b[>3u", "\x1b[<1u"),
                ("\x1b[?2004h", "\x1b[?2004l"),
                ("\x1b[?1000h", "\x1b[?1000l"),
                ("\x1b[?1004h", "\x1b[?1004l"),
            ] {
                assert!(
                    !setup.contains(enabled) || restored.contains(disabled),
                    "flush {fail_at} left {enabled:?} active"
                );
            }
        }
    }
}
