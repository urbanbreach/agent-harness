use std::fs;
use std::path::{Path, PathBuf};

use harness_core::config::{load_resolved_config, HarnessConfig};
use harness_core::coord::CoordinatorConfig;

#[cfg(test)]
pub(crate) fn shipped_example_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs/harness.example.jsonc")
}

pub(crate) fn config_digest_for_paths(paths: &[PathBuf]) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    for path in paths {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        let config_bytes = fs::read(path)
            .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
        hasher.update(&config_bytes);
        hasher.update(&[0xff]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) struct LoadedCliConfig {
    pub(crate) config: HarnessConfig,
    pub(crate) digest: String,
}

pub(crate) fn load_optional_config_with_digest(
    config_path: Option<&Path>,
) -> Result<Option<LoadedCliConfig>, String> {
    let Some(loaded) = load_resolved_config(config_path).map_err(|err| err.to_string())? else {
        return Ok(None);
    };

    Ok(Some(LoadedCliConfig {
        digest: config_digest_for_paths(&loaded.paths)?,
        config: loaded.config,
    }))
}

pub(crate) fn apply_runtime_metadata(
    coordinator_config: &mut CoordinatorConfig,
    deterministic: bool,
    config_digest: &str,
) {
    coordinator_config.deterministic_store = deterministic;
    coordinator_config.hook_runtime_config.suppress_execution = deterministic;
    coordinator_config.config_digest = config_digest.to_string();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();
}
