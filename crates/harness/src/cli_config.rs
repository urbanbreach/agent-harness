use harness_core::{
    config::{load_resolved_config_with_context, ConfigLoadContext, HarnessConfig},
    coord::CoordinatorConfig,
};
use std::path::{Path, PathBuf};

pub(crate) struct LoadedCliConfig {
    pub(crate) config: HarnessConfig,
    pub(crate) digest: String,
}
pub(crate) fn shipped_example_config_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/harness.example.jsonc")
}
pub(crate) fn load_optional_config_with_digest_context(
    path: Option<&Path>,
    context: &ConfigLoadContext,
) -> Result<Option<LoadedCliConfig>, String> {
    load_resolved_config_with_context(path, context)
        .map_err(|e| e.to_string())?
        .map(|loaded| {
            let digest =
                blake3::hash(&serde_json::to_vec(&loaded.config).map_err(|e| e.to_string())?)
                    .to_hex()
                    .to_string();
            Ok(LoadedCliConfig {
                config: loaded.config,
                digest,
            })
        })
        .transpose()
}
pub(crate) fn apply_runtime_metadata(
    config: &mut CoordinatorConfig,
    deterministic: bool,
    digest: &str,
) {
    config.deterministic_store = deterministic;
    config.hook_runtime_config.suppress_execution |= deterministic;
    config.config_digest = digest.into();
    config.harness_version = env!("CARGO_PKG_VERSION").into();
}
