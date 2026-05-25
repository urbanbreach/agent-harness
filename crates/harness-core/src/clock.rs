use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

pub trait Clock {
    fn mono_ms(&self) -> u64;
    fn system_time_rfc3339(&self) -> Option<String>;
    fn system_time_rfc3339_millis(&self) -> Option<String>;
}

#[derive(Debug)]
pub struct RealClock {
    start: Instant,
}

impl RealClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn mono_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn system_time_rfc3339(&self) -> Option<String> {
        Some(humantime::format_rfc3339(SystemTime::now()).to_string())
    }

    fn system_time_rfc3339_millis(&self) -> Option<String> {
        Some(crate::session_title::system_time_millis_iso(
            SystemTime::now(),
        ))
    }
}

#[derive(Debug, Default)]
pub struct FakeClock {
    mono_ms: AtomicU64,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            mono_ms: AtomicU64::new(0),
        }
    }

    pub fn advance(&self, ms: u64) {
        self.mono_ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn mono_ms(&self) -> u64 {
        self.mono_ms.load(Ordering::SeqCst)
    }

    fn system_time_rfc3339(&self) -> Option<String> {
        None
    }

    fn system_time_rfc3339_millis(&self) -> Option<String> {
        None
    }
}

pub const HARNESS_DETERMINISTIC_ENV: &str = "HARNESS_DETERMINISTIC";

#[derive(Debug)]
pub enum ClockSource {
    Real(RealClock),
    Fake(FakeClock),
}

impl Clock for ClockSource {
    fn mono_ms(&self) -> u64 {
        match self {
            Self::Real(clock) => clock.mono_ms(),
            Self::Fake(clock) => clock.mono_ms(),
        }
    }

    fn system_time_rfc3339(&self) -> Option<String> {
        match self {
            Self::Real(clock) => clock.system_time_rfc3339(),
            Self::Fake(clock) => clock.system_time_rfc3339(),
        }
    }

    fn system_time_rfc3339_millis(&self) -> Option<String> {
        match self {
            Self::Real(clock) => clock.system_time_rfc3339_millis(),
            Self::Fake(clock) => clock.system_time_rfc3339_millis(),
        }
    }
}

pub struct Determinism;

impl Determinism {
    pub fn enabled(config_deterministic: bool) -> bool {
        Self::enabled_with_env(
            config_deterministic,
            std::env::var(HARNESS_DETERMINISTIC_ENV).ok().as_deref(),
        )
    }

    fn enabled_with_env(config_deterministic: bool, env_value: Option<&str>) -> bool {
        if config_deterministic {
            return true;
        }

        matches!(env_value, Some("1"))
    }

    pub fn select_clock(config_deterministic: bool) -> ClockSource {
        if Self::enabled(config_deterministic) {
            ClockSource::Fake(FakeClock::new())
        } else {
            ClockSource::Real(RealClock::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, ClockSource, Determinism, FakeClock};

    #[test]
    fn fake_clock_starts_at_zero_and_advances_deterministically() {
        let clock = FakeClock::new();
        assert_eq!(clock.mono_ms(), 0);

        clock.advance(10);
        assert_eq!(clock.mono_ms(), 10);

        clock.advance(25);
        assert_eq!(clock.mono_ms(), 35);
    }

    #[test]
    fn deterministic_mode_uses_fake_clock_and_no_system_time() {
        assert!(Determinism::enabled_with_env(false, Some("1")));

        let configured_clock = if Determinism::enabled_with_env(false, Some("1")) {
            ClockSource::Fake(FakeClock::new())
        } else {
            Determinism::select_clock(false)
        };
        assert!(matches!(configured_clock, ClockSource::Fake(_)));
        assert_eq!(configured_clock.system_time_rfc3339(), None);
    }

    #[test]
    fn deterministic_mode_enabled_by_config() {
        assert!(Determinism::enabled_with_env(true, None));
        let clock = if Determinism::enabled_with_env(true, None) {
            ClockSource::Fake(FakeClock::new())
        } else {
            Determinism::select_clock(false)
        };
        assert!(matches!(clock, ClockSource::Fake(_)));
        assert_eq!(clock.system_time_rfc3339(), None);
    }
}
