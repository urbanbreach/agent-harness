use std::sync::Arc;

use harness_core::coord::CoordinatorConfig;
use tokio::task::JoinHandle;

use crate::bootstrap;

use super::live_settings::{demo_coordinator_config, interactive_coordinator_config, LiveSettings};
use super::profile_log;

#[derive(Clone)]
pub(super) struct LiveCoordinatorConfigWarmup {
    state: Arc<tokio::sync::Mutex<(std::path::PathBuf, LiveCoordinatorConfigWarmupState)>>,
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
        } else if settings.config.is_some() {
            let settings = settings.clone();
            LiveCoordinatorConfigWarmupState::Pending(tokio::task::spawn_blocking(move || {
                interactive_coordinator_config(&settings)
            }))
        } else {
            LiveCoordinatorConfigWarmupState::Disabled
        };

        Self {
            state: Arc::new(tokio::sync::Mutex::new((
                settings.workspace_root.clone(),
                state,
            ))),
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

        let mut cached = self.state.lock().await;
        let (workspace, state) = &mut *cached;
        if *workspace != settings.workspace_root {
            if let LiveCoordinatorConfigWarmupState::Pending(handle) = state {
                handle.abort();
            }
            *state = LiveCoordinatorConfigWarmupState::Disabled;
            workspace.clone_from(&settings.workspace_root);
        }
        let config = match std::mem::replace(state, LiveCoordinatorConfigWarmupState::Disabled) {
            LiveCoordinatorConfigWarmupState::Ready(config) => *config,
            LiveCoordinatorConfigWarmupState::Pending(handle) => handle
                .await
                .map_err(|err| format!("live coordinator warmup task failed: {err}"))??,
            LiveCoordinatorConfigWarmupState::Disabled => interactive_coordinator_config(settings)?,
        };
        *state = LiveCoordinatorConfigWarmupState::Ready(Box::new(config.clone()));
        Ok(config)
    }

    /// Every live session clones this config, so its router serves a provider signed in mid-run.
    pub(super) async fn add_signed_in_providers(
        &self,
        settings: &LiveSettings,
        coordinator: Option<&harness_core::coord::CoordinatorHandle>,
    ) -> Result<(), String> {
        let Some(config) = settings.config.as_ref() else {
            return Ok(());
        };
        if matches!(
            self.state.lock().await.1,
            LiveCoordinatorConfigWarmupState::Disabled
        ) {
            return Ok(());
        }
        let current = self.coordinator_config(settings, false).await?;
        let mut refreshed = interactive_coordinator_config(settings)?;
        let providers = bootstrap::provider_map(
            config,
            &harness::CliDeps::real()
                .with_current_dir(settings.workspace_root.clone())
                .with_env("HARNESS_HOME", settings.data_dir.to_string_lossy()),
            &current.secret_registry,
        )?;
        current.provider.add_providers(providers);
        current
            .secret_registry
            .register(refreshed.secret_values.clone())?;
        refreshed.provider = current.provider;
        refreshed.secret_registry = current.secret_registry;
        *self.state.lock().await = (
            settings.workspace_root.clone(),
            LiveCoordinatorConfigWarmupState::Ready(Box::new(refreshed.clone())),
        );
        if let Some(coordinator) = coordinator {
            coordinator
                .refresh_provider_models(refreshed)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}
