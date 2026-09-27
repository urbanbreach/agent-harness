use super::*;
use harness_core::{
    event::EventEnvelopeV1,
    proj::{SessionCatalogEntry, SessionCatalogMetadata},
};
use std::collections::BTreeMap;

pub(super) struct History {
    pub events: Vec<EventEnvelopeV1>,
    pub catalog: SessionCatalogEntry,
    pub counts: harness_core::proj::RunCounts,
}
pub(super) fn root(
    ctx: &ToolContext,
    default: &Path,
    selected: Option<&Path>,
) -> Result<PathBuf, ToolError> {
    selected.map_or_else(
        || Ok(default.to_path_buf()),
        |path| ctx.resolve_workspace_path(path),
    )
}
pub(super) fn directory(
    ctx: &ToolContext,
    root: &Path,
    selector: &str,
) -> Result<PathBuf, ToolError> {
    if harness_core::store::validate_session_id(selector).is_ok() {
        return Ok(root.join(selector));
    }
    let path = Path::new(selector);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid("session path cannot contain .."));
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.workspace_root.join(path)
    };
    let name = path
        .strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .ok_or_else(|| invalid("session must be an immediate child of sessionRoot"))?;
    harness_core::store::validate_session_id(name).map_err(invalid)?;
    Ok(path)
}
pub(super) fn directories(ctx: &ToolContext, root: &Path) -> Result<Vec<PathBuf>, ToolError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        cancelled(ctx)?;
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("events.jsonl").exists() {
            if dirs.len() == 2000 {
                return Err(invalid(
                    "sessionRoot exceeds the 2000-session scan limit; select a session",
                ));
            }
            dirs.push(entry.path());
        }
    }
    dirs.sort_unstable();
    Ok(dirs)
}
pub(super) fn load(ctx: &ToolContext, dir: &Path) -> Result<History, ToolError> {
    cancelled(ctx)?;
    let id = dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid session name"))?;
    harness_core::store::validate_session_id(id).map_err(invalid)?;
    let path = dir.join("events.jsonl");
    let length = harness_core::store::open_private_file(&path)?
        .metadata()?
        .len();
    if length > 64 * 1024 * 1024 {
        return Err(invalid(
            "session journal exceeds the 64 MiB inspection limit",
        ));
    }
    let mut events = Vec::new();
    for event in harness_core::store::JournalReader::open(&path, length).map_err(invalid)? {
        cancelled(ctx)?;
        events.push(event.map_err(|_| invalid("invalid session journal"))?);
    }
    let metadata = harness_core::proj::read_run_metadata(dir)
        .map_err(invalid)?
        .as_ref()
        .map(SessionCatalogMetadata::from);
    let mut catalog = harness_core::proj::project_session_catalog_entry(
        &events,
        id,
        metadata.as_ref(),
        None,
        None,
    )
    .map_err(invalid)?;
    if catalog.run_id != id {
        return Err(invalid("session directory and journal identity disagree"));
    }
    catalog.run_name = catalog.run_name.map(|s| safe(ctx, &s));
    catalog.workspace_root = catalog.workspace_root.map(|s| safe(ctx, &s));
    let counts = harness_core::proj::project_run_summary(&events)
        .map_err(invalid)?
        .counts;
    Ok(History {
        events,
        catalog,
        counts,
    })
}
pub(super) fn safe(ctx: &ToolContext, text: &str) -> String {
    let mut text = ctx.redactor.redact_text(text);
    if text.len() > 8192 {
        text.truncate(text.floor_char_boundary(8192));
        text.push_str(" [shortened]");
    }
    text
}
pub(super) fn document(event: &EventEnvelopeV1) -> Option<std::borrow::Cow<'_, str>> {
    use harness_core::{event::EventV1, session::AssistantPart};
    let text: &str = match &event.payload {
        EventV1::RunStarted(e) => e.run_name.as_str(),
        EventV1::SessionTitleUpdated(e) => &e.title,
        EventV1::RunFinished(e) => &e.summary,
        EventV1::RunFailed(e) => &e.error,
        EventV1::UserMessageSubmitted(e) => &e.text,
        EventV1::ToolCallFinished(e) => e.output_summary.as_deref()?,
        EventV1::TaskCompleted(e) => &e.result_summary,
        EventV1::TaskCancelled(e) => &e.reason,
        EventV1::AssistantMessageFinished(e) => {
            return Some(std::borrow::Cow::Owned(
                e.parts
                    .iter()
                    .filter_map(|part| match part {
                        AssistantPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect(),
            ))
        }
        _ => return None,
    };
    Some(std::borrow::Cow::Borrowed(text))
}
pub(crate) fn summary(ctx: &ToolContext, event: &EventEnvelopeV1) -> Value {
    use harness_core::event::EventV1;
    let mut value = json!({"seq":event.seq,"event_id":event.event_id,"event_type":event.payload.event_type(),"mono_ms":event.mono_ms});
    match &event.payload {
        EventV1::UserMessageSubmitted(e) => {
            value["role"] = "user".into();
            value["request_id"] = e.request_id.to_string().into();
        }
        EventV1::AssistantMessageFinished(e) => {
            value["role"] = "assistant".into();
            value["request_id"] = e.request_id.to_string().into();
        }
        EventV1::ToolCallRequested(e) => {
            value["tool_id"] = safe(ctx, &e.tool_id).into();
            value["tool_call_id"] = e.tool_call_id.to_string().into();
        }
        EventV1::ToolCallFinished(e) => {
            value["status"] = json!(e.status);
            value["tool_call_id"] = e.tool_call_id.to_string().into();
        }
        EventV1::ArtifactWritten(e) => {
            value["path"] = safe(ctx, &e.path).into();
            value["bytes"] = e.bytes.into();
        }
        _ => {}
    }
    if let Some(text) = document(event) {
        value["text"] = safe(ctx, &text).into();
    }
    value
}
pub(super) fn artifacts(ctx: &ToolContext, events: &[EventEnvelopeV1]) -> Value {
    let mut artifacts = BTreeMap::new();
    for event in events {
        if let harness_core::event::EventV1::ArtifactWritten(e) = &event.payload {
            if artifacts.len() < 200 {
                artifacts.insert(safe(ctx, &e.path), e.bytes);
            }
        }
    }
    json!({"artifacts":artifacts,"limit":200})
}
pub(super) fn cancelled(ctx: &ToolContext) -> Result<(), ToolError> {
    if ctx.cancellation.is_cancelled() {
        Err(ToolError::Cancelled)
    } else {
        Ok(())
    }
}
