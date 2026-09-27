use super::*;
use crate::model_resolution::ModelResolution;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProfileModelMetadata {
    pub profile: String,
    pub profile_description: Option<String>,
    pub provider: String,
    pub provider_display_label: String,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: String,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
    pub display_label: String,
    pub token_window_label: Option<String>,
    pub limits: ResolvedModelLimits,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub resolution: ModelResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelCatalogEntry {
    pub provider: String,
    pub provider_display_label: String,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: String,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
    pub display_label: String,
    pub token_window_label: Option<String>,
    pub limits: ResolvedModelLimits,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub supports_reasoning_summaries: bool,
    pub resolution: ModelResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelTarget {
    pub model_ref: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub limits: ResolvedModelLimits,
    pub resolution: ModelResolution,
    pub catalog_entry: Option<Box<ResolvedModelCatalogEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelSelection {
    pub selector: String,
    pub profile: Option<String>,
    pub primary: ResolvedModelTarget,
    pub fallback: Vec<ResolvedModelTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelProfileCatalogEntry {
    pub name: String,
    pub primary: ResolvedModelTarget,
    pub fallback: Vec<ResolvedModelTarget>,
}
