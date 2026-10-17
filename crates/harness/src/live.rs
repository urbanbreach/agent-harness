//! Live session wiring shared by the terminal UI and the GUI server.
use std::sync::Mutex;

use harness_core::auth::plugin::AuthPluginRegistry;
use harness_core::coord::{CoordinatorError, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor};
use harness_tui::app::ConnectProviderOption;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::tui::TuiCommand;

#[path = "live/auth_backend.rs"]
pub(crate) mod auth_backend;
#[path = "live/coordinator_warmup.rs"]
pub(crate) mod coordinator_warmup;
#[path = "live/launch_metadata.rs"]
pub(crate) mod launch_metadata;
#[path = "live/lineage.rs"]
pub(crate) mod lineage;
#[path = "live/live_events.rs"]
pub(crate) mod live_events;
#[path = "live/live_intents.rs"]
pub(crate) mod live_intents;
#[path = "live/live_options.rs"]
pub(crate) mod live_options;
#[path = "live/live_runtime.rs"]
pub(crate) mod live_runtime;
#[path = "live/live_settings.rs"]
pub(crate) mod live_settings;
#[path = "live/model_selection.rs"]
pub(crate) mod model_selection;
#[path = "live/new_live.rs"]
pub(crate) mod new_live;
#[path = "live/profile_log.rs"]
pub(crate) mod profile_log;
#[path = "live/replay.rs"]
pub(crate) mod replay;
#[path = "live/runtime_toggles.rs"]
pub(crate) mod runtime_toggles;
#[path = "live/session_history.rs"]
pub(crate) mod session_history;
#[path = "live/workflow.rs"]
pub(crate) mod workflow;

use self::live_settings::LiveSettings;
pub use self::replay::replay_workspace_root_from_events;

pub(crate) fn set_pending_prompt_commands_from_settings(settings: &LiveSettings) {
    harness_tui::app::set_pending_prompt_commands(harness_core::commands::discover(
        &settings.workspace_root,
        Some(&settings.data_dir),
        &harness_tui::keybindings::reserved_slash_names(),
    ));
}

pub(crate) fn connect_provider_options(
    config: Option<&harness_core::config::HarnessConfig>,
    registry: &AuthPluginRegistry,
    catalog: Option<&harness_core::provider_catalog::ProviderCatalog>,
) -> Vec<ConnectProviderOption> {
    use harness_core::auth::plugin::AuthMethodSpec;
    use harness_core::auth::ProviderId;
    use harness_core::config::ProviderConfig;
    use harness_tui::app::auth_dialog::catalog_providers;
    use std::collections::BTreeSet;

    let mut providers = catalog
        .map(|catalog| catalog_providers(catalog, registry))
        .unwrap_or_default();
    let mut provider_ids = providers
        .iter()
        .map(|provider| provider.id.to_string())
        .collect::<BTreeSet<_>>();

    let Some(config) = config else {
        return providers;
    };

    for (provider_id, provider_config) in &config.providers {
        if provider_ids.contains(provider_id) {
            continue;
        }
        let ProviderConfig::OpenAiCompatible(oc) = provider_config else {
            continue;
        };
        let label = oc.name.as_deref().unwrap_or(provider_id).to_string();

        if let Some(auth_provider) = oc.auth_provider.clone() {
            if provider_ids.contains(auth_provider.as_str()) {
                continue;
            }
            if let Some(plugin) = registry.get(&auth_provider) {
                provider_ids.insert(auth_provider.to_string());
                providers.push(ConnectProviderOption {
                    id: auth_provider.clone(),
                    label,
                    description: plugin.description().to_string(),
                    methods: plugin.auth_methods().to_vec(),
                    models: Vec::new(),
                });
            }
        } else if !oc.api_key_env.is_empty() {
            let env_var = oc.api_key_env[0].clone();
            let already_set = std::env::var(&env_var).is_ok_and(|v| !v.trim().is_empty());
            if !already_set && let Some(id) = ProviderId::parse(provider_id.as_str()) {
                provider_ids.insert(provider_id.clone());
                providers.push(ConnectProviderOption {
                    id,
                    label,
                    description: "API key".to_string(),
                    methods: vec![AuthMethodSpec::ApiKey {
                        label: "Manually enter API Key".to_string(),
                    }],
                    models: Vec::new(),
                });
            }
        }
    }

    providers
}

pub(crate) fn recover_mutex_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) async fn stop_live_source_run(coordinator: &CoordinatorHandle) -> Result<(), String> {
    let stop_result = coordinator.stop_run().await;
    if let Err(err) = stop_result
        && !matches!(err, CoordinatorError::RunNotStarted)
    {
        return Err(err.to_string());
    }
    Ok(())
}

pub(crate) fn user_actor() -> EventActor {
    EventActor::new(ActorKind::User, Some("interactive-user".to_string()))
}

pub(crate) async fn await_task(
    name: &str,
    handle: JoinHandle<Result<(), String>>,
) -> Result<(), String> {
    match handle.await {
        Ok(result) => result.map_err(|err| format!("{name} task failed: {err}")),
        Err(err) => Err(format!("{name} task join failed: {err}")),
    }
}

pub(crate) fn unique_interactive_run_id() -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let namespace = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("interactive:{}:{}", std::process::id(), stamp).as_bytes(),
    );
    format!("run_{}", namespace.simple())
}
