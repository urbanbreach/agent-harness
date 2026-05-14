use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const DEFAULT_MODELS_DEV_URL: &str = "https://models.dev/api.json";
const DEFAULT_GENERATED_CATALOG_PATH: &str = "configs/provider-catalog.generated.json";
const HARNESS_USER_AGENT: &str = concat!("agent-harness/", env!("CARGO_PKG_VERSION"));
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Args, Clone)]
pub struct ModelProbeCommand {
    #[command(flatten)]
    source: ModelCatalogSourceOptions,

    #[command(flatten)]
    filters: ModelCatalogFilterOptions,

    /// Write the generated catalog fragment to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct ModelGenerateCommand {
    #[command(flatten)]
    source: ModelCatalogSourceOptions,

    #[command(flatten)]
    filters: ModelCatalogFilterOptions,

    /// Generated catalog artifact path to update.
    #[arg(long, default_value = DEFAULT_GENERATED_CATALOG_PATH)]
    output: PathBuf,
}

#[derive(Debug, Args, Clone, Default)]
pub struct GeneratedModelCatalogCommand {
    /// Write the embedded generated catalog to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ModelCatalogSourceOptions {
    /// Read models.dev-compatible JSON from a file instead of fetching the default URL.
    #[arg(long)]
    input: Option<PathBuf>,

    /// Read models.dev-compatible JSON from stdin instead of fetching the default URL.
    #[arg(long, default_value_t = false)]
    stdin: bool,

    /// Fetch models.dev-compatible JSON from this URL.
    #[arg(long, default_value = DEFAULT_MODELS_DEV_URL)]
    url: String,
}

#[derive(Debug, Args, Clone)]
struct ModelCatalogFilterOptions {
    /// Restrict generation to one or more provider ids.
    #[arg(long = "provider")]
    providers: Vec<String>,

    /// Include models that do not advertise tool-call support.
    #[arg(long, default_value_t = false)]
    include_non_tool: bool,

    /// Include models marked as deprecated.
    #[arg(long, default_value_t = false)]
    include_deprecated: bool,

    /// Emit low/medium/high reasoning variants for models that advertise reasoning.
    #[arg(long, default_value_t = false)]
    emit_reasoning_variants: bool,
}

pub fn execute(command: ModelProbeCommand) -> ExitCode {
    match execute_probe(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("model probe failed: {err}");
            ExitCode::from(1)
        }
    }
}

pub fn execute_generate(command: ModelGenerateCommand) -> ExitCode {
    match execute_generate_inner(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("model generate failed: {err}");
            ExitCode::from(1)
        }
    }
}

pub fn execute_generated(command: GeneratedModelCatalogCommand) -> ExitCode {
    match write_catalog_body(
        crate::generated_model_catalog::PROVIDER_CATALOG_JSON,
        command.output.as_ref(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("model generated failed: {err}");
            ExitCode::from(1)
        }
    }
}

fn execute_probe(command: ModelProbeCommand) -> Result<(), String> {
    let body = generate_catalog_body(&command.source, &command.filters)?;
    write_catalog_body(&body, command.output.as_ref())
}

fn execute_generate_inner(command: ModelGenerateCommand) -> Result<(), String> {
    let mut filters = command.filters;
    filters.emit_reasoning_variants = true;
    let body = generate_catalog_body(&command.source, &filters)?;
    write_catalog_body(&body, Some(&command.output))
}

fn generate_catalog_body(
    source: &ModelCatalogSourceOptions,
    filters: &ModelCatalogFilterOptions,
) -> Result<String, String> {
    validate_source_options(source)?;

    let source_label = ProbeSource::from_options(source).label();
    let source_body = read_source(source)?;
    let providers: BTreeMap<String, ModelsDevProvider> = serde_json::from_str(&source_body)
        .map_err(|err| format!("failed to parse models.dev JSON: {err}"))?;
    let catalog = build_catalog(&providers, filters, source_label);
    serde_json::to_string_pretty(&catalog)
        .map(|body| format!("{body}\n"))
        .map_err(|err| format!("failed to serialize generated catalog: {err}"))
}

fn validate_source_options(source: &ModelCatalogSourceOptions) -> Result<(), String> {
    if source.input.is_some() && source.stdin {
        return Err("use only one of --input or --stdin".to_string());
    }
    if source.input.is_some() && source.url != DEFAULT_MODELS_DEV_URL {
        return Err("--url cannot be combined with --input".to_string());
    }
    if source.stdin && source.url != DEFAULT_MODELS_DEV_URL {
        return Err("--url cannot be combined with --stdin".to_string());
    }
    Ok(())
}

fn write_catalog_body(body: &str, output: Option<&PathBuf>) -> Result<(), String> {
    if let Some(path) = output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        fs::write(path, body)
            .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    } else {
        print!("{body}");
    }
    Ok(())
}

fn read_source(source: &ModelCatalogSourceOptions) -> Result<String, String> {
    if let Some(path) = source.input.as_deref() {
        return fs::read_to_string(path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()));
    }
    if source.stdin {
        let mut body = String::new();
        io::stdin()
            .read_to_string(&mut body)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
        return Ok(body);
    }

    fetch_url(&source.url)
}

fn fetch_url(url: &str) -> Result<String, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("failed to create fetch runtime: {err}"))?;

    runtime.block_on(async move {
        let response = reqwest::Client::new()
            .get(url)
            .header(reqwest::header::USER_AGENT, HARNESS_USER_AGENT)
            .timeout(FETCH_TIMEOUT)
            .send()
            .await
            .map_err(|err| format!("failed to fetch {url}: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("fetch {url} returned HTTP {status}"));
        }
        response
            .text()
            .await
            .map_err(|err| format!("failed to read response from {url}: {err}"))
    })
}

fn build_catalog(
    providers: &BTreeMap<String, ModelsDevProvider>,
    filters: &ModelCatalogFilterOptions,
    source_label: String,
) -> HarnessCatalogFragment {
    let provider_filter: Vec<&str> = filters.providers.iter().map(String::as_str).collect();
    let mut catalog_providers = BTreeMap::new();

    for (provider_id, provider) in providers {
        if !provider_filter.is_empty() && !provider_filter.contains(&provider_id.as_str()) {
            continue;
        }

        let mut models = BTreeMap::new();
        for (model_id, model) in &provider.models {
            if !filters.include_non_tool && !model.tool_call {
                continue;
            }
            if !filters.include_deprecated && model.status.as_deref() == Some("deprecated") {
                continue;
            }

            models.insert(
                model_id.clone(),
                HarnessModelConfig::from_models_dev(
                    provider_id,
                    provider,
                    model_id,
                    model,
                    filters,
                    &source_label,
                ),
            );
        }

        if models.is_empty() {
            continue;
        }

        catalog_providers.insert(
            provider_id.clone(),
            HarnessProviderConfig::from_models_dev(provider_id, provider, models),
        );
    }

    HarnessCatalogFragment {
        schema: "./config.json".to_string(),
        provider: catalog_providers,
    }
}

#[derive(Debug, Serialize)]
struct HarnessCatalogFragment {
    #[serde(rename = "$schema")]
    schema: String,
    provider: BTreeMap<String, HarnessProviderConfig>,
}

#[derive(Debug, Serialize)]
struct HarnessProviderConfig {
    #[serde(rename = "type")]
    provider_type: &'static str,
    name: String,
    options: HarnessProviderOptions,
    models: BTreeMap<String, HarnessModelConfig>,
}

impl HarnessProviderConfig {
    fn from_models_dev(
        provider_id: &str,
        provider: &ModelsDevProvider,
        models: BTreeMap<String, HarnessModelConfig>,
    ) -> Self {
        Self {
            provider_type: "openai_compatible",
            name: provider.name.clone().unwrap_or_else(|| {
                provider
                    .id
                    .clone()
                    .unwrap_or_else(|| provider_id.to_string())
            }),
            options: HarnessProviderOptions {
                base_url: provider.api.clone().unwrap_or_default(),
                api_key_env: provider.env.clone(),
            },
            models,
        }
    }
}

#[derive(Debug, Serialize)]
struct HarnessProviderOptions {
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "apiKeyEnv", skip_serializing_if = "Vec::is_empty")]
    api_key_env: Vec<String>,
}

#[derive(Debug, Serialize)]
struct HarnessModelConfig {
    name: String,
    metadata: HarnessModelMetadata,
    limit: HarnessModelLimit,
    modalities: HarnessModelModalities,
    #[serde(skip_serializing_if = "Map::is_empty")]
    options: Map<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    variants: BTreeMap<String, HarnessModelVariant>,
}

impl HarnessModelConfig {
    fn from_models_dev(
        provider_id: &str,
        provider: &ModelsDevProvider,
        model_id: &str,
        model: &ModelsDevModel,
        filters: &ModelCatalogFilterOptions,
        source_label: &str,
    ) -> Self {
        let limit = HarnessModelLimit {
            context: model.limit.as_ref().and_then(|limit| limit.context),
            input: model.limit.as_ref().and_then(|limit| limit.input),
            output: model.limit.as_ref().and_then(|limit| limit.output),
        };
        let modalities = HarnessModelModalities {
            input: model
                .modalities
                .as_ref()
                .and_then(|modalities| modalities.input.clone())
                .unwrap_or_else(|| vec!["text".to_string()]),
            output: model
                .modalities
                .as_ref()
                .and_then(|modalities| modalities.output.clone())
                .unwrap_or_else(|| vec!["text".to_string()]),
        };

        let mut options = Map::new();
        options.insert(
            "modelsDev".to_string(),
            build_models_dev_options(source_label, provider_id, provider, model_id, model),
        );

        Self {
            name: model.name.clone().unwrap_or_else(|| model_id.to_string()),
            metadata: HarnessModelMetadata {
                family: model.family.clone(),
                release_stage: release_stage(model.status.as_deref()),
                context_window_tokens: limit.context,
                supports_tool_calls: Some(model.tool_call),
                supports_reasoning_summaries: None,
            },
            limit,
            modalities,
            options,
            variants: reasoning_variants(model.reasoning, filters.emit_reasoning_variants),
        }
    }
}

fn build_models_dev_options(
    source_label: &str,
    provider_id: &str,
    provider: &ModelsDevProvider,
    model_id: &str,
    model: &ModelsDevModel,
) -> Value {
    json!({
        "source": source_label,
        "provider": {
            "id": provider.id.as_deref().unwrap_or(provider_id),
            "npm": provider.npm,
            "api": provider.api,
            "doc": provider.doc,
            "override": model.provider,
        },
        "model": {
            "id": model.id.as_deref().unwrap_or(model_id),
            "releaseDate": model.release_date,
            "lastUpdated": model.last_updated,
            "status": model.status,
            "openWeights": model.open_weights,
            "knowledge": model.knowledge,
        },
        "capabilities": {
            "attachment": model.attachment,
            "reasoning": model.reasoning,
            "temperature": model.temperature,
            "toolCall": model.tool_call,
            "structuredOutput": model.structured_output,
            "interleaved": model.interleaved,
        },
        "cost": model.cost,
        "experimental": model.experimental,
    })
}

enum ProbeSource<'a> {
    File(&'a PathBuf),
    Stdin,
    Url(&'a str),
}

impl<'a> ProbeSource<'a> {
    fn from_options(source: &'a ModelCatalogSourceOptions) -> Self {
        if let Some(path) = source.input.as_ref() {
            Self::File(path)
        } else if source.stdin {
            Self::Stdin
        } else {
            Self::Url(&source.url)
        }
    }

    fn label(&self) -> String {
        match self {
            Self::File(path) => format!("file://{}", path.display()),
            Self::Stdin => "stdin".to_string(),
            Self::Url(url) => (*url).to_string(),
        }
    }
}

fn release_stage(status: Option<&str>) -> Option<&'static str> {
    match status {
        Some("deprecated") => Some("deprecated"),
        Some("alpha" | "beta") => Some("preview"),
        _ => None,
    }
}

fn reasoning_variants(
    reasoning: bool,
    emit_reasoning_variants: bool,
) -> BTreeMap<String, HarnessModelVariant> {
    if !reasoning || !emit_reasoning_variants {
        return BTreeMap::new();
    }

    [
        ("low", "Low", "low"),
        ("medium", "Medium", "medium"),
        ("high", "High", "high"),
    ]
    .into_iter()
    .map(|(id, name, effort)| {
        (
            id.to_string(),
            HarnessModelVariant {
                name: name.to_string(),
                metadata: HarnessVariantMetadata {
                    reasoning_effort: effort.to_string(),
                    recommended_for: format!("models.dev reasoning preset: {effort}"),
                },
            },
        )
    })
    .collect()
}

#[derive(Debug, Serialize)]
struct HarnessModelMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
    #[serde(rename = "releaseStage", skip_serializing_if = "Option::is_none")]
    release_stage: Option<&'static str>,
    #[serde(
        rename = "contextWindowTokens",
        skip_serializing_if = "Option::is_none"
    )]
    context_window_tokens: Option<u32>,
    #[serde(rename = "supportsToolCalls", skip_serializing_if = "Option::is_none")]
    supports_tool_calls: Option<bool>,
    #[serde(
        rename = "supportsReasoningSummaries",
        skip_serializing_if = "Option::is_none"
    )]
    supports_reasoning_summaries: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct HarnessModelLimit {
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<u32>,
}

#[derive(Debug, Serialize)]
struct HarnessModelModalities {
    input: Vec<String>,
    output: Vec<String>,
}

#[derive(Debug, Serialize)]
struct HarnessModelVariant {
    name: String,
    metadata: HarnessVariantMetadata,
}

#[derive(Debug, Serialize)]
struct HarnessVariantMetadata {
    #[serde(rename = "reasoningEffort")]
    reasoning_effort: String,
    #[serde(rename = "recommendedFor")]
    recommended_for: String,
}

#[derive(Debug, Deserialize)]
struct ModelsDevProvider {
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    doc: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, ModelsDevModel>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ModelsDevProviderOverride {
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    shape: Option<String>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevModel {
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    attachment: bool,
    #[serde(default)]
    reasoning: bool,
    #[serde(default = "default_true")]
    temperature: bool,
    #[serde(default)]
    tool_call: bool,
    #[serde(default)]
    structured_output: Option<bool>,
    #[serde(default)]
    interleaved: Option<Value>,
    #[serde(default)]
    knowledge: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    open_weights: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    cost: Option<ModelsDevCost>,
    #[serde(default)]
    limit: Option<ModelsDevLimit>,
    #[serde(default)]
    modalities: Option<ModelsDevModalities>,
    #[serde(default)]
    provider: Option<ModelsDevProviderOverride>,
    #[serde(default)]
    experimental: Option<Value>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
struct ModelsDevCost {
    input: f64,
    output: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning: Option<f64>,
    #[serde(
        default,
        rename = "cache_read",
        skip_serializing_if = "Option::is_none"
    )]
    cache_read: Option<f64>,
    #[serde(
        default,
        rename = "cache_write",
        skip_serializing_if = "Option::is_none"
    )]
    cache_write: Option<f64>,
    #[serde(
        default,
        rename = "input_audio",
        skip_serializing_if = "Option::is_none"
    )]
    input_audio: Option<f64>,
    #[serde(
        default,
        rename = "output_audio",
        skip_serializing_if = "Option::is_none"
    )]
    output_audio: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevLimit {
    context: Option<u32>,
    #[serde(default)]
    input: Option<u32>,
    output: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevModalities {
    #[serde(default)]
    input: Option<Vec<String>>,
    #[serde(default)]
    output: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_execute_generated_with_output_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        let command = GeneratedModelCatalogCommand {
            output: Some(path.clone()),
        };

        let result = execute_generated(command);
        assert_eq!(result, ExitCode::SUCCESS);

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, crate::generated_model_catalog::PROVIDER_CATALOG_JSON);
    }

    #[test]
    fn test_execute_generated_with_stdout() {
        let command = GeneratedModelCatalogCommand {
            output: None,
        };

        let result = execute_generated(command);
        assert_eq!(result, ExitCode::SUCCESS);
    }
}
