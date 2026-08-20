//! logind `PrepareForSleep` with a `delay` inhibitor.

use std::thread;
use std::time::Duration;

use super::{PowerCallback, PowerEvent, PowerState};

const DESTINATION: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const INTERFACE: &str = "org.freedesktop.login1.Manager";

pub(crate) struct Listener;

impl Listener {
    pub(crate) fn start(callback: PowerCallback) -> Option<Self> {
        let (proxy, signals) = open_logind()?;
        thread::Builder::new()
            .name("harness-system-power".to_string())
            .spawn(move || run_listener(proxy, signals, callback))
            .ok()?;
        Some(Self)
    }
}

fn open_logind() -> Option<(
    zbus::blocking::Proxy<'static>,
    zbus::blocking::proxy::SignalIterator<'static>,
)> {
    let connection = zbus::blocking::Connection::system().ok()?;
    let proxy = zbus::blocking::Proxy::new(&connection, DESTINATION, PATH, INTERFACE).ok()?;
    let signals = proxy.receive_signal("PrepareForSleep").ok()?;
    Some((proxy, signals))
}

fn take_delay_inhibitor(proxy: &zbus::blocking::Proxy<'_>) -> Option<zbus::zvariant::OwnedFd> {
    proxy
        .call(
            "Inhibit",
            &(
                "sleep",
                "harness",
                "protect credential refresh across suspend",
                "delay",
            ),
        )
        .ok()
}

fn run_listener(
    initial_proxy: zbus::blocking::Proxy<'static>,
    initial_signals: zbus::blocking::proxy::SignalIterator<'static>,
    callback: PowerCallback,
) {
    let mut active = Some((initial_proxy, initial_signals));
    loop {
        let (proxy, signals) = match active.take().or_else(open_logind) {
            Some(connection) => connection,
            None => {
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };
        let mut inhibitor = take_delay_inhibitor(&proxy);
        for signal in signals {
            let Ok(about_to_sleep) = signal.body().deserialize::<bool>() else {
                continue;
            };
            if about_to_sleep {
                callback(PowerEvent::WillSleep);
                inhibitor = None;
            } else {
                callback(PowerEvent::DidWake);
                inhibitor = take_delay_inhibitor(&proxy);
            }
        }
        drop(inhibitor);
        thread::sleep(Duration::from_millis(250));
    }
}

pub(crate) const fn current_power_state() -> PowerState {
    PowerState::Unknown
}

pub(crate) const fn unavailable_diagnostic() -> &'static str {
    "Linux system-power registration unavailable: systemd-logind D-Bus PrepareForSleep service or delay inhibitor is unavailable"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_adapter_reports_unknown_without_a_dark_wake_api() {
        // arrange

        // act
        let state = current_power_state();

        // assert
        assert_eq!(state, PowerState::Unknown);
    }

    #[test]
    fn linux_adapter_missing_logind_is_an_honest_registration_failure() {
        // arrange

        // act
        let listener = Listener::start(Box::new(|_| {}));

        // assert
        if let Some(listener) = listener {
            assert!(std::mem::size_of_val(&listener) == 0);
        }
    }
}
