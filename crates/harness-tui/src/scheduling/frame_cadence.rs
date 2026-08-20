use std::sync::OnceLock;

use super::FLUSH_DEADLINE_MS;

pub const MIN_DRAW_INTERVAL_ENV: &str = "HARNESS_TUI_MIN_DRAW_MS";

const MIN_INTERVAL_MS: u64 = 1;
const MAX_INTERVAL_MS: u64 = 100;

pub(crate) fn runtime_flush_interval_ms() -> u64 {
    static INTERVAL_MS: OnceLock<u64> = OnceLock::new();
    *INTERVAL_MS.get_or_init(|| {
        std::env::var(MIN_DRAW_INTERVAL_ENV)
            .ok()
            .as_deref()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map_or(FLUSH_DEADLINE_MS, clamp_flush_interval_ms)
    })
}

pub(super) const fn clamp_flush_interval_ms(interval_ms: u64) -> u64 {
    if interval_ms < MIN_INTERVAL_MS {
        MIN_INTERVAL_MS
    } else if interval_ms > MAX_INTERVAL_MS {
        MAX_INTERVAL_MS
    } else {
        interval_ms
    }
}
