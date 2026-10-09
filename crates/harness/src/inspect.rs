use crate::{CliDeps, CliIo};
use clap::{Args, Subcommand};
use harness_core::{
    auth::CredentialStore,
    config::*,
    redact::{DefaultRedactor, Redactor, SecretRedactor, SecretRegistry},
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Args)]
pub(crate) struct DoctorCommand {
    #[arg(long)]
    json: bool,
}
#[derive(Args)]
pub(crate) struct ModelsCommand {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<ModelsSubcommand>,
}
#[derive(Subcommand)]
enum ModelsSubcommand {
    List,
    Generated {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Generate(crate::models::CatalogCommand),
    Probe(crate::models::CatalogCommand),
}
#[derive(Args)]
pub(crate) struct ProvidersCommand {
    #[command(subcommand)]
    command: Protocols,
}
#[derive(Subcommand)]
enum Protocols {
    Protocols,
}

pub(crate) fn configured(
    path: Option<&Path>,
    deps: &CliDeps,
) -> Result<crate::runtime_catalog::RuntimeCatalogResolution, String> {
    let context = deps.config_load_context();
    let loaded = load_resolved_config_with_lookup(path, &context, &|name| deps.env_var_value(name))
        .map_err(|e| e.to_string())?;
    let store = CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    crate::runtime_catalog::resolve_runtime_catalog(
        loaded.map(|loaded| loaded.config),
        None,
        None,
        &context,
        store.as_ref(),
        &|name| deps.env_var_value(name),
    )
}
pub(crate) fn redactor(config: &HarnessConfig, deps: &CliDeps) -> Result<SecretRedactor, String> {
    let store = CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let registry = Arc::new(SecretRegistry::default());
    registry.register(crate::bootstrap::secret_values(
        config,
        deps,
        store.as_ref(),
    )?)?;
    Ok(SecretRedactor::new(
        Arc::new(DefaultRedactor::default()),
        registry,
    ))
}
pub(crate) fn print_json(io: &mut CliIo<'_>, value: &impl serde::Serialize) -> Result<(), String> {
    serde_json::to_writer_pretty(&mut io.stdout, value).map_err(|e| e.to_string())?;
    writeln!(io.stdout).map_err(|e| e.to_string())
}
pub(crate) fn models(
    command: ModelsCommand,
    path: Option<&Path>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    match command.command {
        Some(ModelsSubcommand::Generated { output }) => {
            return crate::models::generated(output, io, deps)
        }
        Some(ModelsSubcommand::Generate(command)) => {
            return crate::models::generate(command, true, io, deps)
        }
        Some(ModelsSubcommand::Probe(command)) => {
            return crate::models::generate(command, false, io, deps)
        }
        Some(ModelsSubcommand::List) | None => {}
    }
    let resolved = configured(path, deps)?;
    let redactor = redactor(&resolved.config, deps)?;
    let catalog = configured_model_catalog(&resolved.config);
    if catalog.is_empty() {
        return Err("No models configured. Connect a provider with `harness auth login` or configure a model.".into());
    }
    if command.json {
        let mut value = serde_json::to_value(&catalog).map_err(|e| e.to_string())?;
        harness_core::redact::redact_in_place(&redactor, &mut value);
        return print_json(io, &value);
    }
    for entry in catalog {
        let label = redactor.redact_text(&format!(
            "{}:{}{}",
            entry.provider,
            entry.model,
            entry
                .variant
                .as_ref()
                .map_or_else(String::new, |v| format!("/{v}"))
        ));
        writeln!(
            io.stdout,
            "{label} {}",
            serde_json::to_string(&entry.limits).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(crate) fn protocols(_: ProvidersCommand, io: &mut CliIo<'_>) -> Result<(), String> {
    print_json(
        io,
        &json!([
            {"protocol":"open_ai_compatible","support":"supported","apis":["responses","chat_completions"]},
            {"protocol":"anthropic_messages","support":"supported","apis":["messages"]}
        ]),
    )
}
pub(crate) fn doctor(
    command: DoctorCommand,
    path: Option<&Path>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let resolved = configured(path, deps)?;
    let config = &resolved.config;
    let catalog = configured_model_catalog(config);
    let unknown = catalog
        .iter()
        .filter(|entry| !entry.limits.is_selectable_known())
        .count();
    let mut checks = vec![
        json!({"name":"configuration","status":"pass","message":"Configuration is valid."}),
        json!({"name":"providers","status":if resolved.no_provider_connected {"fail"} else {"pass"},
            "message":if resolved.no_provider_connected {"No provider connected. Run `harness auth login <provider>`, use /login in the TUI, or set a provider API key such as OPENAI_API_KEY or ANTHROPIC_API_KEY."} else {"Provider configuration is present; live access is unverified."}}),
        json!({"name":"model_limits","status":if unknown > 0 {"warn"} else {"pass"},"message":format!("{} model selections; {unknown} have unknown context or output limits.",catalog.len())}),
    ];
    let workspace = deps.current_dir().map_err(|e| e.to_string())?;
    if !workspace.is_dir() {
        checks.push(json!({"name":"workspace","status":"fail","message":"Working directory is unavailable."}));
    }
    let failed = checks.iter().any(|check| check["status"] == "fail");
    let mut report = json!({"no_network_probes":true,"provider_execution_proof":false,
        "readiness_scope":"local_configuration","checks":checks});
    harness_core::redact::redact_in_place(&redactor(config, deps)?, &mut report);
    if command.json {
        print_json(io, &report)?;
    } else if let Some(checks) = report["checks"].as_array() {
        for check in checks {
            let text = |key| check.get(key).and_then(Value::as_str).unwrap_or("");
            writeln!(
                io.stdout,
                "{} {}: {}",
                text("status").to_ascii_uppercase(),
                text("name"),
                text("message")
            )
            .map_err(|e| e.to_string())?;
        }
    }
    if failed {
        Err("local readiness checks failed".into())
    } else {
        Ok(())
    }
}
