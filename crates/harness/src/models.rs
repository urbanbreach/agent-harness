use crate::UnwrapOrAbort;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::auth::CredentialStore;
use harness_core::config::{configured_model_catalog, load_resolved_config_with_context};

use crate::model_probe::{GeneratedModelCatalogCommand, ModelGenerateCommand, ModelProbeCommand};

#[derive(Debug, Args, Clone, Default)]
pub struct ModelsCommand {
    #[command(subcommand)]
    command: Option<ModelsSubcommand>,
}

#[derive(Debug, Subcommand, Clone)]
enum ModelsSubcommand {
    /// List configured models and their resolved token limits.
    List,
    /// Write the generated provider catalog artifact.
    Generate(ModelGenerateCommand),
    /// Print the generated provider catalog embedded in this build.
    Generated(GeneratedModelCatalogCommand),
    /// Probe models.dev capability data and emit a harness provider catalog fragment.
    Probe(ModelProbeCommand),
}

pub fn execute_with_io(
    cmd: ModelsCommand,
    config_path: Option<PathBuf>,
    io: &mut crate::CliIo<'_>,
    deps: &crate::CliDeps,
) -> i32 {
    execute_with_writers(cmd, config_path, io.stdout, io.stderr, deps)
}

fn execute_with_writers(
    cmd: ModelsCommand,
    config_path: Option<PathBuf>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    deps: &crate::CliDeps,
) -> i32 {
    if let Some(command) = cmd.command {
        match command {
            ModelsSubcommand::List => {}
            ModelsSubcommand::Generate(command) => {
                return crate::model_probe::execute_generate_with_writers(command, stdout, stderr);
            }
            ModelsSubcommand::Generated(command) => {
                return crate::model_probe::execute_generated_with_writers(command, stdout, stderr);
            }
            ModelsSubcommand::Probe(command) => {
                return crate::model_probe::execute_probe_with_writers(command, stdout, stderr);
            }
        }
    }

    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(stderr, "failed to resolve config context: {err}");
            return 2;
        }
    };
    let loaded = match load_resolved_config_with_context(config_path.as_deref(), &config_context) {
        Ok(loaded) => loaded,
        Err(err) => {
            let _ = writeln!(stderr, "failed to load config: {err}");
            return 1;
        }
    };

    let credential_store = CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let runtime_catalog = match crate::runtime_catalog::resolve_runtime_catalog(
        loaded.as_ref().map(|loaded| loaded.config.clone()),
        loaded.as_ref().map(|_| "configured".to_string()),
        None,
        credential_store.as_ref(),
        &|name| deps.env_var_value(name),
    ) {
        Ok(runtime_catalog) => runtime_catalog,
        Err(err) => {
            let _ = writeln!(stderr, "failed to resolve runtime model catalog: {err}");
            return 1;
        }
    };

    if loaded.is_none() && !runtime_catalog.has_connected_provider() {
        let _ = writeln!(stderr, "models unavailable: Connect a provider with `harness auth login`, `/auth`, or `/connect` to list authenticated built-in models.");
        return 2;
    }

    let connected_filter = if loaded.is_none() {
        Some(
            runtime_catalog
                .connected_provider_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
        )
    } else {
        None
    };

    for entry in configured_model_catalog(&runtime_catalog.config) {
        if connected_filter
            .as_ref()
            .is_some_and(|connected| !connected.contains(entry.provider.as_str()))
        {
            continue;
        }
        let mut segments = vec![format!("{}:{}", entry.provider, entry.model)];

        if let Some(variant) = entry.variant.as_deref() {
            segments.push(format!("variant={variant}"));
        }
        segments.push(format!("label={}", entry.display_label));
        if let Some(reasoning_effort) = entry.reasoning_effort.as_deref() {
            segments.push(format!("reasoning={reasoning_effort}"));
        }
        if let Some(text_verbosity) = entry.text_verbosity.as_deref() {
            segments.push(format!("verbosity={text_verbosity}"));
        }
        if let Some(token_window_label) = entry.token_window_label.as_deref() {
            segments.push(format!("tokens={token_window_label}"));
        }
        segments.push(format!(
            "context={}",
            entry
                .limits
                .context_window_tokens()
                .map_or_else(|| "unknown".to_string(), |value| value.to_string())
        ));
        segments.push(format!(
            "context_provenance={}",
            entry.limits.context_window.provenance.kind.as_str()
        ));
        segments.push(format!(
            "max_input={}",
            entry
                .limits
                .max_input_tokens()
                .map_or_else(|| "unknown".to_string(), |value| value.to_string())
        ));
        segments.push(format!(
            "max_input_provenance={}",
            entry.limits.max_input.provenance.kind.as_str()
        ));
        segments.push(format!(
            "max_output={}",
            entry
                .limits
                .max_output_tokens()
                .map_or_else(|| "unknown".to_string(), |value| value.to_string())
        ));
        segments.push(format!(
            "max_output_provenance={}",
            entry.limits.max_output.provenance.kind.as_str()
        ));
        segments.push(format!(
            "max_input_semantics={}",
            entry.limits.max_input_semantics.as_str()
        ));

        let _ = writeln!(stdout, "{}", segments.join(" | "));
    }

    0
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use harness_core::auth::{
        AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
    };

    use super::{execute_with_writers, ModelsCommand};

    #[test]
    fn no_config_models_without_provider_prints_connect_guidance() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let deps = crate::CliDeps::real()
            .with_current_dir(temp.path().to_path_buf())
            .with_env(
                "HARNESS_DATA_HOME",
                temp.path().join("data").to_string_lossy(),
            );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = execute_with_writers(
            ModelsCommand::default(),
            None,
            &mut stdout,
            &mut stderr,
            &deps,
        );

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(String::from_utf8(stderr)
            .unwrap_or_abort()
            .contains("Connect a provider"));
    }

    #[test]
    fn no_config_models_with_stored_copilot_lists_builtin_provider() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let data_home = temp.path().join("data");
        CredentialStore::new(data_home.join("harness"))
            .save(&StoredCredential::api_key(
                AuthProviderId::github_copilot(),
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .unwrap_or_abort();
        let deps = crate::CliDeps::real()
            .with_current_dir(temp.path().to_path_buf())
            .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = execute_with_writers(
            ModelsCommand::default(),
            None,
            &mut stdout,
            &mut stderr,
            &deps,
        );

        assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
        let stdout = String::from_utf8(stdout).unwrap_or_abort();
        assert!(stdout.contains("github-copilot:"), "{stdout}");
        assert!(stdout.contains("label="), "{stdout}");
        assert!(stderr.is_empty());
    }
}
