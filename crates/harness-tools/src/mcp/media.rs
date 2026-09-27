use super::*;
use base64::Engine;
use harness_core::attachment_transport::{AttachmentMetadata, MAX_ATTACHMENTS};

pub(super) fn extract(value: &mut Value, id: &str) -> Result<Vec<AttachmentMetadata>, ToolError> {
    let mut attachments = Vec::new();
    visit(value, id, &mut attachments)?;
    Ok(attachments)
}

fn visit(
    value: &mut Value,
    id: &str,
    attachments: &mut Vec<AttachmentMetadata>,
) -> Result<(), ToolError> {
    if let Value::Array(items) = value {
        for item in items {
            visit(item, id, attachments)?;
        }
        return Ok(());
    }
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let mime = object
        .get("mimeType")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("image/"))
        .map(str::to_owned);
    if let Some(mime) = mime {
        let key = if object.get("type").and_then(Value::as_str) == Some("image") {
            "data"
        } else {
            "blob"
        };
        if let Some(data) = object.remove(key) {
            if attachments.len() == MAX_ATTACHMENTS {
                return Err(failure("MCP returned too many attachments"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.as_str().ok_or_else(|| failure("invalid MCP image"))?)
                .map_err(|_| failure("invalid MCP image encoding"))?;
            let attachment_id = format!("{id}-{}", attachments.len());
            attachments.push(crate::media::attachment(
                attachment_id.clone(),
                &mime,
                &bytes,
            )?);
            object.insert("attachment_id".into(), attachment_id.into());
        }
    }
    for key in ["content", "contents", "messages", "resource"] {
        if let Some(child) = object.get_mut(key) {
            visit(child, id, attachments)?;
        }
    }
    Ok(())
}
