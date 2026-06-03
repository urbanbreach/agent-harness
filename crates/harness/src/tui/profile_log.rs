use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

fn handoff_profile_file() -> Option<&'static Mutex<std::fs::File>> {
    static PROFILE_FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    PROFILE_FILE
        .get_or_init(|| {
            let path = std::env::var_os("HARNESS_TUI_PROFILE_LOG")?;
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()?;
            Some(Mutex::new(file))
        })
        .as_ref()
}

fn handoff_profile_start() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

pub(super) fn profile_handoff(event: &str) {
    let Some(file) = handoff_profile_file() else {
        return;
    };

    let elapsed_ms = handoff_profile_start().elapsed().as_millis();
    let mut file = match file.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let _ = writeln!(file, "{elapsed_ms:>6}ms {event}");
}
