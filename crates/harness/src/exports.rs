use crate::{CliDeps, CliIo};
use harness_core::{
    config::HarnessConfig,
    event::{EventEnvelopeV1, EventV1},
    redact::{redact_in_place, DefaultRedactor, Redactor},
    session::AssistantPart,
    transcript_projection::{project_transcript, ProjectedMessageRole, ProjectedPart},
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
mod journal;
pub(crate) use journal::export as journal;

#[derive(clap::Args)]
pub(crate) struct ExportCommand {
    pub session: String,
    #[arg(long)]
    pub output: Option<PathBuf>,
}
pub(crate) fn markdown(
    command: ExportCommand,
    config: Option<&Path>,
    directory: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let cwd = deps.current_dir().map_err(|e| e.to_string())?;
    let configured = crate::inspect::configured(config, deps)?;
    let root = cwd.join(directory.unwrap_or_else(|| configured.config.runtime.session_dir.clone()));
    let path = crate::recovery::resolve_session_run_dir(&command.session, &root, &cwd)?;
    let redactor = crate::inspect::redactor(&configured.config, deps)?;
    let events = crate::replay::read_bounded_history(&path)?;
    harness_core::proj::project_resume_plan(&events, &command.session)
        .map_err(|e| e.to_string())?;
    let transcript = project_transcript(&events).map_err(|e| e.to_string())?;
    let mut markdown = String::new();
    for message in transcript.messages {
        let role = match message.role {
            ProjectedMessageRole::User => "User",
            ProjectedMessageRole::Assistant => "Assistant",
            ProjectedMessageRole::System => continue,
        };
        let text: String = message
            .parts
            .into_iter()
            .filter_map(|part| match part {
                ProjectedPart::Text(text) => Some(text.text),
                _ => None,
            })
            .collect();
        if !text.is_empty() {
            markdown.push_str(&format!("## {role}\n\n{}\n\n", redactor.redact_text(&text)));
        }
    }
    if markdown.is_empty() {
        return Err("session has no conversation content to export".into());
    }
    check_text(&markdown, &redactor)?;
    write_output(
        markdown.as_bytes(),
        command.output.as_deref(),
        &path,
        io,
        deps,
    )
}

pub(crate) fn bundle(
    path: &Path,
    config: &HarnessConfig,
    include_events: bool,
) -> Result<Value, String> {
    let mut events = crate::replay::read_bounded_history(path)?;
    harness_core::proj::project_resume_plan(&events, "").map_err(|e| e.to_string())?;
    let catalog = crate::replay::inspect_session(path)?.catalog;
    let metadata = harness_core::proj::read_metadata_value(path).map_err(|e| e.to_string())?;
    let mut pending = std::collections::HashMap::new();
    events.retain_mut(|event| clean_event(event, &mut pending));
    let replay = harness_core::proj::project_run_summary(&events).map_err(|e| e.to_string())?;
    let native = harness_tools::coordinator_registry_with_skills(
        config.permissions.shell_allowlist.clone(),
        config.skills.clone(),
    );
    let mut bundle = json!({"schema_version":"harness-session-export-v1","run_dir":path,
    "catalog":catalog,"metadata":metadata,"replay":replay,
    "support":{
        "readiness_scope":"local_configuration","no_network_probes":true,"provider_execution_proof":false,
        "config_summary":config,
        "provider_summary":config.providers.keys().collect::<Vec<_>>(),
        "agent_catalog_summary":config.agents.keys().collect::<Vec<_>>(),
        "native_tool_catalog_summary":native.tool_ids(),
        "session_tool_readiness":harness_core::proj::inspect_resume_plan(path),
        "raw_tool_results_omitted":true,
        "secret_scan_status":{"status":"clean","secret_finding_count":0}
    }});
    if include_events {
        bundle["events"] = serde_json::to_value(events).map_err(|e| e.to_string())?;
    }
    Ok(bundle)
}

fn clean_event(
    event: &mut EventEnvelopeV1,
    pending: &mut std::collections::HashMap<String, String>,
) -> bool {
    match &mut event.payload {
        EventV1::ProviderStreamDelta(delta) => {
            pending
                .entry(delta.request_id.to_string())
                .or_default()
                .push_str(&delta.delta);
            false
        }
        EventV1::ProviderReasoningDelta(_) => false,
        EventV1::AssistantMessageFinished(message) => {
            if let Some(text) = pending.remove(message.request_id.as_str()) {
                if message.parts.is_empty() && !text.is_empty() {
                    message.parts.push(AssistantPart::Text { text });
                }
            }
            message
                .parts
                .retain(|part| !matches!(part, AssistantPart::Reasoning { .. }));
            true
        }
        EventV1::ToolCallFinished(tool) => {
            tool.output_json = None;
            true
        }
        _ => true,
    }
}

/// Checks object keys as well as values; redaction never silently renames a key.
pub(crate) fn checked_json(mut value: Value, redactor: &dyn Redactor) -> Result<Vec<u8>, String> {
    check_value(&mut value, redactor)?;
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn check_value(value: &mut Value, redactor: &dyn Redactor) -> Result<(), String> {
    redact_in_place(redactor, value);
    if harness_core::redact::has_unredacted_secret(redactor, value) {
        return Err("export secret scan failed".into());
    }
    Ok(())
}
fn check_text(text: &str, redactor: &dyn Redactor) -> Result<(), String> {
    if redactor.redact_text(text) != text
        || DefaultRedactor::default().secret_finding_count(text) != 0
    {
        Err("export secret scan failed".into())
    } else {
        Ok(())
    }
}
pub(crate) fn write_output(
    bytes: &[u8],
    output: Option<&Path>,
    source: &Path,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    if let Some(output) = output {
        let cwd = deps.current_dir().map_err(|e| e.to_string())?;
        let output = destination(&cwd.join(output), source)?;
        harness_core::store::write_private_atomic(&output, bytes).map_err(|e| e.to_string())
    } else {
        io.stdout.write_all(bytes).map_err(|e| e.to_string())
    }
}
fn destination(output: &Path, session: &Path) -> Result<PathBuf, String> {
    let output =
        harness_core::tool::resolve_file_path(Path::new("."), output).map_err(|e| e.to_string())?;
    let root = session
        .parent()
        .ok_or("session parent missing")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if output.starts_with(root) {
        Err("export destination must be outside the session root".into())
    } else {
        Ok(output)
    }
}
