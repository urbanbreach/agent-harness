use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use harness_core::sleep_wake_auth::supervisor::{
    CredentialRefreshResult, SleepWakeCredentialRefresher, SleepWakeSupervisor,
    SleepWakeSupervisorConfig, SleepWakeSupervisorError, SleepWakeSupervisorEvent,
};
use harness_core::sleep_wake_auth::{HookSleepWakeEventSource, SleepWakeHostEvent};
use harness_core::system_power::PowerState;
use harness_core::UnwrapOrAbort;

struct CountingRefresher(AtomicUsize);

#[async_trait]
impl SleepWakeCredentialRefresher for CountingRefresher {
    async fn refresh_near_expiry(&self, _: Duration) -> CredentialRefreshResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        CredentialRefreshResult::Completed
    }
}

#[tokio::test]
async fn process_supervisor_gates_sleep_dark_wake_and_refreshes_one_full_wake_once() {
    // Given: a process-scoped supervisor whose power state transitions through sleep and dark wake.
    let (source, injector) = HookSleepWakeEventSource::open();
    let state = Arc::new(Mutex::new(PowerState::DarkWake));
    let state_query = Arc::clone(&state);
    let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
    let mut supervisor = SleepWakeSupervisor::start_with_source(
        Box::new(source),
        Arc::clone(&refresher) as Arc<dyn SleepWakeCredentialRefresher>,
        SleepWakeSupervisorConfig {
            refresh_leeway: Duration::ZERO,
            sleep_budget: Duration::from_millis(20),
            power_state: Arc::new(move || *state_query.lock().unwrap_or_abort()),
        },
    )
    .unwrap_or_abort();
    let mut events = supervisor.subscribe();

    // When: sleep, dark wake, then one full wake are injected.
    injector.inject(SleepWakeHostEvent::Sleep).unwrap_or_abort();
    assert_eq!(
        events.recv().await.unwrap_or_abort(),
        SleepWakeSupervisorEvent::SleepGateRaised
    );
    injector.inject(SleepWakeHostEvent::Wake).unwrap_or_abort();
    assert_eq!(
        events.recv().await.unwrap_or_abort(),
        SleepWakeSupervisorEvent::WakeDark
    );
    *state.lock().unwrap_or_abort() = PowerState::FullWake;
    injector.inject(SleepWakeHostEvent::Wake).unwrap_or_abort();

    // Then: no unsafe refresh runs while sleeping/dark, exactly one wake refresh occurs, and a second monitor is refused.
    assert_eq!(
        events.recv().await.unwrap_or_abort(),
        SleepWakeSupervisorEvent::WakeFull
    );
    assert_eq!(
        events.recv().await.unwrap_or_abort(),
        SleepWakeSupervisorEvent::RefreshStarted
    );
    assert_eq!(
        events.recv().await.unwrap_or_abort(),
        SleepWakeSupervisorEvent::RefreshCompleted
    );
    assert_eq!(refresher.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        SleepWakeSupervisor::start_with_source(
            Box::new(HookSleepWakeEventSource::open().0),
            Arc::clone(&refresher) as Arc<dyn SleepWakeCredentialRefresher>,
            SleepWakeSupervisorConfig::default()
        ),
        Err(SleepWakeSupervisorError::AlreadyRegistered)
    ));
    supervisor.shutdown().await;
}
