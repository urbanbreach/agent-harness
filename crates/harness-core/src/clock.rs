use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime},
};

pub trait Clock {
    fn mono_ms(&self) -> u64;
    fn system_time_rfc3339(&self) -> Option<String>;
    fn system_time_rfc3339_millis(&self) -> Option<String> {
        self.system_time_rfc3339()
    }
}

pub const HARNESS_DETERMINISTIC_ENV: &str = "HARNESS_DETERMINISTIC";
pub struct Determinism;
impl Determinism {
    pub fn enabled(configured: bool) -> bool {
        configured
            || std::env::var(HARNESS_DETERMINISTIC_ENV)
                .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
    }
}

#[derive(Debug)]
pub struct RealClock(Instant);

impl RealClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}
impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clock for RealClock {
    fn mono_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    fn system_time_rfc3339(&self) -> Option<String> {
        Some(humantime::format_rfc3339_seconds(SystemTime::now()).to_string())
    }
    fn system_time_rfc3339_millis(&self) -> Option<String> {
        Some(humantime::format_rfc3339_millis(SystemTime::now()).to_string())
    }
}

#[derive(Debug, Default)]
pub struct FakeClock(AtomicU64);

impl FakeClock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::Relaxed);
    }
}
impl Clock for FakeClock {
    fn mono_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
    fn system_time_rfc3339(&self) -> Option<String> {
        None
    }
}
