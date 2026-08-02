//! Native system suspend/resume notifications used by credential protection.
//!
//! The listener is process-scoped. Its `WillSleep` callback may block only for
//! the configured platform budget, after which the native adapter acknowledges
//! the transition and lets the operating system proceed.

/// A native system-power transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    WillSleep,
    DidWake,
}

/// A coarse power-state sample used to reject unsafe dark-wake refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    FullWake,
    DarkWake,
    Unknown,
}

pub type PowerCallback = Box<dyn Fn(PowerEvent) + Send + Sync + 'static>;

/// Returns a cheap native state sample without requiring a running listener.
pub fn current_power_state() -> PowerState {
    imp::current_power_state()
}

/// Exact operator diagnostic when native registration cannot be established.
pub const fn native_platform_diagnostic() -> &'static str {
    imp::unavailable_diagnostic()
}

#[cfg(target_os = "linux")]
#[path = "system_power/linux.rs"]
mod imp;

#[cfg(target_os = "macos")]
#[path = "system_power/macos.rs"]
mod imp;

#[cfg(target_os = "windows")]
#[path = "system_power/windows.rs"]
mod imp;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod imp {
    use super::{PowerCallback, PowerState};

    pub(crate) struct Listener;

    impl Listener {
        pub(crate) fn start(_callback: PowerCallback) -> Option<Self> {
            None
        }
    }

    pub(crate) const fn current_power_state() -> PowerState {
        PowerState::Unknown
    }

    pub(crate) const fn unavailable_diagnostic() -> &'static str {
        "native system-power notifications are unsupported on this platform"
    }
}

/// A registered native listener. Dropping it releases platform resources where
/// supported; Linux's blocking logind signal iterator remains process-lifetime.
pub struct SystemPowerListener {
    inner: imp::Listener,
}

impl SystemPowerListener {
    /// Registers exactly one native adapter callback for this listener.
    pub fn start<F>(callback: F) -> Option<Self>
    where
        F: Fn(PowerEvent) + Send + Sync + 'static,
    {
        imp::Listener::start(Box::new(callback)).map(|inner| Self { inner })
    }

    pub fn is_registered(&self) -> bool {
        let _ = &self.inner;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_events_are_distinct_and_copyable() {
        let event = PowerEvent::WillSleep;
        assert_eq!(event, PowerEvent::WillSleep);
        assert_ne!(event, PowerEvent::DidWake);
    }

    #[test]
    fn current_state_query_never_panics() {
        let _state = current_power_state();
    }

    #[test]
    fn unavailable_diagnostic_is_operator_safe() {
        assert!(!native_platform_diagnostic().is_empty());
    }
}
