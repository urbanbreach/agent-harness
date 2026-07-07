use serde_json::Value;
use thiserror::Error;

use crate::{CompletionRequest, ToolDef};

mod gemini;
mod kimi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSchemaFamily {
    OpenAiCompatible,
    Gemini,
    Kimi,
}

impl ProviderSchemaFamily {
    const fn label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai-compatible",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderSchemaCompatibilityError {
    #[error("tool `{tool_id}` schema is not compatible with {family}: {reason}")]
    UnsupportedToolSchema {
        family: &'static str,
        tool_id: String,
        reason: &'static str,
    },
}

pub fn prepare_tools_for_family(
    family: ProviderSchemaFamily,
    tools: Vec<ToolDef>,
) -> Result<Vec<ToolDef>, ProviderSchemaCompatibilityError> {
    tools
        .into_iter()
        .map(|tool| prepare_tool_for_family(family, tool))
        .collect()
}

pub(crate) fn prepare_request_tools(
    mut request: CompletionRequest,
) -> Result<CompletionRequest, ProviderSchemaCompatibilityError> {
    if let Some(tools) = request.tools.take() {
        let family = family_for_request(request.provider_id.as_deref(), &request.model_id);
        request.tools = Some(prepare_tools_for_family(family, tools)?);
    }
    Ok(request)
}

pub fn family_for_request(provider_id: Option<&str>, model_id: &str) -> ProviderSchemaFamily {
    let provider = provider_id.unwrap_or_default().to_ascii_lowercase();
    let model = model_id.to_ascii_lowercase();
    if provider == "google" || model.contains("gemini") {
        return ProviderSchemaFamily::Gemini;
    }
    if provider == "moonshotai" || model.contains("kimi") {
        return ProviderSchemaFamily::Kimi;
    }
    ProviderSchemaFamily::OpenAiCompatible
}

fn prepare_tool_for_family(
    family: ProviderSchemaFamily,
    mut tool: ToolDef,
) -> Result<ToolDef, ProviderSchemaCompatibilityError> {
    validate_root_schema(family, &tool.tool_id, &tool.parameters)?;
    tool.parameters = match family {
        ProviderSchemaFamily::OpenAiCompatible => tool.parameters,
        ProviderSchemaFamily::Gemini => gemini::sanitize(tool.parameters),
        ProviderSchemaFamily::Kimi => kimi::sanitize(tool.parameters),
    };
    validate_root_schema(family, &tool.tool_id, &tool.parameters)?;
    Ok(tool)
}

fn validate_root_schema(
    family: ProviderSchemaFamily,
    tool_id: &str,
    schema: &Value,
) -> Result<(), ProviderSchemaCompatibilityError> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(error(family, tool_id, "expected top-level `type: object`"));
    }
    for key in ["oneOf", "anyOf", "allOf", "enum", "not"] {
        if schema.get(key).is_some() {
            return Err(error(
                family,
                tool_id,
                "top-level combinators (`oneOf`/`anyOf`/`allOf`/`enum`/`not`) are not allowed",
            ));
        }
    }
    Ok(())
}

fn error(
    family: ProviderSchemaFamily,
    tool_id: &str,
    reason: &'static str,
) -> ProviderSchemaCompatibilityError {
    ProviderSchemaCompatibilityError::UnsupportedToolSchema {
        family: family.label(),
        tool_id: tool_id.to_string(),
        reason,
    }
}
