use crate::config::{
    ModelConfig, ModelLimitConfig, ModelLimitError, ModelLimitProvenance, ModelMetadataConfig,
    ModelModalitiesConfig, ModelVariantConfig, ResolvedModelLimits,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{Arc, OnceLock},
};
mod cache;
mod json;
pub use json::duplicate_checked_json_value;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const EMBEDDED: &str = include_str!("../../../configs/provider-catalog.generated.json");

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    providers: Arc<BTreeMap<String, ProviderCatalogEntry>>,
    diagnostics: Arc<[CatalogDiagnostic]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogDiagnostic {
    pub provider: String,
    pub model: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCatalogEntry {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key_env: Vec<String>,
    pub models: BTreeMap<String, ModelCatalogEntry>,
    pub auth_methods: Vec<CatalogAuthMethod>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub name: String,
    pub limits: ResolvedModelLimits,
    pub definition: ModelConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CatalogAuthMethod {
    ApiKey,
    OAuth(OAuthFlow),
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OAuthFlow {
    BrowserPkce,
    DeviceCode,
}
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("catalog contains no provider with a usable model")]
    NoUsableModels,
}
#[derive(Debug, thiserror::Error)]
pub enum CatalogModelError {
    #[error("catalog model `{provider}:{model}` was not found")]
    NotFound { provider: String, model: String },
    #[error(transparent)]
    InvalidLimits(#[from] ModelLimitError),
}
impl ProviderCatalog {
    pub fn from_json(body: &str) -> Result<Self, CatalogError> {
        Self::parse(body, false, "file://<redacted>")
    }
    pub fn from_embedded() -> Result<Self, CatalogError> {
        static CATALOG: OnceLock<Result<ProviderCatalog, String>> = OnceLock::new();
        CATALOG
            .get_or_init(|| {
                Self::parse(EMBEDDED, true, "https://models.dev/api.json")
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(CatalogError::Invalid)
    }
    pub fn from_path(path: &Path) -> Result<Self, CatalogError> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Err(CatalogError::Invalid(
                "catalog must be a regular file".into(),
            ));
        }
        let mut body = String::new();
        std::fs::File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_string(&mut body)?;
        Self::parse(&body, false, "file://<redacted>")
    }
    pub fn providers(&self) -> Vec<&ProviderCatalogEntry> {
        self.providers.values().collect()
    }
    pub fn provider(&self, id: &str) -> Option<&ProviderCatalogEntry> {
        self.providers.get(id)
    }
    pub fn diagnostics(&self) -> &[CatalogDiagnostic] {
        &self.diagnostics
    }
    pub fn validated_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<&ModelCatalogEntry, CatalogModelError> {
        let entry = self
            .provider(provider)
            .and_then(|p| p.models.get(model))
            .ok_or_else(|| CatalogModelError::NotFound {
                provider: provider.into(),
                model: model.into(),
            })?;
        entry.limits.validate(&format!("{provider}:{model}"))?;
        Ok(entry)
    }
    pub fn sorted_by_priority(&self) -> Vec<&ProviderCatalogEntry> {
        let mut entries = self.providers();
        entries.sort_by_key(|p| match p.id.as_str() {
            "openai" | "codex" => 0,
            "github-copilot" => 1,
            "anthropic" => 2,
            "google" => 3,
            "openrouter" => 4,
            _ => 5,
        });
        entries
    }
    fn parse(body: &str, generated: bool, source: &str) -> Result<Self, CatalogError> {
        if body.len() as u64 > MAX_BYTES {
            return Err(CatalogError::Invalid("catalog exceeds 16 MiB".into()));
        }
        let value = duplicate_checked_json_value(body)
            .map_err(|_| CatalogError::Invalid("invalid catalog JSON".into()))?;
        let raw: BTreeMap<String, RawProvider> =
            serde_json::from_value(value.get("provider").cloned().unwrap_or(value))
                .map_err(|_| CatalogError::Invalid("invalid catalog providers".into()))?;
        let mut providers = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (id, raw) in raw {
            if crate::auth::ProviderId::parse(&id).is_none() {
                return Err(CatalogError::Invalid(
                    "invalid catalog provider identifier".into(),
                ));
            }
            let mut models = BTreeMap::new();
            for (model, value) in raw.models {
                match parse_model(&value, generated, source, &model, &format!("{id}:{model}")) {
                    Ok(entry) => {
                        models.insert(model, entry);
                    }
                    Err(message) => diagnostics.push(CatalogDiagnostic {
                        provider: id.clone(),
                        model,
                        message,
                    }),
                }
            }
            let mut auth_methods = vec![CatalogAuthMethod::ApiKey];
            if id == "codex" {
                auth_methods.push(CatalogAuthMethod::OAuth(OAuthFlow::BrowserPkce));
            }
            if matches!(id.as_str(), "codex" | "github-copilot") {
                auth_methods.push(CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode));
            }
            providers.insert(
                id.clone(),
                ProviderCatalogEntry {
                    name: raw.name.unwrap_or_else(|| id.clone()),
                    base_url: raw
                        .options
                        .base_url
                        .filter(|url| !url.trim().is_empty())
                        .or(raw.api.filter(|url| !url.trim().is_empty()))
                        .unwrap_or_else(|| match id.as_str() {
                            "openai" => "https://api.openai.com/v1".into(),
                            "anthropic" => "https://api.anthropic.com/v1".into(),
                            _ => String::new(),
                        }),
                    id,
                    api_key_env: if raw.options.api_key_env.is_empty() {
                        raw.env
                    } else {
                        raw.options.api_key_env
                    },
                    models,
                    auth_methods,
                },
            );
        }
        if providers.values().all(|p| p.models.is_empty()) {
            return Err(CatalogError::NoUsableModels);
        }
        Ok(Self {
            providers: Arc::new(providers),
            diagnostics: diagnostics.into(),
        })
    }
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProvider {
    name: Option<String>,
    api: Option<String>,
    env: Vec<String>,
    options: RawOptions,
    models: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawOptions {
    #[serde(rename = "baseURL", alias = "baseUrl")]
    base_url: Option<String>,
    api_key_env: Vec<String>,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawModel {
    name: String,
    family: Option<String>,
    status: Option<String>,
    limit: Limits,
    metadata: ModelMetadataConfig,
    modalities: ModelModalitiesConfig,
    variants: BTreeMap<String, ModelVariantConfig>,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    tool_call: Option<bool>,
    reasoning: Option<bool>,
    #[serde(alias = "lastUpdated")]
    last_updated: Option<String>,
    options: serde_json::Value,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Limits {
    context: Option<u64>,
    input: Option<u64>,
    output: Option<u64>,
}
fn parse_model(
    value: &serde_json::Value,
    generated: bool,
    source: &str,
    model: &str,
    identity: &str,
) -> Result<ModelCatalogEntry, String> {
    let mut raw: RawModel =
        serde_json::from_value(value.clone()).map_err(|_| "invalid model metadata".to_owned())?;
    let source = sanitize_catalog_source_origin(
        raw.options
            .pointer("/modelsDev/source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(source),
    );
    let verified = raw
        .last_updated
        .as_deref()
        .or_else(|| {
            raw.options
                .pointer("/modelsDev/model/lastUpdated")
                .and_then(serde_json::Value::as_str)
        })
        .and_then(sanitize_catalog_verified_at);
    let provenance = if generated {
        ModelLimitProvenance::generated(source, verified)
    } else {
        ModelLimitProvenance::discovered(source, verified)
    };
    let combine = |a, b| match (a, b) {
        (Some(a), Some(b)) if a != b => Err("conflicting model limits".to_owned()),
        _ => Ok(a.or(b)),
    };
    let limits = checked_catalog_limits(
        combine(
            raw.limit.context,
            raw.metadata.context_window_tokens.map(u64::from),
        )?,
        combine(raw.limit.input, raw.max_input_tokens)?,
        combine(raw.limit.output, raw.max_output_tokens)?,
        provenance,
        identity,
    )?;
    raw.metadata.family = raw.metadata.family.or(raw.family);
    if raw.status.as_deref() == Some("deprecated") {
        raw.metadata.release_stage = Some(crate::config::ModelReleaseStage::Deprecated);
    }
    raw.metadata.supports_tool_calls = raw.metadata.supports_tool_calls.or(raw.tool_call);
    raw.metadata.supports_reasoning_summaries = raw
        .metadata
        .supports_reasoning_summaries
        .or(raw.reasoning)
        .or_else(|| {
            raw.options
                .pointer("/modelsDev/capabilities/reasoning")
                .and_then(serde_json::Value::as_bool)
        });
    let name = if raw.name.is_empty() {
        model.into()
    } else {
        raw.name
    };
    let mut options = raw.options.as_object().cloned().unwrap_or_default();
    // Keep request options, not a second copy of catalog cost/provenance data.
    options.remove("modelsDev");
    options.remove("catalogSource");
    let definition = ModelConfig {
        display_name: name.clone(),
        limit: ModelLimitConfig {
            context: limits.context_window_tokens(),
            input: limits.max_input_tokens(),
            output: limits.max_output_tokens(),
        },
        limit_provenance: limits.context_window.provenance.clone(),
        metadata: raw.metadata,
        modalities: raw.modalities,
        variants: raw.variants,
        options: options.into_iter().collect(),
        ..Default::default()
    };
    Ok(ModelCatalogEntry {
        name,
        limits,
        definition,
    })
}
pub fn checked_catalog_limits(
    context: Option<u64>,
    input: Option<u64>,
    output: Option<u64>,
    mut provenance: ModelLimitProvenance,
    identity: &str,
) -> Result<ResolvedModelLimits, String> {
    provenance.verified_at = provenance
        .verified_at
        .as_deref()
        .and_then(sanitize_catalog_verified_at);
    let convert = |value: Option<u64>| {
        value
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "model limit exceeds u32".to_owned())
    };
    let limits = ResolvedModelLimits::from_values(
        convert(context)?,
        convert(input)?,
        convert(output)?,
        provenance,
    );
    limits.validate(identity).map_err(|e| e.to_string())?;
    if !limits.is_selectable_known() {
        return Err("model context and output limits are unknown".into());
    }
    Ok(limits)
}
pub fn sanitize_catalog_verified_at(value: &str) -> Option<String> {
    let date = match value.len() {
        4 => format!("{value}-01-01"),
        7 => format!("{value}-01"),
        10 => value.into(),
        _ => return None,
    };
    humantime::parse_rfc3339(&format!("{date}T00:00:00Z"))
        .ok()
        .map(|_| value.into())
}
pub fn sanitize_catalog_source_origin(value: &str) -> String {
    if value.starts_with("file:") {
        return "file://<redacted>".into();
    }
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return "<redacted>".into();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return "<redacted>".into();
    }
    // Paths and query strings may both contain access tokens. Provenance only needs an origin.
    url.origin().ascii_serialization()
}
