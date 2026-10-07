//! Restart bindings (senpi `session-binding*.ts`, `session-commit-boundary.ts`).
//!
//! Also `session-registry-wiring.ts`. senpi anchors a strict private sidecar to a ledger marker
//! and to the committed assistant; the harness request carries the whole history, so the
//! sidecar is validated against it: the sent-prefix digest, the semantic hash of the assistant
//! right after that prefix, and an append-only tail (only user and tool messages follow it).
use super::continuity::BindingSnapshot;
use super::sync::{digest, sent_hash_prefix_digest};
use crate::anthropic_subscription::prompt::{LaneContext, LaneMessage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredBinding {
    pub schema_version: u32,
    pub session_id: String,
    pub sdk_session_id: String,
    pub sent_count: usize,
    pub sent_prefix_hash: String,
    pub assistant_content_hash: String,
    pub last_assistant_uuid: Option<String>,
    pub account_name: String,
    pub model_id: String,
    pub system_prompt_hash: String,
    pub toolset_hash: String,
}

fn sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bounded(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}

impl StoredBinding {
    fn valid(&self) -> bool {
        self.schema_version == 1
            && bounded(&self.session_id)
            && bounded(&self.sdk_session_id)
            && sha256_hex(&self.sent_prefix_hash)
            && sha256_hex(&self.assistant_content_hash)
            && self.last_assistant_uuid.as_deref().is_none_or(bounded)
            && bounded(&self.account_name)
            && bounded(&self.model_id)
            && sha256_hex(&self.system_prompt_hash)
            && sha256_hex(&self.toolset_hash)
    }
}

/// Only the payload the model produced is fingerprinted.
pub fn assistant_content_hash(text: &str, tool_calls: &[(String, String, Value)]) -> String {
    digest(&json!({
        "role": "assistant",
        "text": text,
        "toolCalls": tool_calls.iter().map(|(id, name, arguments)| json!({"id": id, "name": name, "arguments": arguments})).collect::<Vec<_>>(),
    }))
}

pub fn message_content_hash(message: &LaneMessage) -> Option<String> {
    let LaneMessage::Assistant {
        text, tool_calls, ..
    } = message
    else {
        return None;
    };
    let calls: Vec<_> = tool_calls
        .iter()
        .map(|c| (c.id.clone(), c.name.clone(), c.arguments.clone()))
        .collect();
    Some(assistant_content_hash(text, &calls))
}

/// The assistant committed right after the first `sent_count` sent messages, and whether
/// only user/tool messages follow it.
pub fn committed_assistant(
    context: &LaneContext,
    sent_count: usize,
) -> Option<(&LaneMessage, bool)> {
    let mut sent = 0;
    let mut messages = context.messages.iter().filter(|m| match m {
        LaneMessage::User { .. } | LaneMessage::ToolResult { .. } => super::sync::is_transmitted(m),
        LaneMessage::Assistant { .. } => true,
    });
    let committed = loop {
        let message = messages.next()?;
        match message {
            LaneMessage::Assistant { .. } if sent == sent_count => break message,
            LaneMessage::Assistant { .. } => {}
            _ => {
                sent += 1;
                if sent > sent_count {
                    return None;
                }
            }
        }
    };
    let append_only = messages.all(|m| !matches!(m, LaneMessage::Assistant { .. }));
    Some((committed, append_only))
}

pub struct BindingStore {
    directory: PathBuf,
}

fn file_name(session_id: &str) -> Option<String> {
    let safe = !session_id.is_empty()
        && session_id.len() <= 256
        && session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && !session_id.starts_with('.');
    safe.then(|| format!("{session_id}.claude-sdk-oauth-binding.json"))
}

impl BindingStore {
    pub fn new(agent_dir: &Path) -> Self {
        Self {
            directory: agent_dir.join("anthropic-subscription-bindings"),
        }
    }
    fn path(&self, session_id: &str) -> Option<PathBuf> {
        file_name(session_id).map(|name| self.directory.join(name))
    }
    pub fn read(&self, session_id: &str) -> Option<StoredBinding> {
        let path = self.path(session_id)?;
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
            return None;
        }
        let stored: StoredBinding = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        (stored.valid() && stored.session_id == session_id).then_some(stored)
    }
    pub fn write(&self, record: &StoredBinding) -> Result<(), String> {
        if !record.valid() {
            return Err("invalid stored binding".into());
        }
        let path = self.path(&record.session_id).ok_or("invalid session id")?;
        let serialized = format!(
            "{}\n",
            serde_json::to_string(record).map_err(|e| e.to_string())?
        );
        if serialized.len() as u64 > MAX_RECORD_BYTES {
            return Err(format!(
                "Stored binding exceeds {MAX_RECORD_BYTES} bytes: {}",
                serialized.len()
            ));
        }
        std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let mut random = [0u8; 8];
        let _ = getrandom::fill(&mut random);
        let temporary = self.directory.join(format!(
            ".{}.tmp",
            crate::anthropic_subscription::prompt::hex(&random)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let result = options
            .open(&temporary)
            .and_then(|mut file| file.write_all(serialized.as_bytes()))
            .and_then(|()| std::fs::rename(&temporary, &path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|e| e.to_string())
    }
    pub fn delete(&self, session_id: &str) {
        if let Some(path) = self.path(session_id) {
            let _ = std::fs::remove_file(path);
        }
    }
    /// The newest cold-seed overflow that carried both an estimate and a provider count,
    /// so a restarted process re-learns the session's calibration.
    pub fn write_calibration(&self, session_id: &str, estimated: u64, reported: u64) {
        let Some(name) = file_name(session_id) else {
            return;
        };
        let path = self.directory.join(name.replace("binding", "cold-seed"));
        let body = json!({"estimatedTokens": estimated, "reportedTokens": reported}).to_string();
        if std::fs::create_dir_all(&self.directory).is_ok() {
            let _ = std::fs::write(path, body);
        }
    }
    pub fn read_calibration(&self, session_id: &str) -> Option<(u64, u64)> {
        let name = file_name(session_id)?.replace("binding", "cold-seed");
        let value: Value =
            serde_json::from_slice(&std::fs::read(self.directory.join(name)).ok()?).ok()?;
        Some((
            value["estimatedTokens"].as_u64()?,
            value["reportedTokens"].as_u64()?,
        ))
    }
}

pub fn binding_from_stored(stored: &StoredBinding) -> BindingSnapshot {
    BindingSnapshot {
        sdk_session_id: stored.sdk_session_id.clone(),
        sent_count: stored.sent_count,
        sent_hashes: Vec::new(),
        sent_prefix_hash: Some(stored.sent_prefix_hash.clone()),
        last_assistant_uuid: stored.last_assistant_uuid.clone(),
        assistant_uuid_by_index: stored
            .last_assistant_uuid
            .iter()
            .map(|uuid| (stored.sent_count, uuid.clone()))
            .collect(),
        account_name: stored.account_name.clone(),
        model_id: stored.model_id.clone(),
        system_prompt_hash: stored.system_prompt_hash.clone(),
        toolset_hash: stored.toolset_hash.clone(),
        unanswered_turn_digest: None,
        sdk_session_id_confirmed: None,
    }
}

/// `bindingFromStoredBranch`: the stored binding when the request still carries the
/// committed assistant unchanged after the sent prefix, followed only by new user input.
pub fn restore_binding(
    stored: &StoredBinding,
    context: &LaneContext,
    hashes: &[String],
) -> Option<BindingSnapshot> {
    if hashes.len() < stored.sent_count
        || sent_hash_prefix_digest(hashes, stored.sent_count) != stored.sent_prefix_hash
    {
        return None;
    }
    let (committed, append_only) = committed_assistant(context, stored.sent_count)?;
    (append_only && message_content_hash(committed)? == stored.assistant_content_hash)
        .then(|| binding_from_stored(stored))
}

/// `storedBindingFromEntry` / `storedBindingFromBinding` for the turn just committed.
pub fn stored_binding(
    session_id: &str,
    binding: &BindingSnapshot,
    hashes: &[String],
    assistant_content_hash: String,
) -> Option<StoredBinding> {
    if binding.sdk_session_id_confirmed == Some(false) || binding.sent_count != hashes.len() {
        return None;
    }
    Some(StoredBinding {
        schema_version: 1,
        session_id: session_id.into(),
        sdk_session_id: binding.sdk_session_id.clone(),
        sent_count: hashes.len(),
        sent_prefix_hash: sent_hash_prefix_digest(hashes, hashes.len()),
        assistant_content_hash,
        last_assistant_uuid: binding.last_assistant_uuid.clone(),
        account_name: binding.account_name.clone(),
        model_id: binding.model_id.clone(),
        system_prompt_hash: binding.system_prompt_hash.clone(),
        toolset_hash: binding.toolset_hash.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic_subscription::prompt::{Content, LaneToolCall};
    use crate::anthropic_subscription::session::sync::{sent_message_hashes, sent_messages};

    #[test]
    fn restored_bindings_require_the_committed_assistant_and_an_append_only_tail(
    ) -> Result<(), String> {
        let user = |t: &str| LaneMessage::User {
            content: Content::Text(t.into()),
        };
        let assistant = |t: &str| LaneMessage::Assistant {
            text: t.into(),
            tool_calls: vec![LaneToolCall {
                id: "c".into(),
                name: "read".into(),
                arguments: json!({"filePath": "x"}),
            }],
            timestamp: 0,
        };
        let mut context = LaneContext {
            messages: vec![user("a"), assistant("one"), user("b")],
            ..LaneContext::default()
        };
        let hashes = sent_message_hashes(&sent_messages(&context)[..1]);
        let root = tempfile::tempdir().map_err(|e| e.to_string())?;
        let store = BindingStore::new(root.path());
        let h = "0".repeat(64);
        let stored = StoredBinding {
            schema_version: 1,
            session_id: "s1".into(),
            sdk_session_id: "sdk".into(),
            sent_count: 1,
            sent_prefix_hash: sent_hash_prefix_digest(&hashes, 1),
            assistant_content_hash: message_content_hash(&context.messages[1]).ok_or("hash")?,
            last_assistant_uuid: Some("u1".into()),
            account_name: "default".into(),
            model_id: "m".into(),
            system_prompt_hash: h.clone(),
            toolset_hash: h,
        };
        store.write(&stored)?;
        let read = store.read("s1").ok_or("missing")?;
        let all = sent_message_hashes(&sent_messages(&context));
        assert!(restore_binding(&read, &context, &all).is_some());
        context.messages[1] = assistant("rewritten");
        assert!(restore_binding(&read, &context, &all).is_none());
        context.messages[1] = assistant("one");
        context.messages.push(assistant("later"));
        assert!(restore_binding(&read, &context, &all).is_none());
        assert!(store.read("../escape").is_none());
        Ok(())
    }
}
