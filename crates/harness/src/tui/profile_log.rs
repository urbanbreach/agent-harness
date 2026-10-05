use std::io::Write;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

fn handoff_profile_file() -> Option<&'static Mutex<std::fs::File>> {
    static PROFILE_FILE: LazyLock<Option<Mutex<std::fs::File>>> = LazyLock::new(|| {
        let path = std::env::var_os("HARNESS_TUI_PROFILE_LOG")?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Mutex::new(file))
    });
    PROFILE_FILE.as_ref()
}

fn handoff_profile_start() -> &'static Instant {
    static START: LazyLock<Instant> = LazyLock::new(Instant::now);
    &START
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
