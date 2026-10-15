use std::io::Read;
use std::path::{Path, PathBuf};

use harness_core::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, EventV1, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::foreign_session::{ForeignImportResult, ForeignSessionCandidate};
use harness_core::session::AssistantPart;
use harness_core::store::{open_private_file, EventEnvelopeWithoutSeqV1, EventStore, Journal};
use serde_json::{json, Value};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 65_536;

struct Message {
    user: bool,
    text: String,
}

fn safe_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err("foreign session paths cannot contain symlinks".into())
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string())
            }
            _ => {}
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    safe_path(path)?;
    let file = open_private_file(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_BYTES {
        return Err("foreign journal exceeds 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("foreign journal exceeds 64 MiB".into());
    }
    Ok(bytes)
}

fn text(content: &Value, provider: &str) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    let mut text = String::new();
    for part in content.as_array().into_iter().flatten() {
        let visible = match provider {
            "claude" => part["type"] == "text",
            _ => matches!(part["type"].as_str(), Some("input_text" | "output_text")),
        };
        if visible && let Some(value) = part["text"].as_str() {
            text.push_str(value);
        }
    }
    text
}

fn parse(bytes: &[u8]) -> Result<(&'static str, Vec<Message>), String> {
    let mut messages = Vec::new();
    let mut event_messages = Vec::new();
    let mut provider = None;
    let mut records = 0;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        records += 1;
        if records > MAX_RECORDS {
            return Err("foreign journal exceeds 65536 records".into());
        }
        let record: Value = serde_json::from_slice(line)
            .map_err(|error| format!("invalid foreign JSONL record {records}: {error}"))?;
        let kind = record["type"].as_str();
        let (format, message) = if matches!(kind, Some("user" | "assistant"))
            && record["message"].is_object()
        {
            if record["isSidechain"].as_bool() == Some(true)
                || record["isMeta"].as_bool() == Some(true)
            {
                continue;
            }
            ("claude", &record["message"])
        } else if kind == Some("response_item") && record["payload"]["type"] == "message" {
            ("codex", &record["payload"])
        } else if kind == Some("session_meta") {
            if provider.is_some_and(|provider| provider != "codex") {
                return Err("mixed foreign journal formats".into());
            }
            provider = Some("codex");
            continue;
        } else if kind == Some("event_msg") {
            let payload = &record["payload"];
            if let Some(user) = match payload["type"].as_str() {
                Some("user_message") => Some(true),
                Some("agent_message") => Some(false),
                _ => None,
            } && let Some(message) = payload["message"].as_str().filter(|text| !text.is_empty())
            {
                event_messages.push(Message {
                    user,
                    text: message.into(),
                });
            }
            continue;
        } else {
            continue;
        };
        if provider.is_some_and(|provider| provider != format) {
            return Err("mixed foreign journal formats".into());
        }
        provider = Some(format);
        let user = match message["role"].as_str() {
            Some("user") => true,
            Some("assistant") => false,
            _ => continue,
        };
        if message["phase"] == "analysis" {
            continue;
        }
        let text = text(&message["content"], format);
        if !text.is_empty() {
            messages.push(Message { user, text });
        }
    }
    let provider = provider.ok_or("unsupported foreign journal format")?;
    // Newer rollout journals contain both response items and duplicate UI event messages.
    if provider == "codex" && messages.is_empty() {
        messages = event_messages;
    }
    if messages.is_empty() {
        return Err("foreign journal has no supported visible conversation text".into());
    }
    Ok((provider, messages))
}

fn descriptor(source: &Path) -> Result<PathBuf, String> {
    safe_path(source)?;
    if source.is_file() {
        return Ok(source.to_owned());
    }
    for marker in ["rollout.jsonl", "transcript.jsonl"] {
        let path = source.join(marker);
        if std::fs::symlink_metadata(&path).is_ok() {
            return Ok(path);
        }
    }
    Err("unsupported foreign descriptor; expected a supported conversation JSONL journal".into())
}

pub(super) fn import(source: &Path, destination: &Path) -> Result<ForeignImportResult, String> {
    safe_path(source)?;
    if source.is_dir() && std::fs::symlink_metadata(source.join("events.jsonl")).is_ok() {
        return harness_core::foreign_session::import_foreign_session_as_replay(
            source,
            destination,
        )
        .map_err(|error| error.to_string());
    }
    safe_path(destination)?;
    let resolved_destination = harness_core::tool::resolve_file_path(source, destination)
        .map_err(|error| error.to_string())?;
    if source.is_dir()
        && resolved_destination
            .starts_with(source.canonicalize().map_err(|error| error.to_string())?)
    {
        return Err("foreign import destination must not be inside its source".into());
    }
    let path = descriptor(source)?;
    let bytes = read(&path)?;
    let (provider, messages) = parse(&bytes)?;
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let journal = Journal::open(staging.path(), "foreign-visible", true)
        .map_err(|error| error.to_string())?;
    for (index, message) in messages.into_iter().enumerate() {
        let request_id = format!("foreign-message-{index}");
        let text = harness_core::redact::redact_artifact_text(&message.text);
        let payload = if message.user {
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text,
            })
        } else {
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: request_id.into(),
                tool_call_count: 0,
                parts: vec![AssistantPart::Text { text }],
                provenance: None,
                assistant_message: None,
            })
        };
        journal
            .append(EventEnvelopeWithoutSeqV1 {
                schema_version: SCHEMA_VERSION,
                event_id: format!("foreign-visible-{index}"),
                run_id: "foreign-visible".into(),
                mono_ms: index as u64,
                ts: None,
                actor: EventActor::new(
                    if message.user {
                        ActorKind::User
                    } else {
                        ActorKind::Worker
                    },
                    None,
                ),
                correlation_id: None,
                causation_id: None,
                stream_key: None,
                payload,
            })
            .map_err(|error| error.to_string())?;
    }
    journal.close_writer().map_err(|error| error.to_string())?;
    if read(&path)? != bytes {
        return Err("foreign source changed during import".into());
    }
    let mut imported = harness_core::foreign_session::import_foreign_session_as_replay(
        &staging.path().join("foreign-visible"),
        destination,
    )
    .map_err(|error| error.to_string())?;
    imported.source_path = source.to_owned();
    imported.format = format!("{provider}_visible_jsonl_v1");
    Ok(imported)
}

pub(super) fn discover(root: &Path) -> Result<Value, String> {
    safe_path(root)?;
    let mut pending = vec![root.to_owned()];
    let mut candidates = Vec::new();
    let mut visited = 0;
    let mut scanned_bytes = 0;
    while let Some(directory) = pending.pop() {
        visited += 1;
        if visited > 4096 {
            return Err("foreign scan exceeds 4096 directories; choose a narrower root".into());
        }
        for candidate in harness_core::foreign_session::discover_foreign_sessions(&directory)
            .map_err(|error| error.to_string())?
        {
            let path = candidate.path().to_owned();
            let mut value = serde_json::to_value(&candidate).map_err(|error| error.to_string())?;
            match &candidate {
                ForeignSessionCandidate::Discoverable { marker, .. }
                    if marker == "events.jsonl" =>
                {
                    value["importable"] = true.into();
                }
                ForeignSessionCandidate::Discoverable { marker, .. }
                    if marker.ends_with("jsonl") =>
                {
                    scanned_bytes += std::fs::metadata(path.join(marker))
                        .map_err(|error| error.to_string())?
                        .len();
                    if scanned_bytes > MAX_BYTES {
                        return Err("foreign scan exceeds 64 MiB; choose a narrower root".into());
                    }
                    match read(&path.join(marker))
                        .and_then(|bytes| parse(&bytes).map(|(provider, _)| provider))
                    {
                        Ok(provider) => {
                            value["kind"] = provider.into();
                            value["importable"] = true.into();
                        }
                        Err(error) => {
                            value["importable"] = false.into();
                            value["reason"] = error.into();
                        }
                    }
                }
                ForeignSessionCandidate::Rejected { .. }
                    if path.is_dir() && safe_path(&path).is_ok() =>
                {
                    pending.push(path);
                    continue;
                }
                ForeignSessionCandidate::Rejected { .. }
                    if path
                        .extension()
                        .is_some_and(|extension| extension == "jsonl")
                        && safe_path(&path).is_ok() =>
                {
                    scanned_bytes += std::fs::metadata(&path)
                        .map_err(|error| error.to_string())?
                        .len();
                    if scanned_bytes > MAX_BYTES {
                        return Err("foreign scan exceeds 64 MiB; choose a narrower root".into());
                    }
                    match read(&path).and_then(|bytes| parse(&bytes).map(|(provider, _)| provider))
                    {
                        Ok(provider) => {
                            value = json!({"status":"discoverable","kind":provider,"path":path,"marker":path.file_name().and_then(|name| name.to_str()),"importable":true});
                        }
                        Err(error) => {
                            value = json!({"status":"rejected","path":path,"reason":error,"importable":false});
                        }
                    }
                }
                ForeignSessionCandidate::Rejected { .. } => continue,
                _ => {
                    value["importable"] = false.into();
                }
            }
            candidates.push(value);
            if candidates.len() > 512 {
                return Err("foreign scan exceeds 512 candidates; choose a narrower root".into());
            }
        }
    }
    candidates.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let count = |status: &str| {
        candidates
            .iter()
            .filter(|candidate| candidate["status"] == status)
            .count()
    };
    let importable = candidates
        .iter()
        .filter(|candidate| candidate["importable"] == true)
        .count();
    Ok(json!({"scan_root":root,"count":candidates.len(),"summary":{
        "total":candidates.len(),"discoverable":count("discoverable"),"importable":importable,
        "discoverable_not_importable":count("discoverable") - importable,"corrupt":count("corrupt"),"rejected":count("rejected")
    },"candidates":candidates}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::proj::SessionModeSource;
    use harness_core::transcript_projection::{
        project_transcript, ProjectedMessageRole, ProjectedPart,
    };

    #[test]
    fn provider_jsonl_discovery_imports_visible_roles_without_reasoning_or_source_mutation(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        let histories = home.path().join("provider-histories");
        let destination = home.path().join("harness/sessions");
        let fixtures = [
            (
                "claude/projects/work/transcript.jsonl",
                vec![
                    json!({"type":"user","message":{"role":"user","content":"hello user"}}),
                    json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"private reasoning"},{"type":"text","text":"visible answer"},{"type":"tool_use","name":"shell","input":{"command":"do not execute"}}]}}),
                    json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"visible followup"}]}}),
                ],
            ),
            (
                "codex/sessions/2026/rollout-2026-10-16.jsonl",
                vec![
                    json!({"type":"session_meta","payload":{"id":"synthetic"}}),
                    json!({"type":"event_msg","payload":{"type":"user_message","message":"hello user"}}),
                    json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello user"}]}}),
                    json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"private reasoning"}]}}),
                    json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"visible answer"}]}}),
                    json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"visible followup"}]}}),
                    json!({"type":"event_msg","payload":{"type":"agent_message","message":"visible answer"}}),
                ],
            ),
        ];
        let mut originals = Vec::new();
        for (relative, records) in fixtures {
            let path = histories.join(relative);
            std::fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
            let bytes = records
                .into_iter()
                .map(|record| serde_json::to_string(&record))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            std::fs::write(&path, &bytes)?;
            originals.push((path, bytes));
        }
        let found = discover(&histories)?;
        assert_eq!(found["summary"]["importable"], 2);
        for candidate in found["candidates"].as_array().ok_or("missing candidates")? {
            let source = Path::new(candidate["path"].as_str().ok_or("missing path")?);
            let imported = import(source, &destination)?;
            assert_eq!(imported.source_path, source);
            assert_eq!(imported.mode_source, SessionModeSource::ReplayOnly);
            assert!(!harness_core::proj::inspect_resume_plan(&imported.run_dir).is_resumable);
            let events = harness_core::store::read_events(&imported.run_dir.join("events.jsonl"))?;
            let transcript = project_transcript(&events)?;
            let visible: Vec<_> = transcript
                .messages
                .iter()
                .map(|message| {
                    let text: String = message
                        .parts
                        .iter()
                        .filter_map(|part| match part {
                            ProjectedPart::Text(text) => Some(text.text.as_str()),
                            _ => None,
                        })
                        .collect();
                    (message.role, text)
                })
                .collect();
            assert_eq!(
                visible,
                vec![
                    (ProjectedMessageRole::User, "hello user".into()),
                    (ProjectedMessageRole::Assistant, "visible answer".into()),
                    (ProjectedMessageRole::Assistant, "visible followup".into())
                ]
            );
            let journal = serde_json::to_string(&events)?;
            assert!(!journal.contains("private reasoning"));
            assert!(!journal.contains("do not execute"));
        }
        for (path, bytes) in originals {
            assert_eq!(std::fs::read_to_string(path)?, bytes);
        }
        let unsupported = histories.join("unsupported.jsonl");
        std::fs::write(&unsupported, "{\"arbitrary\":\"descriptor\"}")?;
        let absent = home.path().join("must-not-create");
        assert!(import(&unsupported, &absent).is_err());
        assert!(!absent.exists());
        std::fs::write(&unsupported, "{\"type\":\"session_meta\"}\n{broken")?;
        assert!(import(&unsupported, &absent).is_err());
        assert!(!absent.exists());
        #[cfg(unix)]
        {
            let alias = home.path().join("source-alias");
            std::os::unix::fs::symlink(&histories, &alias)?;
            assert!(discover(&alias).is_err());
            assert!(import(&alias.join("claude/projects/work"), &absent).is_err());
        }
        Ok(())
    }
}
