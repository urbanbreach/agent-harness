use harness_core::{
    auth::{CredentialStore, ProviderCredentialManager, ProviderId},
    sleep_wake_auth::*,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn wake_refresh_requires_expiry_and_stops_without_work_after_cancellation(
) -> Result<(), Box<dyn std::error::Error>> {
    for (event, expiry, refresh) in [
        (SleepWakeHostEvent::Sleep, Some(0), false),
        (SleepWakeHostEvent::Suspend, Some(0), false),
        (SleepWakeHostEvent::Wake, None, false),
        (SleepWakeHostEvent::Wake, Some(i64::MAX), false),
        (SleepWakeHostEvent::Resume, Some(1000), true),
        (SleepWakeHostEvent::Wake, Some(i64::MIN), true),
    ] {
        let snapshot = CredentialExpirySnapshot::with_default_leeway(expiry, 1000);
        let decision = decide_sleep_wake_credential_refresh_for(event, Some(&snapshot));
        assert_eq!(decision.is_refresh(), refresh);
        assert_eq!(decision.event(), event);
    }
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path());
    let manager =
        ProviderCredentialManager::new(store, ProviderId::codex(), Vec::new(), "", |_| None);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let decision = decide_sleep_wake_credential_refresh_for(
        SleepWakeHostEvent::Wake,
        Some(&CredentialExpirySnapshot::with_default_leeway(Some(1), 2)),
    );
    assert!(
        execute_sleep_wake_refresh_decision(&decision, &manager, &cancel)
            .await
            .is_cancelled()
    );
    let (mut source, injector) = HookSleepWakeEventSource::open();
    injector.inject(SleepWakeHostEvent::Wake)?;
    let results = run_sleep_wake_refresh_loop(&mut source, &manager, |_| None, cancel).await;
    assert!(results.is_empty());
    let (mut source, injector) = HookSleepWakeEventSource::open();
    injector.inject(SleepWakeHostEvent::Sleep)?;
    injector.inject(SleepWakeHostEvent::Wake)?;
    drop(injector);
    let results = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        run_sleep_wake_refresh_loop(&mut source, &manager, |_| None, CancellationToken::new()),
    )
    .await?;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(SleepWakeRefreshExecution::is_skipped));
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    Ok(())
}
