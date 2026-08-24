//! Multi-format foreign session file import (jsonl envelopes, raw text, markdown).
//!
//! Extends the single-format directory import (see `import.rs`) with explicit
//! file-format converters:
//!
//! - `jsonl` — a file of harness-compatible event envelopes (same schema as the
//!   directory `events.jsonl` marker import).
//! - `txt` — raw text; every non-empty line becomes one imported user message.
//! - `md` — markdown transcript; ATX headings whose text is exactly `user` or
//!   `assistant` (case-insensitive, optional trailing colon) delimit message
//!   roles. Body lines until the next role heading form the message text.
//!   Content before the first role heading is imported as a user message.
//!
//! Like the directory import, file import materializes a **new** replay-only
//! harness session under the destination store via append-only event writes and
//! never mutates the source file. Unknown formats fail closed.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::digest::digest12;
use crate::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderAssistantMessageMetadata, RunFinishedEvent, RunStartedEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use crate::ids::RunId;
use crate::session::{AssistantPart, ProviderProvenance};
use crate::proj::SessionModeSource;
use crate::session_paths::META_FILE_NAME;

use super::import::{next_import_run_id, parse_events_jsonl, rewrite_import_events, write_events_jsonl};
use super::{ForeignImportResult, ForeignSessionError};

/// Explicit foreign file import format (CLI `--format` names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignImportFileFormat {
    /// Harness-compatible event envelopes, one JSON object per line.
    Jsonl,
    /// Raw text transcript; every non-empty line is one user message.
    Txt,
    /// Markdown transcript with `user` / `assistant` role headings.
    Md,
}

impl ForeignImportFileFormat {
    /// CLI-facing format name (`jsonl` / `txt` / `md`).
    pub const fn as_cli_name(self) -> &'static str {
        match self {
            Self::Jsonl => "jsonl",
            Self::Txt => "txt",
            Self::Md => "md",
        }
    }

    /// Stable format id recorded in the imported session's `meta.json`.
    pub const fn stable_format_id(self) -> &'static str {
        match self {
            Self::Jsonl => "events_jsonl_v1",
            Self::Txt => "raw_text_v1",
            Self::Md => "markdown_v1",
        }
    }

    /// Case-insensitive CLI name parse. Unknown names fail closed (`None`).
    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "jsonl" => Some(Self::Jsonl),
            "txt" | "text" => Some(Self::Txt),
            "md" | "markdown" => Some(Self::Md),
            _ => None,
        }
    }

    /// Best-effort format inference from a file extension. Unknown or missing
    /// extensions fail closed (`None`); operators must pass `--format` then.
    pub fn from_extension(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "jsonl" => Some(Self::Jsonl),
            "txt" => Some(Self::Txt),
            "md" | "markdown" => Some(Self::Md),
            _ => None,
        }
    }

    /// Operator-facing list of supported CLI format names.
    pub const SUPPORTED_CLI_NAMES: &'static str = "jsonl, txt, md";
}

/// Role of one parsed transcript message (text formats only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptMessage {
    role: TranscriptRole,
    text: String,
}

/// Import one foreign session **file** as a new harness replay-only session.
///
/// The source file is read-only; writes are append-only into a freshly created
/// run directory under `dest_session_dir`. Fail-closed for missing files,
/// empty content, and unparseable envelope lines.
pub fn import_foreign_file_as_replay(
    source_path: &Path,
    format: ForeignImportFileFormat,
    dest_session_dir: &Path,
) -> Result<ForeignImportResult, ForeignSessionError> {
    if !source_path.is_file() {
        return Err(ForeignSessionError::SourceNotFile {
            path: source_path.display().to_string(),
            reason: format!(
                "format `{}` expects a source file; pass a directory without --format for marker-based import",
                format.as_cli_name()
            ),
        });
    }
    if dest_session_dir.exists() && !dest_session_dir.is_dir() {
        return Err(ForeignSessionError::DestinationNotDirectory {
            path: dest_session_dir.display().to_string(),
        });
    }

    let run_id = next_import_run_id();
    let message_count: Option<usize>;
    let events = match format {
        ForeignImportFileFormat::Jsonl => {
            let source_events = parse_events_jsonl(source_path)?;
            if source_events.is_empty() {
                return Err(ForeignSessionError::EmptySource {
                    path: source_path.display().to_string(),
                });
            }
            let source_run_id = source_events.first().map(|event| event.run_id.to_string());
            message_count = None;
            rewrite_import_events(&source_events, source_run_id.as_deref(), &run_id)
        }
        ForeignImportFileFormat::Txt => {
            let text = read_source_text(source_path)?;
            let messages = parse_text_messages(&text);
            if messages.is_empty() {
                return Err(ForeignSessionError::EmptySource {
                    path: source_path.display().to_string(),
                });
            }
            message_count = Some(messages.len());
            synthesize_transcript_events(&run_id, &messages)
        }
        ForeignImportFileFormat::Md => {
            let text = read_source_text(source_path)?;
            let messages = parse_markdown_messages(&text);
            if messages.is_empty() {
                return Err(ForeignSessionError::EmptySource {
                    path: source_path.display().to_string(),
                });
            }
            message_count = Some(messages.len());
            synthesize_transcript_events(&run_id, &messages)
        }
    };

    materialize_import_run(dest_session_dir, &run_id, &events, source_path, format, message_count)
}

fn read_source_text(source_path: &Path) -> Result<String, ForeignSessionError> {
    fs::read_to_string(source_path).map_err(|err| ForeignSessionError::SourceRead {
        path: source_path.display().to_string(),
        message: err.to_string(),
    })
}

/// Raw text import contract: every non-empty trimmed line is one user message.
fn parse_text_messages(text: &str) -> Vec<TranscriptMessage> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| TranscriptMessage {
            role: TranscriptRole::User,
            text: line.to_string(),
        })
        .collect()
}

/// Markdown transcript contract: ATX headings whose text is exactly `user` or
/// `assistant` (case-insensitive, optional trailing colon) delimit roles.
/// Lines between role headings form the message body verbatim. Content before
/// the first role heading is imported as a user message.
fn parse_markdown_messages(text: &str) -> Vec<TranscriptMessage> {
    let mut messages: Vec<TranscriptMessage> = Vec::new();
    let mut current_role: Option<TranscriptRole> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if let Some(role) = heading_role(line) {
            flush_transcript_message(&mut messages, current_role, &current_lines);
            current_role = Some(role);
            current_lines.clear();
            continue;
        }
        if current_role.is_none() && line.trim().is_empty() {
            // Drop leading blank lines before any content/role.
            continue;
        }
        current_lines.push(line.to_string());
    }
    flush_transcript_message(&mut messages, current_role, &current_lines);
    messages
}

fn flush_transcript_message(
    messages: &mut Vec<TranscriptMessage>,
    role: Option<TranscriptRole>,
    lines: &[String],
) {
    let body = lines.join("\n").trim().to_string();
    if body.is_empty() {
        return;
    }
    // Content before the first explicit role heading imports as a user message.
    let role = role.unwrap_or(TranscriptRole::User);
    messages.push(TranscriptMessage { role, text: body });
}

fn heading_role(line: &str) -> Option<TranscriptRole> {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let label = rest.trim().trim_end_matches(':').trim().to_ascii_lowercase();
    match label.as_str() {
        "user" => Some(TranscriptRole::User),
        "assistant" => Some(TranscriptRole::Assistant),
        _ => None,
    }
}

/// Build a deterministic replay event stream for parsed transcript messages.
///
/// User messages map to `UserMessageSubmitted`; assistant messages map directly
/// to a semantic `AssistantMessageFinished` commit. Imported text is durable
/// semantic history, never provider transport telemetry.
fn synthesize_transcript_events(run_id: &str, messages: &[TranscriptMessage]) -> Vec<EventEnvelopeV1> {
    let mut events: Vec<EventEnvelopeV1> = Vec::with_capacity(messages.len() + 2);
    let mut seq: u64 = 0;
    let mut turn_counter: u64 = 0;
    let mut current_request_id: Option<String> = None;

    let push = |events: &mut Vec<EventEnvelopeV1>,
                    seq: &mut u64,
                    actor: EventActor,
                    correlation_id: Option<String>,
                    payload: EventV1| {
        *seq = seq.saturating_add(1);
        events.push(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-import-{run_id}-{seq:020}", seq = *seq),
            seq: *seq,
            run_id: RunId::from(run_id),
            mono_ms: *seq,
            ts: None,
            actor,
            correlation_id,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload,
        });
    };

    push(
        &mut events,
        &mut seq,
        EventActor::new(ActorKind::System, None),
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "replay".into(),
            workspace_root: String::new(),
        }),
    );

    for message in messages {
        match message.role {
            TranscriptRole::User => {
                turn_counter = turn_counter.saturating_add(1);
                let request_id = format!("req-import-turn-{turn_counter:016}");
                current_request_id = Some(request_id.clone());
                push(
                    &mut events,
                    &mut seq,
                    EventActor::new(ActorKind::User, None),
                    Some(request_id.clone()),
                    EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                        request_id: request_id.into(),
                        text: message.text.clone(),
                    }),
                );
            }
            TranscriptRole::Assistant => {
                if current_request_id.is_none() {
                    turn_counter = turn_counter.saturating_add(1);
                    current_request_id = Some(format!("req-import-turn-{turn_counter:016}"));
                }
                let request_id = current_request_id.clone().unwrap_or_default();
                let digest = digest12(message.text.as_bytes());
                push(
                    &mut events,
                    &mut seq,
                    EventActor::new(ActorKind::System, None),
                    Some(request_id.clone()),
                    EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                        request_id: request_id.clone().into(),
                        tool_call_count: 0,
                        parts: vec![AssistantPart::Text {
                            text: message.text.clone(),
                        }],
                        provenance: Some(ProviderProvenance {
                            provider_id: "foreign-import".to_string(),
                            model_id: "imported-transcript".to_string(),
                            request_id: request_id.into(),
                            response_id: None,
                            stop_reason: Some("stop".to_string()),
                            usage: None,
                        }),
                        assistant_message: Some(ProviderAssistantMessageMetadata {
                            message_id: None,
                            text_digest: Some(digest.to_string()),
                            reasoning_digest: None,
                        }),
                    }),
                );
                current_request_id = None;
            }
        }
    }

    push(
        &mut events,
        &mut seq,
        EventActor::new(ActorKind::System, None),
        None,
        EventV1::RunFinished(RunFinishedEvent {
            summary: format!(
                "imported foreign transcript ({} messages)",
                messages.len()
            ),
        }),
    );

    events
}

fn materialize_import_run(
    dest_session_dir: &Path,
    run_id: &str,
    events: &[EventEnvelopeV1],
    source_path: &Path,
    format: ForeignImportFileFormat,
    message_count: Option<usize>,
) -> Result<ForeignImportResult, ForeignSessionError> {
    fs::create_dir_all(dest_session_dir).map_err(|err| ForeignSessionError::DestinationWrite {
        path: dest_session_dir.display().to_string(),
        message: err.to_string(),
    })?;

    let run_dir = dest_session_dir.join(run_id);
    if run_dir.exists() {
        return Err(ForeignSessionError::DestinationWrite {
            path: run_dir.display().to_string(),
            message: "run directory already exists".to_string(),
        });
    }
    fs::create_dir_all(&run_dir).map_err(|err| ForeignSessionError::DestinationWrite {
        path: run_dir.display().to_string(),
        message: err.to_string(),
    })?;

    write_events_jsonl(&run_dir, events)?;
    let foreign_import_meta = serde_json::json!({
        "format": format.stable_format_id(),
        "source_kind": "file",
        "source_path": source_path.display().to_string(),
        "event_count": events.len(),
        "message_count": message_count,
        "policy": "read-only replay import; append-only new events.jsonl; source path never mutated"
    });
    write_file_import_meta(&run_dir, run_id, format, &foreign_import_meta)?;

    Ok(ForeignImportResult {
        run_id: run_id.to_string(),
        run_dir,
        event_count: events.len(),
        source_path: source_path.to_path_buf(),
        format: format.stable_format_id().to_string(),
        mode_source: SessionModeSource::ReplayOnly,
    })
}

fn write_file_import_meta(
    run_dir: &Path,
    run_id: &str,
    format: ForeignImportFileFormat,
    foreign_import_meta: &serde_json::Value,
) -> Result<(), ForeignSessionError> {
    let path = run_dir.join(META_FILE_NAME);
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let meta = serde_json::json!({
        "run_id": run_id,
        "run_name": "replay",
        "workspace_root": "",
        "created_at": created_at,
        "config_digest": format!("foreign-import-{}", format.stable_format_id()),
        "harness_version": env!("CARGO_PKG_VERSION"),
        "mode_source": "replay_only",
        "foreign_import": foreign_import_meta,
    });
    let body = serde_json::to_string_pretty(&meta).map_err(|err| {
        ForeignSessionError::DestinationWrite {
            path: path.display().to_string(),
            message: err.to_string(),
        }
    })?;
    fs::write(&path, format!("{body}\n")).map_err(|err| ForeignSessionError::DestinationWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_name_parse_is_case_insensitive_and_fail_closed() {
        // arrange
        // act
        // assert
        assert_eq!(
            ForeignImportFileFormat::from_cli_name("JSONL"),
            Some(ForeignImportFileFormat::Jsonl)
        );
        assert_eq!(
            ForeignImportFileFormat::from_cli_name(" txt "),
            Some(ForeignImportFileFormat::Txt)
        );
        assert_eq!(
            ForeignImportFileFormat::from_cli_name("markdown"),
            Some(ForeignImportFileFormat::Md)
        );
        assert_eq!(ForeignImportFileFormat::from_cli_name("xml"), None);
    }

    #[test]
    fn extension_inference_maps_known_extensions_and_fails_closed() {
        // arrange
        // act
        // assert
        assert_eq!(
            ForeignImportFileFormat::from_extension(Path::new("a/b.jsonl")),
            Some(ForeignImportFileFormat::Jsonl)
        );
        assert_eq!(
            ForeignImportFileFormat::from_extension(Path::new("notes.TXT")),
            Some(ForeignImportFileFormat::Txt)
        );
        assert_eq!(
            ForeignImportFileFormat::from_extension(Path::new("chat.markdown")),
            Some(ForeignImportFileFormat::Md)
        );
        assert_eq!(ForeignImportFileFormat::from_extension(Path::new("chat.log")), None);
        assert_eq!(ForeignImportFileFormat::from_extension(Path::new("noext")), None);
    }

    #[test]
    fn text_lines_become_individual_user_messages() {
        // arrange
        // act
        let messages = parse_text_messages("first line\n\n  second line  \n   \nthird\n");

        // assert
        let texts: Vec<&str> = messages.iter().map(|message| message.text.as_str()).collect();
        assert_eq!(texts, vec!["first line", "second line", "third"]);
        assert!(messages.iter().all(|m| m.role == TranscriptRole::User));
    }

    #[test]
    fn markdown_role_headings_delimit_messages() {
        // arrange
        let markdown = "# User\nWhat is Rust?\n\n## Assistant\nA systems language.\nWith two lines.\n# user\nThanks!\n";

        // act
        let messages = parse_markdown_messages(markdown);

        // assert
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, TranscriptRole::User);
        assert_eq!(messages[0].text, "What is Rust?");
        assert_eq!(messages[1].role, TranscriptRole::Assistant);
        assert_eq!(messages[1].text, "A systems language.\nWith two lines.");
        assert_eq!(messages[2].role, TranscriptRole::User);
        assert_eq!(messages[2].text, "Thanks!");
    }

    #[test]
    fn markdown_preamble_before_first_role_heading_imports_as_user() {
        // arrange
        let markdown = "Imported log notes\nsecond preamble line\n# Assistant\nreply\n";

        // act
        let messages = parse_markdown_messages(markdown);

        // assert
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, TranscriptRole::User);
        assert!(messages[0].text.contains("Imported log notes"));
        assert_eq!(messages[1].role, TranscriptRole::Assistant);
        assert_eq!(messages[1].text, "reply");
    }

    #[test]
    fn markdown_without_role_headings_imports_as_single_user_message() {
        // arrange
        let markdown = "plain notes\nmore notes\n";

        // act
        let messages = parse_markdown_messages(markdown);

        // assert
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, TranscriptRole::User);
        assert_eq!(messages[0].text, "plain notes\nmore notes");
    }

    #[test]
    fn markdown_headings_with_other_labels_are_body_text() {
        // arrange
        let markdown = "# Summary\nnot a role heading\n## User notes\nstill body\n# User\nreal message\n";

        // act
        let messages = parse_markdown_messages(markdown);

        // assert
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, TranscriptRole::User);
        assert!(messages[0].text.contains("not a role heading"));
        assert!(messages[0].text.contains("## User notes"));
        assert_eq!(messages[1].text, "real message");
    }

    #[test]
    fn synthesized_events_project_user_and_assistant_messages() {
        // arrange
        let messages = vec![
            TranscriptMessage {
                role: TranscriptRole::User,
                text: "hello".to_string(),
            },
            TranscriptMessage {
                role: TranscriptRole::Assistant,
                text: "world".to_string(),
            },
        ];

        // act
        let events = synthesize_transcript_events("run_synthetic_test", &messages);

        // assert
        assert!(matches!(events.first().unwrap().payload, EventV1::RunStarted(_)));
        assert!(matches!(events.last().unwrap().payload, EventV1::RunFinished(_)));
        let user_count = events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::UserMessageSubmitted(_)))
            .count();
        let assistant_parts = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::AssistantMessageFinished(data) => Some(data.parts.clone()),
                _ => None,
            })
            .unwrap_or_default();
        assert_eq!(user_count, 1);
        assert_eq!(
            assistant_parts,
            vec![AssistantPart::Text {
                text: "world".to_string(),
            }]
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event.payload, EventV1::ProviderStreamDelta(_))));
        let seqs: Vec<u64> = events.iter().map(|event| event.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted, "synthetic events must be contiguous-ordered");
        assert_eq!(seqs[0], 1);
        assert_eq!(seqs.last().copied(), Some(seqs.len() as u64));
    }
}
