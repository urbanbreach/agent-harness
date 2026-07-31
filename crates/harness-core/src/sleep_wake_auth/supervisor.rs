// allow: SIZE_OK — process-lifecycle state machine and private deterministic race harness share internal-only invariants.
//! Process-scoped credential protection around native sleep/wake transitions.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::auth::ProviderCredentialManager;
use crate::system_power::{
    current_power_state, native_platform_diagnostic, PowerEvent, PowerState, SystemPowerListener,
};

use super::SleepWakeEventSource;

const EVENT_BUFFER: usize = 32;
const INPUT_BUFFER: usize = 16;
static PROCESS_SUPERVISOR_REGISTERED: AtomicBool = AtomicBool::new(false);
static NATIVE_PROCESS_SUPERVISOR: OnceLock<Mutex<Option<SleepWakeSupervisor>>> = OnceLock::new();

/// No credential material is included in supervisor fan-out events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepWakeSupervisorEvent {
    Started,
    SleepGateRaised,
    SleepBudgetElapsed,
    WakeFull,
    WakeDark,
    WakeUnknown,
    RefreshStarted,
    RefreshCompleted,
    RefreshFailed,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepWakeSupervisorState {
    Awake,
    Sleeping,
    DarkWake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialRefreshResult {
    Completed,
    Failed,
}

#[async_trait]
pub trait SleepWakeCredentialRefresher: Send + Sync {
    async fn refresh_near_expiry(&self, leeway: Duration) -> CredentialRefreshResult;
}

#[async_trait]
impl SleepWakeCredentialRefresher for ProviderCredentialManager {
    async fn refresh_near_expiry(&self, leeway: Duration) -> CredentialRefreshResult {
        match self.refresh_oauth_if_near_expiry(leeway).await {
            Ok(_) => CredentialRefreshResult::Completed,
            Err(_) => CredentialRefreshResult::Failed,
        }
    }
}

pub type PowerStateQuery = Arc<dyn Fn() -> PowerState + Send + Sync>;

#[derive(Clone)]
pub struct SleepWakeSupervisorConfig {
    pub refresh_leeway: Duration,
    pub sleep_budget: Duration,
    pub power_state: PowerStateQuery,
}

impl Default for SleepWakeSupervisorConfig {
    fn default() -> Self {
        Self {
            refresh_leeway: Duration::from_secs(5 * 60),
            sleep_budget: Duration::from_secs(5),
            power_state: Arc::new(current_power_state),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SleepWakeSupervisorError {
    #[error("a system-power supervisor is already registered for this process")]
    AlreadyRegistered,
    #[error("native system-power registration unavailable: {diagnostic}")]
    NativeUnavailable { diagnostic: &'static str },
    #[error("native system-power supervisor requires an active Tokio runtime")]
    RuntimeUnavailable,
    #[error("native system-power supervisor registry is unavailable")]
    RegistryUnavailable,
}

struct PowerSignal {
    event: PowerEvent,
    acknowledgement: Option<std::sync::mpsc::SyncSender<()>>,
}

/// Owns the one process-wide native registration, refresh gate, fan-out, and
/// shutdown path. Call [`Self::shutdown`] before process teardown.
pub struct SleepWakeSupervisor {
    cancellation: CancellationToken,
    events: broadcast::Sender<SleepWakeSupervisorEvent>,
    worker: Option<JoinHandle<()>>,
    source_forwarder: Option<JoinHandle<()>>,
    native_listener: Option<SystemPowerListener>,
    registered: bool,
}

impl SleepWakeSupervisor {
    pub fn start_native(
        refresher: Arc<dyn SleepWakeCredentialRefresher>,
        config: SleepWakeSupervisorConfig,
    ) -> Result<Self, SleepWakeSupervisorError> {
        let mut registration = ProcessRegistration::acquire()?;
        let cancellation = CancellationToken::new();
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let (signals, receiver) = mpsc::channel(INPUT_BUFFER);
        let callback_signals = signals.clone();
        let budget = config.sleep_budget;
        let listener = SystemPowerListener::start(move |event| {
            let acknowledgement = (event == PowerEvent::WillSleep).then(|| {
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                let _ = callback_signals.blocking_send(PowerSignal {
                    event,
                    acknowledgement: Some(sender),
                });
                let _ = receiver.recv_timeout(budget);
            });
            if acknowledgement.is_none() {
                let _ = callback_signals.blocking_send(PowerSignal {
                    event,
                    acknowledgement: None,
                });
            }
        })
        .ok_or(SleepWakeSupervisorError::NativeUnavailable {
            diagnostic: native_platform_diagnostic(),
        })?;
        let worker = spawn_worker(
            receiver,
            Arc::clone(&refresher),
            config,
            events.clone(),
            cancellation.clone(),
        );
        let _ = events.send(SleepWakeSupervisorEvent::Started);
        registration.disarm();
        Ok(Self {
            cancellation,
            events,
            worker: Some(worker),
            source_forwarder: None,
            native_listener: Some(listener),
            registered: true,
        })
    }

    pub fn start_with_source(
        mut source: Box<dyn SleepWakeEventSource>,
        refresher: Arc<dyn SleepWakeCredentialRefresher>,
        config: SleepWakeSupervisorConfig,
    ) -> Result<Self, SleepWakeSupervisorError> {
        let mut registration = ProcessRegistration::acquire()?;
        let cancellation = CancellationToken::new();
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let (signals, receiver) = mpsc::channel(INPUT_BUFFER);
        let forwarder_cancellation = cancellation.clone();
        let source_forwarder = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = forwarder_cancellation.cancelled() => return,
                    event = source.recv() => match event {
                        Some(event) => {
                            let power_event = if event.may_trigger_refresh_evaluation() {
                                PowerEvent::DidWake
                            } else {
                                PowerEvent::WillSleep
                            };
                            if signals.send(PowerSignal { event: power_event, acknowledgement: None }).await.is_err() {
                                return;
                            }
                        }
                        None => return,
                    },
                }
            }
        });
        let worker = spawn_worker(
            receiver,
            refresher,
            config,
            events.clone(),
            cancellation.clone(),
        );
        let _ = events.send(SleepWakeSupervisorEvent::Started);
        registration.disarm();
        Ok(Self {
            cancellation,
            events,
            worker: Some(worker),
            source_forwarder: Some(source_forwarder),
            native_listener: None,
            registered: true,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SleepWakeSupervisorEvent> {
        self.events.subscribe()
    }

    pub async fn shutdown(&mut self) {
        self.cancellation.cancel();
        if let Some(listener) = self.native_listener.take() {
            drop(listener);
        }
        if let Some(forwarder) = self.source_forwarder.take() {
            let _ = forwarder.await;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.await;
        }
        if self.registered {
            PROCESS_SUPERVISOR_REGISTERED.store(false, Ordering::SeqCst);
            self.registered = false;
        }
    }
}

/// Installs the native supervisor once for the process runtime.
pub fn install_native_process_supervisor<R>(
    refresher: Arc<R>,
) -> Result<(), SleepWakeSupervisorError>
where
    R: SleepWakeCredentialRefresher + 'static,
{
    if tokio::runtime::Handle::try_current().is_err() {
        return Err(SleepWakeSupervisorError::RuntimeUnavailable);
    }
    let registry = NATIVE_PROCESS_SUPERVISOR.get_or_init(|| Mutex::new(None));
    let mut supervisor = registry
        .lock()
        .map_err(|_| SleepWakeSupervisorError::RegistryUnavailable)?;
    if supervisor.is_none() {
        let refresher = refresher as Arc<dyn SleepWakeCredentialRefresher>;
        *supervisor = Some(SleepWakeSupervisor::start_native(
            refresher,
            SleepWakeSupervisorConfig::default(),
        )?);
    }
    Ok(())
}

/// Stops the process-owned native listener and waits for its bounded cleanup.
pub async fn shutdown_native_process_supervisor() -> Result<(), SleepWakeSupervisorError> {
    let registry = NATIVE_PROCESS_SUPERVISOR.get_or_init(|| Mutex::new(None));
    let supervisor = registry
        .lock()
        .map_err(|_| SleepWakeSupervisorError::RegistryUnavailable)?
        .take();
    if let Some(mut supervisor) = supervisor {
        supervisor.shutdown().await;
    }
    Ok(())
}

impl Drop for SleepWakeSupervisor {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if self.registered {
            PROCESS_SUPERVISOR_REGISTERED.store(false, Ordering::SeqCst);
            self.registered = false;
        }
    }
}

struct ProcessRegistration {
    armed: bool,
}

impl ProcessRegistration {
    fn acquire() -> Result<Self, SleepWakeSupervisorError> {
        PROCESS_SUPERVISOR_REGISTERED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| SleepWakeSupervisorError::AlreadyRegistered)?;
        Ok(Self { armed: true })
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessRegistration {
    fn drop(&mut self) {
        if self.armed {
            PROCESS_SUPERVISOR_REGISTERED.store(false, Ordering::SeqCst);
        }
    }
}

fn spawn_worker(
    receiver: mpsc::Receiver<PowerSignal>,
    refresher: Arc<dyn SleepWakeCredentialRefresher>,
    config: SleepWakeSupervisorConfig,
    events: broadcast::Sender<SleepWakeSupervisorEvent>,
    cancellation: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(run_worker(
        receiver,
        refresher,
        config,
        events,
        cancellation,
    ))
}

async fn run_worker(
    mut receiver: mpsc::Receiver<PowerSignal>,
    refresher: Arc<dyn SleepWakeCredentialRefresher>,
    config: SleepWakeSupervisorConfig,
    events: broadcast::Sender<SleepWakeSupervisorEvent>,
    cancellation: CancellationToken,
) {
    let mut state = SleepWakeSupervisorState::Awake;
    let mut refresh: Option<JoinHandle<CredentialRefreshResult>> = None;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                cancel_refresh(&mut refresh, config.sleep_budget).await;
                let _ = events.send(SleepWakeSupervisorEvent::Shutdown);
                return;
            }
            signal = receiver.recv() => match signal {
                Some(signal) => process_signal(signal, &mut state, &mut refresh, &refresher, &config, &events).await,
                None => return,
            },
            result = await_refresh(&mut refresh), if refresh.is_some() => {
                emit_refresh_result(result, &events);
            }
        }
    }
}

async fn process_signal(
    signal: PowerSignal,
    state: &mut SleepWakeSupervisorState,
    refresh: &mut Option<JoinHandle<CredentialRefreshResult>>,
    refresher: &Arc<dyn SleepWakeCredentialRefresher>,
    config: &SleepWakeSupervisorConfig,
    events: &broadcast::Sender<SleepWakeSupervisorEvent>,
) {
    match signal.event {
        PowerEvent::WillSleep => {
            *state = SleepWakeSupervisorState::Sleeping;
            let _ = events.send(SleepWakeSupervisorEvent::SleepGateRaised);
            finish_refresh(refresh, config.sleep_budget, events).await;
        }
        PowerEvent::DidWake => match (config.power_state)() {
            PowerState::FullWake => {
                *state = SleepWakeSupervisorState::Awake;
                let _ = events.send(SleepWakeSupervisorEvent::WakeFull);
                start_refresh(refresh, refresher, config.refresh_leeway, events);
            }
            PowerState::DarkWake => {
                *state = SleepWakeSupervisorState::DarkWake;
                let _ = events.send(SleepWakeSupervisorEvent::WakeDark);
            }
            PowerState::Unknown => {
                *state = SleepWakeSupervisorState::Awake;
                let _ = events.send(SleepWakeSupervisorEvent::WakeUnknown);
                start_refresh(refresh, refresher, config.refresh_leeway, events);
            }
        },
    }
    if let Some(acknowledgement) = signal.acknowledgement {
        let _ = acknowledgement.send(());
    }
}

fn start_refresh(
    refresh: &mut Option<JoinHandle<CredentialRefreshResult>>,
    refresher: &Arc<dyn SleepWakeCredentialRefresher>,
    leeway: Duration,
    events: &broadcast::Sender<SleepWakeSupervisorEvent>,
) {
    if refresh.is_some() {
        return;
    }
    let refresher = Arc::clone(refresher);
    *refresh = Some(tokio::spawn(async move {
        refresher.refresh_near_expiry(leeway).await
    }));
    let _ = events.send(SleepWakeSupervisorEvent::RefreshStarted);
}

async fn await_refresh(
    refresh: &mut Option<JoinHandle<CredentialRefreshResult>>,
) -> CredentialRefreshResult {
    let Some(handle) = refresh.as_mut() else {
        return CredentialRefreshResult::Failed;
    };
    let result = match handle.await {
        Ok(result) => result,
        Err(_) => CredentialRefreshResult::Failed,
    };
    *refresh = None;
    result
}

async fn finish_refresh(
    refresh: &mut Option<JoinHandle<CredentialRefreshResult>>,
    budget: Duration,
    events: &broadcast::Sender<SleepWakeSupervisorEvent>,
) {
    if refresh.is_none() {
        return;
    }
    match timeout(budget, await_refresh(refresh)).await {
        Ok(result) => emit_refresh_result(result, events),
        Err(_) => {
            let _ = events.send(SleepWakeSupervisorEvent::SleepBudgetElapsed);
        }
    }
}

async fn cancel_refresh(
    refresh: &mut Option<JoinHandle<CredentialRefreshResult>>,
    budget: Duration,
) {
    if refresh.is_none() {
        return;
    }
    if timeout(budget, await_refresh(refresh)).await.is_err() {
        if let Some(handle) = refresh.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}

fn emit_refresh_result(
    result: CredentialRefreshResult,
    events: &broadcast::Sender<SleepWakeSupervisorEvent>,
) {
    let event = match result {
        CredentialRefreshResult::Completed => SleepWakeSupervisorEvent::RefreshCompleted,
        CredentialRefreshResult::Failed => SleepWakeSupervisorEvent::RefreshFailed,
    };
    let _ = events.send(event);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::sleep_wake_auth::HookSleepWakeEventSource;

    struct CountingRefresher(AtomicUsize);

    #[async_trait]
    impl SleepWakeCredentialRefresher for CountingRefresher {
        async fn refresh_near_expiry(&self, _leeway: Duration) -> CredentialRefreshResult {
            self.0.fetch_add(1, Ordering::SeqCst);
            CredentialRefreshResult::Completed
        }
    }

    struct BlockingRefresher {
        started: tokio::sync::Notify,
        release: tokio::sync::Notify,
    }

    #[async_trait]
    impl SleepWakeCredentialRefresher for BlockingRefresher {
        async fn refresh_near_expiry(&self, _leeway: Duration) -> CredentialRefreshResult {
            self.started.notify_one();
            self.release.notified().await;
            CredentialRefreshResult::Completed
        }
    }

    fn config(state: PowerState) -> SleepWakeSupervisorConfig {
        SleepWakeSupervisorConfig {
            refresh_leeway: Duration::ZERO,
            sleep_budget: Duration::from_millis(20),
            power_state: Arc::new(move || state),
        }
    }

    fn refresher_source<T>(refresher: &Arc<T>) -> Arc<dyn SleepWakeCredentialRefresher>
    where
        T: SleepWakeCredentialRefresher + 'static,
    {
        Arc::clone(refresher) as Arc<dyn SleepWakeCredentialRefresher>
    }

    #[tokio::test]
    async fn dark_wake_does_not_start_credential_refresh() {
        let (source, injector) = HookSleepWakeEventSource::open();
        let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
        let mut supervisor = SleepWakeSupervisor::start_with_source(
            Box::new(source),
            refresher_source(&refresher),
            config(PowerState::DarkWake),
        )
        .unwrap();
        let mut events = supervisor.subscribe();
        injector
            .inject(super::super::SleepWakeHostEvent::Wake)
            .unwrap();
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::WakeDark
        );
        assert_eq!(refresher.0.load(Ordering::SeqCst), 0);
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn unknown_power_state_preserves_wake_refresh_behavior() {
        let (source, injector) = HookSleepWakeEventSource::open();
        let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
        let mut supervisor = SleepWakeSupervisor::start_with_source(
            Box::new(source),
            refresher_source(&refresher),
            config(PowerState::Unknown),
        )
        .unwrap();
        let mut events = supervisor.subscribe();
        injector
            .inject(super::super::SleepWakeHostEvent::Wake)
            .unwrap();
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::WakeUnknown
        );
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::RefreshStarted
        );
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::RefreshCompleted
        );
        assert_eq!(refresher.0.load(Ordering::SeqCst), 1);
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_releases_process_registration_for_the_next_supervisor() {
        let (source, _injector) = HookSleepWakeEventSource::open();
        let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
        let mut first = SleepWakeSupervisor::start_with_source(
            Box::new(source),
            refresher_source(&refresher),
            config(PowerState::FullWake),
        )
        .unwrap();
        assert!(matches!(
            SleepWakeSupervisor::start_with_source(
                Box::new(HookSleepWakeEventSource::open().0),
                refresher_source(&refresher),
                config(PowerState::FullWake),
            ),
            Err(SleepWakeSupervisorError::AlreadyRegistered)
        ));
        first.shutdown().await;
        let (source, _injector) = HookSleepWakeEventSource::open();
        let mut second = SleepWakeSupervisor::start_with_source(
            Box::new(source),
            refresher_source(&refresher),
            config(PowerState::FullWake),
        )
        .unwrap();
        second.shutdown().await;
    }

    #[tokio::test]
    async fn sleep_gate_waits_only_for_the_platform_budget_then_preserves_refresh_completion() {
        let (source, injector) = HookSleepWakeEventSource::open();
        let refresher = Arc::new(BlockingRefresher {
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let mut supervisor = SleepWakeSupervisor::start_with_source(
            Box::new(source),
            refresher_source(&refresher),
            config(PowerState::FullWake),
        )
        .unwrap();
        let mut events = supervisor.subscribe();
        injector
            .inject(super::super::SleepWakeHostEvent::Wake)
            .unwrap();
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::WakeFull
        );
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::RefreshStarted
        );
        refresher.started.notified().await;
        injector
            .inject(super::super::SleepWakeHostEvent::Sleep)
            .unwrap();
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::SleepGateRaised
        );
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::SleepBudgetElapsed
        );
        refresher.release.notify_one();
        assert_eq!(
            events.recv().await.unwrap(),
            SleepWakeSupervisorEvent::RefreshCompleted
        );
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_cancels_a_refresh_that_cannot_finish_within_its_budget() {
        let (source, injector) = HookSleepWakeEventSource::open();
        let refresher = Arc::new(BlockingRefresher {
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let mut supervisor = SleepWakeSupervisor::start_with_source(
            Box::new(source),
            refresher_source(&refresher),
            config(PowerState::FullWake),
        )
        .unwrap();
        injector
            .inject(super::super::SleepWakeHostEvent::Wake)
            .unwrap();
        refresher.started.notified().await;
        supervisor.shutdown().await;
    }

    #[test]
    fn supervisor_events_do_not_include_credential_material() {
        assert_eq!(
            format!("{:?}", SleepWakeSupervisorEvent::RefreshFailed),
            "RefreshFailed"
        );
    }
}
