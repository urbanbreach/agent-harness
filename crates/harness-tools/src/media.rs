use harness_core::{
    attachment_transport::AttachmentMetadata,
    tool::{ToolContext, ToolError, ToolResult},
};

// File and web readers call this from their joined blocking worker.
pub(crate) fn artifact(
    ctx: &ToolContext,
    bytes: Vec<u8>,
    mut metadata: serde_json::Value,
) -> Result<ToolResult, ToolError> {
    let kind = if bytes.starts_with(b"%PDF-") {
        "PDF"
    } else {
        "binary"
    };
    let size = bytes.len();
    let artifact = tokio::runtime::Handle::current()
        .block_on(
            ctx.coordinator
                .retain_tool_binary(ctx.tool_call_id.to_string(), bytes),
        )
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    metadata["artifact"] =
        serde_json::to_value(&artifact).map_err(|e| ToolError::Execution(e.to_string()))?;
    metadata["kind"] = kind.to_ascii_lowercase().into();
    Ok(ToolResult::structured_with_artifacts(
        format!(
            "Retained {kind} artifact ({size} bytes); contents were not extracted.\nArtifact: {}",
            artifact.path
        ),
        metadata,
        vec![artifact],
    ))
}

pub(crate) fn mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else {
        None
    }
}

pub(crate) fn attachment(
    id: String,
    mime: &str,
    bytes: &[u8],
) -> Result<AttachmentMetadata, ToolError> {
    let attachment = AttachmentMetadata::from_bytes(id, mime, None, bytes, None);
    attachment
        .bytes()
        .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
    Ok(attachment)
}
