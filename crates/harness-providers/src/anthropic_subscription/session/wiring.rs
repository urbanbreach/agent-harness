//! Coordinator session events (senpi `session-registry-wiring.ts`).
use super::binding::BindingStore;
use super::reattach::{binding_from_entry, remember_binding};
use super::registry::{close_session, get_session, record_pending_fork, touch, SessionEntry};
use super::stream::invalidate_binding;
use crate::ProviderSessionEvent;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, MutexGuard, PoisonError},
};

/// The provider id each resident session last ran under; two configured subscription
/// providers share one registry, so a routing event names the owner it moved away from.
static SESSION_PROVIDERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

fn providers() -> MutexGuard<'static, HashMap<String, String>> {
    SESSION_PROVIDERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub fn record_session_provider(session_id: &str, provider_id: &str) {
    providers().insert(session_id.into(), provider_id.into());
}

/// Leaving the lane is an excursion, not an invalidation: the live query closes but the
/// binding stays, so coming back reattaches at the recorded prefix.
pub fn keep_binding_then_close(entry: &SessionEntry, reason: &str) {
    let hashes = entry.lock().sent_hashes.clone().unwrap_or_default();
    remember_binding(&entry.session_id, binding_from_entry(entry, &hashes));
    close_session(&entry.session_id, reason);
}

pub fn handle_session_event(event: &ProviderSessionEvent, store: Option<&BindingStore>) {
    match event {
        ProviderSessionEvent::Routed {
            session_id,
            provider_id,
            ..
        } => {
            let moved = providers()
                .get(session_id)
                .is_some_and(|owner| owner != provider_id);
            if moved && let Some(entry) = get_session(session_id) {
                keep_binding_then_close(&entry, "model_selected");
            }
        }
        // Leaving the lane closes the live query but keeps the binding; staying switches the
        // model in place, and a session that cannot switch closes and reattaches.
        ProviderSessionEvent::ModelSelected {
            session_id,
            provider_id,
            model_id,
        } => {
            let owner = providers().get(session_id).cloned();
            let Some(entry) = get_session(session_id) else {
                return;
            };
            if owner.is_some_and(|owner| owner != *provider_id) {
                keep_binding_then_close(&entry, "model_selected");
                return;
            }
            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                return;
            };
            let model = model_id.clone();
            handle.spawn(async move {
                if entry.query.set_model(&model).await.is_ok() {
                    entry.lock().model_id = model;
                    touch(&entry);
                }
            });
        }
        // Closing a streaming query would kill the in-flight turn; reasoning is part of the
        // toolset fingerprint, so the next admission reattaches with the new level anyway.
        ProviderSessionEvent::ReasoningSelected { session_id } => {
            if let Some(entry) = get_session(session_id)
                && !entry.lock().has_active_turn()
            {
                keep_binding_then_close(&entry, "thinking_level_selected");
            }
        }
        ProviderSessionEvent::Compacted { session_id } => {
            record_pending_fork(session_id, "compaction");
            invalidate_binding(session_id, store, "compaction");
        }
        ProviderSessionEvent::Rewound { session_id } => {
            invalidate_binding(session_id, store, "tree_changed");
        }
        ProviderSessionEvent::Closed { session_id, reason } => {
            providers().remove(session_id);
            close_session(session_id, reason);
        }
    }
}
