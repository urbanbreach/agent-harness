use crate::{CliDeps, CliIo};
use harness_core::{config::*, provider_catalog::ProviderCatalog};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, path::PathBuf};

#[derive(clap::Args)]
pub(crate) struct CatalogCommand {
    #[arg(long, conflicts_with_all = ["stdin", "url"])]
    input: Option<PathBuf>,
    #[arg(long, conflicts_with = "url")]
    stdin: bool,
    #[arg(long)]
    url: Option<String>,
    #[arg(long = "provider")]
    providers: Vec<String>,
    #[arg(long)]
    include_non_tool: bool,
    #[arg(long)]
    include_deprecated: bool,
    #[arg(long)]
    emit_reasoning_variants: bool,
    #[arg(long)]
    output: Option<PathBuf>,
}

pub(crate) fn generated(
    output: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    write_catalog(
        include_bytes!("../../../configs/provider-catalog.generated.json"),
        output,
        io,
        deps,
    )
}
pub(crate) fn generate(
    command: CatalogCommand,
    generate: bool,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let catalog = if let Some(path) = &command.input {
        ProviderCatalog::from_path(&deps.current_dir().map_err(|e| e.to_string())?.join(path))
    } else if command.stdin {
        let mut body = String::new();
        io.stdin
            .take(16 * 1024 * 1024 + 1)
            .read_to_string(&mut body)
            .map_err(|e| e.to_string())?;
        ProviderCatalog::from_json(&body)
    } else {
        let url = command
            .url
            .clone()
            .unwrap_or_else(|| "https://models.dev/api.json".into());
        // The blocking catalog client must not create/drop its runtime inside a caller's Tokio runtime.
        std::thread::spawn(move || ProviderCatalog::fetch_from_url(&url))
            .join()
            .map_err(|_| "catalog fetch stopped unexpectedly")?
    }
    .map_err(|e| e.to_string())?;
    let mut providers = BTreeMap::<String, Value>::new();
    for provider in catalog
        .providers()
        .into_iter()
        .filter(|p| command.providers.is_empty() || command.providers.contains(&p.id))
    {
        let models: BTreeMap<_, _> = provider
            .models
            .iter()
            .filter_map(|(id, entry)| {
                if !command.include_non_tool
                    && entry.definition.metadata.supports_tool_calls != Some(true)
                    || !command.include_deprecated
                        && entry.definition.metadata.release_stage
                            == Some(ModelReleaseStage::Deprecated)
                {
                    return None;
                }
                let mut model = entry.definition.clone();
                if (generate || command.emit_reasoning_variants)
                    && model.variants.is_empty()
                    && model.metadata.supports_reasoning_summaries == Some(true)
                {
                    for effort in [
                        ModelVariantReasoningEffort::Low,
                        ModelVariantReasoningEffort::Medium,
                        ModelVariantReasoningEffort::High,
                    ] {
                        model.variants.insert(
                            effort.as_str().into(),
                            ModelVariantConfig {
                                metadata: ModelVariantMetadataConfig {
                                    reasoning_effort: Some(effort),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                }
                let provenance = entry.limits.primary_provenance();
                model.options.insert(
                    "modelsDev".into(),
                    json!({
                        "source": provenance.source,
                        "model": {"lastUpdated": provenance.verified_at}
                    }),
                );
                Some((id.clone(), model))
            })
            .collect();
        if models.is_empty() {
            continue;
        }
        providers.insert(provider.id.clone(), json!({
            "type":if provider.id == "anthropic" {"anthropic_messages"} else {"openai_compatible"},
            "name":provider.name,"options":{"baseURL":provider.base_url,"apiKeyEnv":provider.api_key_env}, "models":models,
        }));
    }
    if providers.is_empty() {
        return Err("catalog contains no provider with a usable model after filtering".into());
    }
    let body =
        serde_json::to_vec_pretty(&json!({"provider":providers})).map_err(|e| e.to_string())?;
    let output = command
        .output
        .or_else(|| generate.then(|| "configs/provider-catalog.generated.json".into()));
    write_catalog(&body, output, io, deps)
}
fn write_catalog(
    body: &[u8],
    output: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    if let Some(output) = output {
        let path = deps.current_dir().map_err(|e| e.to_string())?.join(output);
        harness_core::store::write_private_atomic(&path, body).map_err(|e| e.to_string())
    } else {
        io.stdout
            .write_all(body)
            .and_then(|()| writeln!(io.stdout))
            .map_err(|e| e.to_string())
    }
}
