use crate::config::{ResolvedModelLimits, ResolvedModelTarget};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

pub const META_FILE_NAME: &str = "meta.json";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionModeSource {
    InteractiveLive,
    InteractiveMock,
    Prompt,
    ScenarioFixture,
    ReplayOnly,
    #[default]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetadata {
    pub run_id: String,
    pub run_name: String,
    pub workspace_root: String,
    #[serde(default)]
    pub created_at: Option<String>,
    pub config_digest: String,
    pub harness_version: String,
    #[serde(default)]
    pub recorded_runtime_context: Option<RecordedRuntimeContext>,
    #[serde(default)]
    pub mode_source: Option<SessionModeSource>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordedRuntimeContext {
    pub profile: String,
    pub profile_description: Option<String>,
    pub provider: String,
    pub provider_display_label: Option<String>,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub variant: Option<String>,
    pub display_label: String,
    pub model_display_label: Option<String>,
    pub variant_display_label: Option<String>,
    pub token_window_label: Option<String>,
    pub model_limits: ResolvedModelLimits,
    pub last_request_budget: Option<crate::RequestBudgetSnapshot>,
    #[serde(skip_serializing)]
    pub context_window_tokens: Option<u32>,
    #[serde(skip_serializing)]
    pub max_input_tokens: Option<u32>,
    #[serde(skip_serializing)]
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub recommended_for: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub thinking: Option<serde_json::Value>,
}
impl RecordedRuntimeContext {
    pub fn from_model_target(profile: &str, target: &ResolvedModelTarget) -> Self {
        let catalog = target.catalog_entry.as_deref();
        Self {
            profile: profile.into(),
            provider: target.provider.clone(),
            model: target.model.clone(),
            variant: target.variant.clone(),
            model_limits: target.limits.clone(),
            reasoning_effort: target.reasoning_effort.clone(),
            text_verbosity: target.text_verbosity.clone(),
            thinking: target.thinking.clone(),
            display_label: catalog
                .map_or_else(|| target.model_ref.clone(), |c| c.display_label.clone()),
            provider_display_label: catalog.map(|c| c.provider_display_label.clone()),
            provider_backend_label: catalog.and_then(|c| c.provider_backend_label.clone()),
            model_display_label: catalog.map(|c| c.model_display_label.clone()),
            variant_display_label: catalog.and_then(|c| c.variant_display_label.clone()),
            token_window_label: catalog.and_then(|c| c.token_window_label.clone()),
            description: catalog.and_then(|c| c.description.clone()),
            recommended_for: catalog.and_then(|c| c.recommended_for.clone()),
            ..Self::default()
        }
    }
    pub fn from_profile_model(profile: &str, model: &str) -> Self {
        let parsed = crate::agent::AgentModelRef::parse(model);
        Self {
            profile: profile.into(),
            provider: parsed.provider_id,
            model: parsed.model_id,
            display_label: model.into(),
            ..Self::default()
        }
    }
    pub fn effective_model_limits(&self) -> ResolvedModelLimits {
        if self.model_limits.context_window_tokens().is_some() {
            self.model_limits.clone()
        } else {
            ResolvedModelLimits::compatibility_mirror(
                self.context_window_tokens,
                self.max_input_tokens,
                self.max_output_tokens,
            )
        }
    }
}
pub fn load_run_metadata(dir: &Path) -> Option<RunMetadata> {
    read_run_metadata(dir).ok().flatten()
}
pub fn read_run_metadata(dir: &Path) -> Result<Option<RunMetadata>, crate::store::EventStoreError> {
    let Some(value) = read_metadata_value(dir)? else {
        return Ok(None);
    };
    let mut metadata: RunMetadata = serde_json::from_value(value)?;
    if let Some(context) = &mut metadata.recorded_runtime_context {
        context.model_limits = context.effective_model_limits();
    }
    Ok(Some(metadata))
}
pub fn read_metadata_value(
    dir: &Path,
) -> Result<Option<serde_json::Value>, crate::store::EventStoreError> {
    let path = dir.join(META_FILE_NAME);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(meta)
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 1024 * 1024 =>
        {
            return Err(crate::store::EventStoreError::Invalid(
                "invalid session metadata file",
            ))
        }
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(crate::store::EventStoreError::Invalid(
            "session metadata exceeds 1 MiB",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if !value.is_object() {
        return Err(crate::store::EventStoreError::Invalid(
            "session metadata must be an object",
        ));
    }
    Ok(Some(value))
}
