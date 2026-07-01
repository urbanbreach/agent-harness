use harness_core::tool::ToolError;
use serde::de::DeserializeOwned;

pub(crate) fn parse_tool_args<T: DeserializeOwned>(
    args_json: serde_json::Value,
) -> Result<T, ToolError> {
    serde_json::from_value(args_json).map_err(|err| {
        ToolError::InvalidArguments(format!(
            "The tool call arguments are invalid. Rewrite the JSON arguments to match this tool's schema. Details: {err}"
        ))
    })
}
