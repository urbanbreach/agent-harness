// allow: SIZE_OK — CLI replay (event replay + output rendering)
use crate::UnwrapOrAbort;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use harness_core::event::{EventEnvelopeV1, EventV1, RunStartedEvent};
use harness_core::proj::{
    project_run_summary, project_session_catalog_entry, ChildSessionTerminalState, RunStatus,
    SessionCatalogEntry, SessionCatalogMetadata,
};
use serde::Serialize;
use serde_json::Value;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME, META_FILE_NAME};
use crate::defaults::RESUME_UNAVAILABLE_FALLBACK_REASON;

#[path = "replay/recovery_story.rs"]
mod recovery_story;

use recovery_story::{summarize_recovery_counts, summarize_recovery_story};

#[derive(Debug, Args, Clone)]
pub struct ReplayCommand {
    #[arg(long)]
    pub session: PathBuf,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayArtifactSummary {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub present_on_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayChildSessionSummary {
    pub child_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_child_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<ChildSessionTerminalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_terminal_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_terminal_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivered_turn_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_tool_call_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplaySummary {
    pub run_id: String,
    pub run_name: Option<String>,
    pub session_path: PathBuf,
    pub status: RunStatus,
    pub workspace_root: Option<String>,
    pub mode_source: harness_core::proj::SessionModeSource,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
    pub artifact_count: usize,
    pub child_session_count: usize,
    pub parent_session_id: Option<String>,
    pub total_events: u64,
    pub counts_by_type: BTreeMap<String, u64>,
    pub pending_permissions: Vec<String>,
    pub tasks_in_flight: Vec<String>,
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ReplayArtifactSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_sessions: Vec<ReplayChildSessionSummary>,
}

#[derive(Debug, Clone)]
pub struct SessionInspectionEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
    pub sort_unix_ms: u128,
    pub artifact_count: usize,
    pub child_session_count: usize,
}

impl SessionInspectionEntry {
    pub(crate) fn sort_by_updated_desc(entries: &mut [Self]) {
        entries.sort_by_key(|entry| (Reverse(entry.sort_unix_ms), entry.run_dir.clone()));
    }

    pub(crate) fn sort_by_updated_asc(entries: &mut [Self]) {
        entries.sort_by_key(|entry| (entry.sort_unix_ms, entry.run_dir.clone()));
    }

    pub(crate) fn is_visible_in_operator_history(&self) -> bool {
        !matches!(
            self.catalog.mode_source,
            harness_core::proj::SessionModeSource::ScenarioFixture
                | harness_core::proj::SessionModeSource::ReplayOnly
        )
    }

    pub(crate) fn normalize_lineage(mut self) -> Self {
        if let Some(parent_run_id) = harness_lineage_parent_run_id(&self.run_dir) {
            self.catalog.parent_session_id = Some(parent_run_id);
        }
        self
    }
}

pub fn execute_with_io(
    command: ReplayCommand,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> i32 {
    match summarize_session(&command.session) {
        Ok(summary) => {
            if command.json {
                match serde_json::to_string_pretty(&summary) {
                    Ok(body) => {
                        let _ = writeln!(stdout, "{body}");
                    }
                    Err(err) => {
                        let _ = writeln!(stderr, "failed to serialize replay summary: {err}");
                        return 1;
                    }
                }
            } else {
                print_human_summary(&summary, stdout);
            }
            0
        }
        Err(err) => {
            let _ = writeln!(stderr, "replay failed: {err}");
            1
        }
    }
}

pub fn summarize_session(run_dir: &Path) -> Result<ReplaySummary, String> {
    let events = load_events_from_run_dir(run_dir)?;
    if events.is_empty() {
        return Err(format!("no events found in {}", run_dir.display()));
    }

    let projection = project_run_summary(events.iter()).map_err(|err| err.to_string())?;
    let run_id = events[0].run_id.clone();
    let (meta, _meta_error) = load_meta_lossy(run_dir);
    let catalog = project_session_catalog_entry(
        events.iter(),
        run_id.as_str(),
        meta.as_ref().map(|it| &it.catalog),
        None,
        None,
    )
    .map_err(|err| err.to_string())?;
    let run_name = run_started_event(&events).map(|data| data.run_name.to_string());
    let (artifacts, child_sessions) = summarize_recovery_story(run_dir, &events, run_id.as_str());

    Ok(ReplaySummary {
        run_id: run_id.to_string(),
        run_name,
        session_path: run_dir.to_path_buf(),
        status: projection.status,
        workspace_root: catalog.workspace_root,
        mode_source: catalog.mode_source,
        is_resumable: catalog.is_resumable,
        resume_disabled_reason: catalog.resume_disabled_reason,
        artifact_count: artifacts.len(),
        child_session_count: child_sessions.len(),
        parent_session_id: catalog.parent_session_id,
        total_events: projection.counts.total_events,
        counts_by_type: projection.counts.by_type,
        pending_permissions: projection.pending_permissions.into_iter().collect(),
        tasks_in_flight: projection.tasks_in_flight.into_iter().collect(),
        last_error: projection.last_error,
        artifacts,
        child_sessions,
    })
}

pub fn inspect_session_catalog(session_dir: &Path) -> Result<Vec<SessionInspectionEntry>, String> {
    let read_dir = fs::read_dir(session_dir).map_err(|err| {
        format!(
            "failed to read session directory {}: {err}",
            session_dir.display()
        )
    })?;

    let mut entries = read_dir
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_dir() && path.join(EVENTS_FILE_NAME).exists())
        .map(|run_dir| inspect_single_session(&run_dir))
        .map(SessionInspectionEntry::normalize_lineage)
        .collect::<Vec<_>>();

    SessionInspectionEntry::sort_by_updated_desc(&mut entries);

    Ok(entries)
}

fn harness_lineage_parent_run_id(run_dir: &Path) -> Option<String> {
    let body = fs::read_to_string(run_dir.join(META_FILE_NAME)).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&body).ok()?;
    let lineage = value.get("harness_lineage")?;
    lineage
        .get("harness_source_run_id")
        .or_else(|| lineage.get("parent_run_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn inspect_single_session(run_dir: &Path) -> SessionInspectionEntry {
    let run_id_fallback = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<unknown-run>");

    let (events, events_error) = load_events_lossy(run_dir);
    let (meta, _meta_error) = load_meta_lossy(run_dir);

    let modified = run_dir
        .join(EVENTS_FILE_NAME)
        .metadata()
        .ok()
        .and_then(|it| it.modified().ok());
    let sort_unix_ms = modified
        .and_then(system_time_unix_ms)
        .or_else(|| {
            meta.as_ref().and_then(|it| {
                it.created_at
                    .as_deref()
                    .and_then(parse_unix_ms_from_string_timestamp)
            })
        })
        .unwrap_or(0);
    let last_updated_at = modified
        .and_then(system_time_unix_ms)
        .map(|it| it.to_string())
        .or_else(|| meta.as_ref().and_then(|it| it.created_at.clone()));

    let degraded_reason = events_error.map(|err| format!("events unavailable: {err}"));

    let catalog = match project_session_catalog_entry(
        events.iter(),
        run_id_fallback,
        meta.as_ref().map(|it| &it.catalog),
        last_updated_at.clone(),
        degraded_reason,
    ) {
        Ok(entry) => entry,
        Err(err) => SessionCatalogEntry {
            run_id: events
                .first()
                .map(|event| event.run_id.to_string())
                .unwrap_or_else(|| run_id_fallback.to_string()),
            run_name: run_started_event(&events).map(|data| data.run_name.to_string()),
            status: None,
            last_updated_at,
            workspace_root: run_started_event(&events).map(|data| data.workspace_root.clone()),
            profile_preset: None,
            provider_model: None,
            mode_source: harness_core::proj::SessionModeSource::Unknown,
            is_resumable: false,
            resume_disabled_reason: Some(format!("projection unavailable: {err}")),
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    };

    let (artifact_count, child_session_count) =
        summarize_recovery_counts(run_dir, &events, run_id_fallback);

    SessionInspectionEntry {
        run_dir: run_dir.to_path_buf(),
        catalog,
        sort_unix_ms,
        artifact_count,
        child_session_count,
    }
}

fn run_started_event(events: &[EventEnvelopeV1]) -> Option<&RunStartedEvent> {
    events.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(data) => Some(data),
        _ => None,
    })
}

fn load_events_lossy(run_dir: &Path) -> (Vec<EventEnvelopeV1>, Option<String>) {
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let text = match fs::read_to_string(&events_path) {
        Ok(text) => text,
        Err(err) => {
            return (
                Vec::new(),
                Some(format!(
                    "failed to read events file {}: {err}",
                    events_path.display()
                )),
            )
        }
    };

    let mut events = Vec::new();
    let mut first_error: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        match serde_json::from_str::<EventEnvelopeV1>(line) {
            Ok(event) => events.push(event),
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(format!("line {}: {err}", index + 1));
                }
            }
        }
    }

    (events, first_error)
}

fn load_meta_lossy(
    run_dir: &Path,
) -> (Option<SessionCatalogMetadataWithCreatedAt>, Option<String>) {
    let meta_path = run_dir.join(META_FILE_NAME);
    if !meta_path.exists() {
        return (None, None);
    }

    let body = match fs::read_to_string(&meta_path) {
        Ok(body) => body,
        Err(err) => {
            return (
                None,
                Some(format!(
                    "failed to read metadata file {}: {err}",
                    meta_path.display()
                )),
            )
        }
    };

    match serde_json::from_str::<SessionCatalogMetadataWithCreatedAt>(&body) {
        Ok(meta) => (Some(meta), None),
        Err(err) => (
            None,
            Some(format!(
                "failed to parse metadata file {}: {err}",
                meta_path.display()
            )),
        ),
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SessionCatalogMetadataWithCreatedAt {
    #[serde(flatten)]
    catalog: SessionCatalogMetadata,
    #[serde(default)]
    created_at: Option<String>,
}

impl AsRef<SessionCatalogMetadata> for SessionCatalogMetadataWithCreatedAt {
    fn as_ref(&self) -> &SessionCatalogMetadata {
        &self.catalog
    }
}

fn system_time_unix_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn parse_unix_ms_from_string_timestamp(value: &str) -> Option<u128> {
    value.parse::<u128>().ok()
}

pub(crate) fn print_human_summary(summary: &ReplaySummary, out: &mut dyn std::io::Write) {
    macro_rules! line {
        ($($arg:tt)*) => {
            let _ = writeln!(out, $($arg)*);
        };
    }
    let session_path = sanitize_human_text(&summary.session_path.display().to_string());

    line!("run_id: {}", sanitize_human_text(&summary.run_id));
    line!(
        "run_name: {}",
        summary
            .run_name
            .as_deref()
            .map(sanitize_human_text)
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    line!("session_path: {session_path}");
    line!("status: {:?}", summary.status);
    line!(
        "workspace_root: {}",
        summary
            .workspace_root
            .as_deref()
            .map(sanitize_human_text)
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    line!("mode: {:?}", summary.mode_source);
    line!(
        "continue: {}",
        if summary.is_resumable {
            "ready".to_string()
        } else {
            summary
                .resume_disabled_reason
                .as_deref()
                .map(|reason| format!("blocked · {}", sanitize_human_text(reason)))
                .unwrap_or_else(|| "blocked".to_string())
        }
    );
    line!(
        "parent_session: {}",
        summary
            .parent_session_id
            .as_deref()
            .map(sanitize_human_text)
            .unwrap_or_else(|| "-".to_string())
    );
    line!("child_sessions: {}", summary.child_session_count);
    line!("artifacts: {}", summary.artifact_count);
    line!("total_events: {}", summary.total_events);
    if let Some(last_error) = &summary.last_error {
        line!("last_error: {}", sanitize_human_text(last_error));
    }
    line!("next_steps:");
    line!("  replay_tui: harness tui --replay {session_path}");
    if summary.is_resumable {
        line!("  continue_tui: harness tui --continue {session_path}");
    } else {
        line!(
            "  continue_tui: unavailable ({})",
            summary
                .resume_disabled_reason
                .as_deref()
                .map(sanitize_human_text)
                .unwrap_or_else(|| RESUME_UNAVAILABLE_FALLBACK_REASON.to_string())
        );
    }
    line!("counts:");
    for (event_type, count) in &summary.counts_by_type {
        line!("  {event_type}: {count}");
    }

    line!("artifacts: {}", summary.artifact_count);
    if summary.artifacts.is_empty() {
        line!("  <none>");
    } else {
        for artifact in &summary.artifacts {
            line!("  - {}", sanitize_human_text(&artifact.path));
            let mut details = Vec::new();
            if let Some(digest) = artifact.digest.as_deref() {
                details.push(format!("digest={}", short_digest(digest)));
            }
            if let Some(bytes) = artifact.bytes {
                details.push(format!("bytes={bytes}"));
            }
            push_sanitized_detail(&mut details, "tool_call", artifact.tool_call_id.as_deref());
            push_sanitized_detail(&mut details, "tool", artifact.tool_id.as_deref());
            push_sanitized_detail(
                &mut details,
                "effective",
                artifact.effective_tool_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "canonical",
                artifact.canonical_tool_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "alias",
                artifact.alias_source_tool_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "child_session",
                artifact.child_session_id.as_deref(),
            );
            details.push(format!(
                "present={}",
                if artifact.present_on_disk {
                    "yes"
                } else {
                    "no"
                }
            ));
            if !details.is_empty() {
                line!("    {}", details.join(", "));
            }
        }
    }

    line!("child_sessions: {}", summary.child_session_count);
    if summary.child_sessions.is_empty() {
        line!("  <none>");
    } else {
        for child in &summary.child_sessions {
            line!("  - {}", sanitize_human_text(&child.child_session_id));
            let mut details = Vec::new();
            push_sanitized_detail(&mut details, "profile", child.profile.as_deref());
            push_sanitized_detail(
                &mut details,
                "provider_model",
                child.provider_model.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "request",
                child.latest_child_request_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "parent_tool",
                child.parent_tool_call_id.as_deref(),
            );
            push_sanitized_detail(&mut details, "parent_task", child.parent_task_id.as_deref());
            push_sanitized_detail(
                &mut details,
                "parent_request",
                child.parent_request_id.as_deref(),
            );
            push_sanitized_detail(
                &mut details,
                "parent_session",
                child.parent_session_id.as_deref(),
            );
            if let Some(terminal_state) = child.terminal_state {
                details.push(format!("state={terminal_state:?}"));
            }
            push_sanitized_detail(
                &mut details,
                "notification",
                child.notification_status.as_deref(),
            );
            if let Some(elapsed_ms) = child.elapsed_ms {
                details.push(format!("elapsed_ms={elapsed_ms}"));
            }
            push_sanitized_detail(&mut details, "reason", child.terminal_reason.as_deref());
            push_sanitized_detail(
                &mut details,
                "notification_summary",
                child.notification_summary.as_deref(),
            );
            if !details.is_empty() {
                line!("    {}", details.join(", "));
            }
            if !child.related_tool_call_ids.is_empty() {
                line!(
                    "    tool_calls={}",
                    child
                        .related_tool_call_ids
                        .iter()
                        .map(|value| sanitize_human_text(value))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !child.artifact_paths.is_empty() {
                line!(
                    "    artifacts={}",
                    child
                        .artifact_paths
                        .iter()
                        .map(|value| sanitize_human_text(value))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !child.next_actions.is_empty() {
                line!("    next_actions:");
                for action in &child.next_actions {
                    line!("      - {}", sanitize_human_text(action));
                }
            }
        }
    }
}

fn sanitize_human_text(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => sanitized.push_str("\\n"),
            '\r' => sanitized.push_str("\\r"),
            '\t' => sanitized.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(&mut sanitized, "\\u{{{:x}}}", u32::from(c));
            }
            _ => sanitized.push(ch),
        }
    }
    sanitized
}

fn push_sanitized_detail(details: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        details.push(format!("{key}={}", sanitize_human_text(value)));
    }
}

fn short_digest(digest: &str) -> String {
    digest.chars().take(12).collect()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
    };
    use harness_core::proj::{SessionCatalogEntry, SessionModeSource};
    use tempfile::tempdir;

    fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: run_id.to_string().into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("test".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload,
        }
    }

    fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap_or_abort())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(run_dir.join(EVENTS_FILE_NAME), format!("{body}\n")).unwrap_or_abort();
    }

    fn inspection_entry(
        run_dir: PathBuf,
        mode_source: SessionModeSource,
    ) -> SessionInspectionEntry {
        SessionInspectionEntry {
            run_dir,
            catalog: SessionCatalogEntry {
                run_id: "run-id".into(),
                run_name: None,
                status: None,
                last_updated_at: None,
                workspace_root: None,
                profile_preset: None,
                provider_model: None,
                mode_source,
                is_resumable: false,
                resume_disabled_reason: None,
                artifact_count: 0,
                child_session_count: 0,
                parent_session_id: None,
            },
            sort_unix_ms: 0,
            artifact_count: 0,
            child_session_count: 0,
        }
    }

    #[test]
    fn session_inspection_entry_owns_operator_visibility_rules() {
        // arrange
        // act
        // assert
        for mode_source in [
            SessionModeSource::InteractiveLive,
            SessionModeSource::InteractiveMock,
            SessionModeSource::Prompt,
            SessionModeSource::Unknown,
        ] {
            assert!(inspection_entry(PathBuf::from("/tmp/visible"), mode_source)
                .is_visible_in_operator_history());
        }

        for mode_source in [
            SessionModeSource::ScenarioFixture,
            SessionModeSource::ReplayOnly,
        ] {
            assert!(!inspection_entry(PathBuf::from("/tmp/hidden"), mode_source)
                .is_visible_in_operator_history());
        }
    }

    #[test]
    fn session_inspection_entry_normalizes_lineage_from_run_meta() {
        // arrange
        // act
        // assert
        let temp = tempdir().unwrap_or_abort();
        let run_dir = temp.path().join("child_session");
        std::fs::create_dir_all(&run_dir).unwrap_or_abort();
        std::fs::write(
            run_dir.join(META_FILE_NAME),
            r#"{"harness_lineage":{"harness_source_run_id":"root_session"}}"#,
        )
        .unwrap_or_abort();

        let entry =
            inspection_entry(run_dir, SessionModeSource::InteractiveLive).normalize_lineage();

        assert_eq!(
            entry.catalog.parent_session_id.as_deref(),
            Some("root_session")
        );
    }

    #[test]
    fn test_inspect_session_catalog_returns_valid_entries() {
        // arrange
        // act
        // assert
        let session_dir = tempdir().unwrap_or_abort();

        // Valid session 1
        let run1_dir = session_dir.path().join("run1");
        std::fs::create_dir_all(&run1_dir).unwrap_or_abort();
        write_events_jsonl(
            &run1_dir,
            &[envelope(
                "run1_id",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "run1-name".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            )],
        );

        // Valid session 2
        let run2_dir = session_dir.path().join("run2");
        std::fs::create_dir_all(&run2_dir).unwrap_or_abort();
        write_events_jsonl(
            &run2_dir,
            &[envelope(
                "run2_id",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "run2-name".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            )],
        );

        // Invalid session (no events.jsonl)
        let run3_dir = session_dir.path().join("run3");
        std::fs::create_dir_all(&run3_dir).unwrap_or_abort();

        // File that is not a directory
        let file_path = session_dir.path().join("not_a_dir.txt");
        std::fs::write(&file_path, "test").unwrap_or_abort();

        let entries = inspect_session_catalog(session_dir.path()).unwrap_or_abort();

        assert_eq!(entries.len(), 2);

        let mut run_ids: Vec<_> = entries.iter().map(|e| e.catalog.run_id.clone()).collect();
        run_ids.sort();

        assert_eq!(run_ids, vec!["run1_id", "run2_id"]);
    }

    #[test]
    fn test_inspect_session_catalog_invalid_dir() {
        // arrange
        // act
        // assert
        let temp = tempdir().unwrap_or_abort();
        let missing_dir = temp.path().join("missing");

        let err = inspect_session_catalog(&missing_dir).unwrap_err();

        assert!(err.contains("failed to read session directory"));
    }
}
