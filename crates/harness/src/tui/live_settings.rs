use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::auth::CredentialStore;
use harness_core::clock::Determinism;
use harness_core::config::{HarnessConfig, ShellAllowlist};
use harness_core::coord::CoordinatorConfig;
use harness_tools::coordinator_registry;
use harness_tui::app::{LaunchMetadata, TogglesConfig};

use crate::bootstrap;
use crate::cli_config::load_optional_config_with_digest_context;
use crate::defaults::{DEFAULT_MOCK_PROFILE, DEFAULT_SESSION_DIR};
use crate::scenarios::{
    create_workspace, default_permission_policy, golden_path_profiles, golden_path_provider,
    ScenarioName,
};

use super::launch_metadata::{
    interactive_launch_metadata, launch_metadata_for_connected_providers,
};
use super::model_selection::{
    apply_persisted_model_selection, apply_persisted_model_selection_from_path,
};
use super::runtime_toggles::runtime_toggles_config;
use super::workflow::LaunchSelection;
use super::{recover_mutex_lock, TuiCommand};

#[derive(Debug, Clone)]
pub(super) struct LiveSettings {
    pub(super) config: Option<HarnessConfig>,
    pub(super) config_path: Option<PathBuf>,
    pub(super) session_dir: PathBuf,
    pub(super) workspace_root: PathBuf,
    pub(super) shell_allowlist: ShellAllowlist,
    pub(super) deterministic: bool,
    pub(super) seed: u64,
    pub(super) config_digest: String,
    pub(super) launch_metadata: LaunchMetadata,
    pub(super) launch_mode_label: Option<String>,
    pub(super) toggles: TogglesConfig,
}

pub(super) enum ResolvedTuiMode {
    Replay {
        run_dir: PathBuf,
    },
    Continue {
        settings: LiveSettings,
        run_dir: PathBuf,
    },
    Interactive {
        settings: LiveSettings,
    },
    Mock {
        settings: LiveSettings,
    },
    Scenario {
        settings: LiveSettings,
        scenario: ScenarioName,
    },
}

pub(super) fn resolve_tui_mode(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
) -> Result<ResolvedTuiMode, String> {
    if let Some(run_dir) = &cmd.replay {
        return Ok(ResolvedTuiMode::Replay {
            run_dir: run_dir.clone(),
        });
    }

    let settings = resolve_live_settings(
        cmd,
        config_path,
        global_session_dir,
        workspace_root,
        config_context,
    )?;

    if let Some(run_dir) = &cmd.continue_session {
        return Ok(ResolvedTuiMode::Continue {
            settings,
            run_dir: run_dir.clone(),
        });
    }

    if let Some(scenario) = cmd.scenario {
        return Ok(ResolvedTuiMode::Scenario { settings, scenario });
    }

    if cmd.mock {
        return Ok(ResolvedTuiMode::Mock { settings });
    }

    Ok(ResolvedTuiMode::Interactive { settings })
}

pub(super) fn resolve_live_settings(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
) -> Result<LiveSettings, String> {
    let credential_store = CredentialStore::from_env();
    resolve_live_settings_with_deps(
        cmd,
        config_path,
        global_session_dir,
        workspace_root,
        config_context,
        credential_store.as_ref(),
        &|name| std::env::var(name).ok(),
        None,
    )
}

#[cfg(test)]
pub(super) fn resolve_live_settings_for_test(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
    credential_store: Option<&CredentialStore>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
    model_selection_path: Option<&Path>,
) -> Result<LiveSettings, String> {
    resolve_live_settings_with_deps(
        cmd,
        config_path,
        global_session_dir,
        workspace_root,
        config_context,
        credential_store,
        env_lookup,
        model_selection_path,
    )
}

fn resolve_live_settings_with_deps(
    cmd: &TuiCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    config_context: &harness_core::config::ConfigLoadContext,
    credential_store: Option<&CredentialStore>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
    model_selection_path: Option<&Path>,
) -> Result<LiveSettings, String> {
    let mut shell_allowlist = ShellAllowlist::default();
    let mut config_session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut config_deterministic = false;
    let mut config_seed = 0;
    let mut config_digest = "none".to_string();
    let mut config_default_profile = DEFAULT_MOCK_PROFILE.to_string();
    let mut live_config: Option<HarnessConfig> = None;
    let mut agent_profiles = golden_path_profiles();

    let loaded = if cmd.mock || cmd.scenario.is_some() {
        None
    } else {
        load_optional_config_with_digest_context(config_path.as_deref(), config_context)?
    };
    let project_config_loaded = loaded.is_some();

    let mut connected_provider_ids = Vec::new();
    let mut no_provider_connected = false;
    if cmd.scenario.is_none() && !cmd.mock {
        let runtime_catalog = crate::runtime_catalog::resolve_runtime_catalog(
            loaded.as_ref().map(|loaded| loaded.config.clone()),
            loaded.as_ref().map(|loaded| loaded.digest.clone()),
            None,
            credential_store,
            env_lookup,
        )?;
        let config = runtime_catalog.config;
        config_digest = runtime_catalog.config_digest;
        connected_provider_ids = runtime_catalog.connected_provider_ids;
        no_provider_connected = runtime_catalog.no_provider_connected;
        config_default_profile = bootstrap::interactive_profile_name(&config);
        agent_profiles = bootstrap::interactive_agent_profiles(&config)?;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        config_session_dir = config.paths.session_dir.clone();
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
        live_config = Some(config);
    } else if let Some(loaded) = loaded {
        let config = loaded.config;
        config_digest = loaded.digest;
        config_default_profile = bootstrap::interactive_profile_name(&config);
        agent_profiles = bootstrap::interactive_agent_profiles(&config)?;
        shell_allowlist = config.permissions.shell_allowlist.clone();
        config_session_dir = config.paths.session_dir.clone();
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
        live_config = Some(config);
    }

    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .unwrap_or(config_session_dir);
    let deterministic = cmd.deterministic || Determinism::enabled(config_deterministic);
    let default_profile = cmd.profile.clone().unwrap_or(config_default_profile);
    let launch_mode_label = if live_config.is_some() {
        None
    } else {
        Some("Demo".to_string())
    };
    let mut launch_metadata =
        interactive_launch_metadata(live_config.as_ref(), &agent_profiles, &default_profile)?;
    if !project_config_loaded {
        launch_metadata = launch_metadata_for_connected_providers(
            launch_metadata,
            &connected_provider_ids,
            no_provider_connected,
        );
    }
    let launch_metadata = if live_config.is_some() && !no_provider_connected {
        if let Some(path) = model_selection_path {
            apply_persisted_model_selection_from_path(launch_metadata, path)
        } else {
            apply_persisted_model_selection(launch_metadata)
        }
    } else {
        launch_metadata
    };
    let toggles = runtime_toggles_config(live_config.as_ref(), &workspace_root);

    Ok(LiveSettings {
        config: live_config,
        config_path,
        session_dir,
        workspace_root,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
        launch_metadata,
        launch_mode_label,
        toggles,
    })
}

pub(super) fn launch_metadata_for_mode(
    settings: &LiveSettings,
    selection: &LaunchSelection,
) -> LaunchMetadata {
    let launch_metadata = recover_mutex_lock(selection).clone();
    if let Some(mode_label) = settings.launch_mode_label.as_deref() {
        launch_metadata.with_mode_label(mode_label)
    } else {
        launch_metadata
    }
}

pub(super) fn demo_coordinator_config(settings: &LiveSettings) -> CoordinatorConfig {
    let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config
}

pub(super) fn interactive_coordinator_config(
    settings: &LiveSettings,
) -> Result<CoordinatorConfig, String> {
    let mut config = settings
        .config
        .clone()
        .ok_or_else(bootstrap::interactive_config_guidance)?;
    config.apply_session_dir_override(Some(settings.session_dir.clone()));
    bootstrap::build_interactive_coordinator_config(&config)
}

pub(super) fn prepare_new_live_workspace(
    settings: &LiveSettings,
    demo_mode: bool,
    run_id_override: &str,
) -> Result<PathBuf, String> {
    if demo_mode {
        return create_workspace(
            &settings.session_dir,
            ScenarioName::GoldenPathInteractive,
            Some(run_id_override),
        );
    }

    Ok(settings.workspace_root.clone())
}

pub(super) fn scenario_launch_metadata() -> LaunchMetadata {
    LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo")
}
