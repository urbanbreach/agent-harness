use std::sync::Arc;

use harness_core::coord::CoordinatorConfig;
use tokio::task::JoinHandle;

use crate::bootstrap;

use super::live_settings::{demo_coordinator_config, interactive_coordinator_config, LiveSettings};
use super::profile_log;

#[derive(Clone)]
pub(super) struct LiveCoordinatorConfigWarmup {
    state: Arc<tokio::sync::Mutex<LiveCoordinatorConfigWarmupState>>,
}

enum LiveCoordinatorConfigWarmupState {
    Disabled,
    Pending(JoinHandle<Result<CoordinatorConfig, String>>),
    Ready(Box<CoordinatorConfig>),
}

impl LiveCoordinatorConfigWarmup {
    pub(super) fn start(settings: &LiveSettings, demo_mode: bool) -> Self {
        profile_log::profile_handoff(&format!(
            "warmup.start demo_mode={} has_config={}",
            demo_mode,
            settings.config.is_some()
        ));
        let state = if demo_mode {
            LiveCoordinatorConfigWarmupState::Disabled
        } else if let Some(mut config) = settings.config.clone() {
            let session_dir = settings.session_dir.clone();
            LiveCoordinatorConfigWarmupState::Pending(tokio::task::spawn_blocking(move || {
                profile_log::profile_handoff("warmup.build.begin");
                config.apply_session_dir_override(Some(session_dir));
                let result = bootstrap::build_interactive_coordinator_config(&config);
                profile_log::profile_handoff("warmup.build.end");
                result
            }))
        } else {
            LiveCoordinatorConfigWarmupState::Disabled
        };

        Self {
            state: Arc::new(tokio::sync::Mutex::new(state)),
        }
    }

    pub(super) async fn coordinator_config(
        &self,
        settings: &LiveSettings,
        demo_mode: bool,
    ) -> Result<CoordinatorConfig, String> {
        if demo_mode {
            profile_log::profile_handoff("warmup.use_demo_config");
            return Ok(demo_coordinator_config(settings));
        }

        let pending = {
            let mut state = self.state.lock().await;
            match &*state {
                LiveCoordinatorConfigWarmupState::Ready(config) => {
                    profile_log::profile_handoff("warmup.cache_hit");
                    return Ok(config.as_ref().clone());
                }
                LiveCoordinatorConfigWarmupState::Disabled => {
                    profile_log::profile_handoff("warmup.disabled_fallback");
                    None
                }
                LiveCoordinatorConfigWarmupState::Pending(_) => {
                    profile_log::profile_handoff("warmup.await_pending");
                    match std::mem::replace(&mut *state, LiveCoordinatorConfigWarmupState::Disabled)
                    {
                        LiveCoordinatorConfigWarmupState::Pending(handle) => Some(handle),
                        LiveCoordinatorConfigWarmupState::Ready(config) => {
                            let resolved = config.as_ref().clone();
                            profile_log::profile_handoff("warmup.ready_race");
                            *state = LiveCoordinatorConfigWarmupState::Ready(config);
                            return Ok(resolved);
                        }
                        LiveCoordinatorConfigWarmupState::Disabled => None,
                    }
                }
            }
        };

        if let Some(handle) = pending {
            let config = handle
                .await
                .map_err(|err| format!("live coordinator warmup task failed: {err}"))??;
            profile_log::profile_handoff("warmup.pending_resolved");
            let mut state = self.state.lock().await;
            *state = LiveCoordinatorConfigWarmupState::Ready(Box::new(config.clone()));
            return Ok(config);
        }

        profile_log::profile_handoff("warmup.rebuild_fallback");
        interactive_coordinator_config(settings)
    }
}
