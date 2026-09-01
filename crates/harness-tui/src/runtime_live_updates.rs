use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use thiserror::Error;

use crate::app::{set_pending_live_prompt_draft, AppState, ToastVariant, UiIntent};
use crate::runtime_integration::RuntimeExperience;
use crate::runtime_scheduling::SchedulingLiveReadiness;
use crate::scheduling::DeferredLiveUpdate;
use crate::{LiveUpdate, OperatorNoticeLevel};

pub(crate) const LIVE_UPDATE_DRAIN_MAX_PER_FRAME: usize = 16;
const LIVE_UPDATE_DRAIN_MAX_DURATION: Duration = Duration::from_millis(8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LiveUpdateDrainState {
    pub changed: bool,
    pub disconnected: bool,
    pub budget_exhausted: bool,
}

#[derive(Clone)]
pub struct LiveUpdateSender(Sender<LiveUpdate>);

pub struct LiveUpdateReceiver {
    receiver: Receiver<LiveUpdate>,
    selected: Mutex<DeferredLiveUpdate<LiveUpdate>>,
}

#[derive(Debug, Error)]
#[error("TUI live-update receiver disconnected")]
pub struct LiveUpdateSendError;

pub fn live_update_channel() -> (LiveUpdateSender, LiveUpdateReceiver) {
    let (sender, receiver) = unbounded();
    (
        LiveUpdateSender(sender),
        LiveUpdateReceiver {
            receiver,
            selected: Mutex::new(DeferredLiveUpdate::default()),
        },
    )
}

impl LiveUpdateSender {
    pub fn send(&self, update: LiveUpdate) -> Result<(), LiveUpdateSendError> {
        self.0.send(update).map_err(|_| LiveUpdateSendError)
    }
}

impl LiveUpdateReceiver {
    pub fn try_recv(&self) -> Result<LiveUpdate, TryRecvError> {
        let mut selected = match self.selected.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match selected.take() {
            Some(update) => Ok(update),
            None => self.receiver.try_recv(),
        }
    }

    pub fn recv(&self) -> Result<LiveUpdate, crossbeam_channel::RecvError> {
        self.receiver.recv()
    }

    pub fn receiver(&self) -> &Receiver<LiveUpdate> {
        &self.receiver
    }

    pub fn is_empty(&self) -> bool {
        let selected_empty = match self.selected.lock() {
            Ok(guard) => !guard.is_some(),
            Err(poisoned) => !poisoned.into_inner().is_some(),
        };
        selected_empty && self.receiver.is_empty()
    }

    pub fn ready_depth(&self) -> usize {
        let selected = match self.selected.lock() {
            Ok(guard) => usize::from(guard.is_some()),
            Err(poisoned) => usize::from(poisoned.into_inner().is_some()),
        };
        selected.saturating_add(self.receiver.len())
    }

    pub fn scheduling_readiness(&self, stream_active: bool) -> SchedulingLiveReadiness {
        let deferred_ready = match self.selected.lock() {
            Ok(guard) => guard.is_some(),
            Err(poisoned) => poisoned.into_inner().is_some(),
        };
        SchedulingLiveReadiness {
            queued_depth: self.receiver.len(),
            deferred_ready,
            stream_active,
        }
    }

    pub fn try_iter(&self) -> crossbeam_channel::TryIter<'_, LiveUpdate> {
        self.receiver.try_iter()
    }

    pub fn defer_selected(&self, update: LiveUpdate) {
        let mut selected = match self.selected.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let deferred = selected.defer(update);
        debug_assert!(deferred.is_ok());
    }
}

pub(crate) fn drain_live_updates(
    app: &mut AppState,
    receiver: &LiveUpdateReceiver,
) -> LiveUpdateDrainState {
    let mut experience = RuntimeExperience::new();
    apply_live_update_quantum(app, receiver, &mut experience)
}

pub(crate) fn apply_live_update_quantum(
    app: &mut AppState,
    receiver: &LiveUpdateReceiver,
    experience: &mut RuntimeExperience,
) -> LiveUpdateDrainState {
    drain_with_limit(app, receiver, experience, LIVE_UPDATE_DRAIN_MAX_PER_FRAME)
}

fn drain_with_limit(
    app: &mut AppState,
    receiver: &LiveUpdateReceiver,
    experience: &mut RuntimeExperience,
    limit: usize,
) -> LiveUpdateDrainState {
    let mut state = LiveUpdateDrainState::default();
    let mut drained = 0_usize;
    let started_at = Instant::now();
    loop {
        if drained >= limit
            || (drained > 0 && started_at.elapsed() >= LIVE_UPDATE_DRAIN_MAX_DURATION)
        {
            state.budget_exhausted = !receiver.is_empty();
            break;
        }
        match receiver.try_recv() {
            Ok(update) => {
                drained = drained.saturating_add(1);
                state.changed |= apply_update(app, experience, update);
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.changed |= app.apply_runtime_event_stream_closed();
                state.disconnected = true;
                break;
            }
        }
    }
    state
}

fn apply_update(
    app: &mut AppState,
    experience: &mut RuntimeExperience,
    update: LiveUpdate,
) -> bool {
    match update {
        LiveUpdate::Event(event) => {
            if app
                .status_banner
                .as_deref()
                .is_some_and(transient_live_status_banner)
            {
                app.set_status_banner(None);
            }
            if let harness_core::event::RuntimeEvent::Durable(durable) = event.as_ref() {
                experience.on_event(durable);
            }
            app.ingest_runtime_event(*event);
        }
        LiveUpdate::Status(status) => {
            if app.status_banner.as_deref() == Some(status.as_str()) {
                return false;
            }
            app.set_status_banner(Some(status));
        }
        LiveUpdate::SessionHistory(entries) => app.set_session_history_entries(entries),
        LiveUpdate::ContinueSession {
            run_id,
            run_dir,
            prompt_draft,
        } => {
            set_pending_live_prompt_draft(Some(prompt_draft));
            app.emit_ui_intent(UiIntent::ContinueSession { run_id, run_dir });
            app.should_quit = true;
        }
        LiveUpdate::OperatorNotice { message, level } => {
            apply_operator_notice(app, message, level);
        }
        LiveUpdate::AuthBackendResult { success, message } => {
            let message = if !success && is_auth_backend_failure_summary(&message) {
                app.status_banner.clone().unwrap_or(message)
            } else {
                message
            };
            app.apply_auth_backend_result(success, &message);
        }
        LiveUpdate::AuthProviderCatalogRefreshed { launch_metadata } => {
            app.apply_auth_provider_catalog_refresh(*launch_metadata);
        }
        LiveUpdate::PluginLifecycleSummary(summary) => {
            app.set_plugin_lifecycle_summary(Some(summary));
        }
    }
    true
}

fn apply_operator_notice(app: &mut AppState, message: String, level: OperatorNoticeLevel) {
    app.append_connect_dialog_authorization_detail(&message);
    if matches!(level, OperatorNoticeLevel::Error) && is_auth_backend_failure_detail(&message) {
        app.note_auth_backend_failure(&message);
    }
    if matches!(level, OperatorNoticeLevel::Error)
        && app.status_banner.as_deref() != Some(message.as_str())
        && !(is_auth_backend_failure_summary(&message)
            && app
                .status_banner
                .as_deref()
                .is_some_and(is_auth_backend_failure_detail))
    {
        app.set_status_banner(Some(message.clone()));
    }
    app.show_toast(
        message,
        match level {
            OperatorNoticeLevel::Info => ToastVariant::Info,
            OperatorNoticeLevel::Error => ToastVariant::Error,
        },
    );
}

fn transient_live_status_banner(status: &str) -> bool {
    let lower = status.to_ascii_lowercase();
    lower == "starting new session" || lower.contains("lagged") || lower.contains("replaying")
}

fn is_auth_backend_failure_summary(message: &str) -> bool {
    message.starts_with("auth backend failed (exit ") && !message.contains('\n')
}

fn is_auth_backend_failure_detail(message: &str) -> bool {
    message.starts_with("auth backend error:")
}
