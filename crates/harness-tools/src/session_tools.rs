// allow: SIZE_OK — session management (lineage + projection + inspection)
use crate::UnwrapOrAbort;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use harness_core::event::EventEnvelopeV1;
use harness_core::proj::{
    project_session_catalog_entry, RunStatus, SessionCatalogEntry, SessionCatalogMetadata,
};
use harness_core::redact::{redact_value, DefaultRedactor};
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::tool_metadata;
use harness_core::ToolResultExt;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::workspace_paths::{
    canonical_workspace_root, ensure_within_workspace_path, normalize_workspace_target_path,
};
use crate::{parse_tool_args, text_json_artifacts_tool_result, text_json_tool_result};

mod summaries;

use summaries::{
    event_counts_by_type, safe_event_summary, safe_message_summaries, safe_search_documents,
    search_excerpt,
};

const DEFAULT_SESSION_ROOT: &str = ".agent-harness/sessions";
const EVENTS_FILE_NAME: &str = "events.jsonl";
const META_FILE_NAME: &str = "meta.json";
const ARTIFACTS_DIR_NAME: &str = "artifacts";
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 200;
const DEFAULT_EVENT_LIMIT: usize = 25;
const MAX_EVENT_LIMIT: usize = 200;
const DEFAULT_MESSAGE_LIMIT: usize = 25;
const MAX_MESSAGE_LIMIT: usize = 200;
const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 200;
const DEFAULT_SEARCH_CONTEXT_LIMIT: usize = 80;
const MAX_SEARCH_CONTEXT_LIMIT: usize = 500;
pub(crate) const MAX_TOOL_INLINE_JSON_CHARS: usize = 24_000;

pub(crate) struct SessionListTool;
pub(crate) struct SessionReadTool;
pub(crate) struct SessionSearchTool;
pub(crate) struct SessionInfoTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionListArgs {
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    session_root: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    resumable: Option<bool>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionReadArgs {
    #[serde(alias = "run_id", alias = "path")]
    session: String,
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    session_root: Option<String>,
    #[serde(default, rename = "eventOffset", alias = "event_offset")]
    event_offset: Option<u32>,
    #[serde(default, rename = "eventLimit", alias = "event_limit")]
    event_limit: Option<u32>,
    #[serde(default, rename = "messageOffset", alias = "message_offset")]
    message_offset: Option<u32>,
    #[serde(default, rename = "messageLimit", alias = "message_limit")]
    message_limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionSearchArgs {
    query: String,
    #[serde(default, alias = "run_id")]
    session: Option<String>,
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    session_root: Option<String>,
    #[serde(default, rename = "caseSensitive", alias = "case_sensitive")]
    case_sensitive: bool,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default, rename = "contextLimit", alias = "context_limit")]
    context_limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionInfoArgs {
    #[serde(alias = "run_id", alias = "path")]
    session: String,
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    session_root: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionEntry {
    run_dir: PathBuf,
    catalog: SessionCatalogEntry,
    events: Vec<EventEnvelopeV1>,
    parse_errors: Vec<String>,
    sort_unix_ms: u128,
}

#[async_trait]
impl Tool for SessionListTool {
    tool_metadata!(
        "session_list",
        "Lists replay-derived Harness sessions from the local session root with filtering, sorting, caps, and redaction. This tool is side-effect free and does not call providers, tools, hooks, MCP servers, or the Harness CLI.",
        ToolCapability::ReadFs,
        crate::json_schema_for::<SessionListArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SessionListArgs = parse_tool_args(args_json)?;
        let session_root = resolve_session_root(&ctx, args.session_root.as_deref())?;
        let mut entries = load_session_entries(&session_root)?;
        entries.retain(|entry| {
            status_matches(args.status.as_deref(), entry.catalog.status)
                && args
                    .profile
                    .as_deref()
                    .is_none_or(|profile| entry.catalog.profile_preset.as_deref() == Some(profile))
                && args
                    .resumable
                    .is_none_or(|resumable| entry.catalog.is_resumable == resumable)
                && args
                    .filter
                    .as_deref()
                    .is_none_or(|filter| session_filter_matches(entry, filter))
        });
        sort_session_entries(&mut entries, args.sort.as_deref())?;
        let limit = clamp_limit(args.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
        let total_count = entries.len();
        let returned = entries
            .iter()
            .take(limit.effective)
            .map(|entry| {
                json!({
                    "run_dir": display_path(&entry.run_dir),
                    "catalog": entry.catalog,
                    "event_count": entry.events.len(),
                    "parse_error_count": entry.parse_errors.len(),
                    "source": "event_replay",
                })
            })
            .collect::<Vec<_>>();

        Ok(text_json_tool_result(
            format!("{} session(s)", returned.len()),
            redact_json(json!({
                "source": "event_replay",
                "session_root": display_path(&session_root),
                "status": args.status,
                "profile": args.profile,
                "resumable": args.resumable,
                "filter": args.filter,
                "sort": args.sort.unwrap_or_else(|| "updated_desc".to_string()),
                "limit": limit.effective,
                "requested_limit": limit.requested,
                "effective_limit": limit.effective,
                "max_limit": MAX_LIST_LIMIT,
                "limit_clamped": limit.clamped,
                "total_count": total_count,
                "returned_count": returned.len(),
                "truncated_count": total_count.saturating_sub(returned.len()),
                "truncated": total_count > returned.len(),
                "redacted": true,
                "sessions": returned,
            })),
        ))
    }
}

#[async_trait]
impl Tool for SessionReadTool {
    tool_metadata!(
        "session_read",
        "Reads a bounded, redacted, replay-derived event/message window for one Harness session. This is side-effect free and never shells out to the Harness CLI.",
        ToolCapability::ReadFs,
        crate::json_schema_for::<SessionReadArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SessionReadArgs = parse_tool_args(args_json)?;
        let session_root = resolve_session_root(&ctx, args.session_root.as_deref())?;
        let entry = resolve_session_entry(&ctx, &session_root, &args.session)?;
        let offset = args.event_offset.unwrap_or(0) as usize;
        let limit = clamp_limit(args.event_limit, DEFAULT_EVENT_LIMIT, MAX_EVENT_LIMIT);
        let message_offset = args.message_offset.unwrap_or(0) as usize;
        let message_limit =
            clamp_limit(args.message_limit, DEFAULT_MESSAGE_LIMIT, MAX_MESSAGE_LIMIT);
        let total_count = entry.events.len();
        let events = entry
            .events
            .iter()
            .skip(offset)
            .take(limit.effective)
            .map(safe_event_summary)
            .collect::<Vec<_>>();
        let message_summaries = safe_message_summaries(&entry);
        let total_message_count = message_summaries.len();
        let messages = message_summaries
            .into_iter()
            .skip(message_offset)
            .take(message_limit.effective)
            .collect::<Vec<_>>();
        let payload = redact_json(json!({
            "source": "event_replay",
            "redacted": true,
            "session_root": display_path(&session_root),
            "run_dir": display_path(&entry.run_dir),
            "catalog": entry.catalog,
            "event_offset": offset,
            "event_limit": limit.effective,
            "requested_event_limit": limit.requested,
            "effective_event_limit": limit.effective,
            "max_event_limit": MAX_EVENT_LIMIT,
            "event_limit_clamped": limit.clamped,
            "total_event_count": total_count,
            "returned_event_count": events.len(),
            "truncated": offset.saturating_add(events.len()) < total_count,
            "message_offset": message_offset,
            "message_limit": message_limit.effective,
            "requested_message_limit": message_limit.requested,
            "effective_message_limit": message_limit.effective,
            "max_message_limit": MAX_MESSAGE_LIMIT,
            "message_limit_clamped": message_limit.clamped,
            "total_message_count": total_message_count,
            "returned_message_count": messages.len(),
            "message_truncated": message_offset.saturating_add(messages.len()) < total_message_count,
            "parse_errors": entry.parse_errors,
            "events": events,
            "messages": messages,
        }));
        maybe_spill_json(
            &ctx,
            "session-read.json",
            format!("{} event(s)", events_len(&payload, "events")),
            payload,
        )
    }
}

#[async_trait]
impl Tool for SessionSearchTool {
    tool_metadata!(
        "session_search",
        "Searches replay-safe session text (user messages, provider prompt summaries, tool summaries, titles, and metadata) with capped, redacted excerpts.",
        ToolCapability::ReadFs,
        crate::json_schema_for::<SessionSearchArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SessionSearchArgs = parse_tool_args(args_json)?;
        let session_root = resolve_session_root(&ctx, args.session_root.as_deref())?;
        let entries = if let Some(selector) = args.session.as_deref() {
            vec![resolve_session_entry(&ctx, &session_root, selector)?]
        } else {
            load_session_entries(&session_root)?
        };
        let limit = clamp_limit(args.limit, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT);
        let context_limit = clamp_limit(
            args.context_limit,
            DEFAULT_SEARCH_CONTEXT_LIMIT,
            MAX_SEARCH_CONTEXT_LIMIT,
        );
        let mut total_count = 0usize;
        let mut matches = Vec::new();
        for entry in &entries {
            for document in safe_search_documents(entry) {
                if let Some(excerpt) = search_excerpt(
                    &document.text,
                    &args.query,
                    args.case_sensitive,
                    context_limit.effective,
                ) {
                    total_count += 1;
                    if matches.len() < limit.effective {
                        matches.push(json!({
                            "session_id": entry.catalog.run_id,
                            "run_dir": display_path(&entry.run_dir),
                            "seq": document.seq,
                            "event_id": document.event_id,
                            "matched_field": document.field,
                            "excerpt": excerpt,
                            "source": "event_replay",
                        }));
                    }
                }
            }
        }
        let payload = redact_json(json!({
            "source": "event_replay",
            "redacted": true,
            "query": args.query,
            "case_sensitive": args.case_sensitive,
            "limit": limit.effective,
            "requested_limit": limit.requested,
            "effective_limit": limit.effective,
            "max_limit": MAX_SEARCH_LIMIT,
            "limit_clamped": limit.clamped,
            "context_limit": context_limit.effective,
            "requested_context_limit": context_limit.requested,
            "effective_context_limit": context_limit.effective,
            "max_context_limit": MAX_SEARCH_CONTEXT_LIMIT,
            "context_limit_clamped": context_limit.clamped,
            "searched_session_count": entries.len(),
            "total_count": total_count,
            "returned_count": matches.len(),
            "truncated_count": total_count.saturating_sub(matches.len()),
            "truncated": total_count > matches.len(),
            "matches": matches,
        }));
        maybe_spill_json(
            &ctx,
            "session-search.json",
            format!("{} match(es)", total_count.min(limit.effective)),
            payload,
        )
    }
}

#[async_trait]
impl Tool for SessionInfoTool {
    tool_metadata!(
        "session_info",
        "Reports replay-derived metadata, lineage, status, event counts, artifacts, and recovery notes for one Harness session without dumping whole logs.",
        ToolCapability::ReadFs,
        crate::json_schema_for::<SessionInfoArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SessionInfoArgs = parse_tool_args(args_json)?;
        let session_root = resolve_session_root(&ctx, args.session_root.as_deref())?;
        let entry = resolve_session_entry(&ctx, &session_root, &args.session)?;
        let event_counts = event_counts_by_type(&entry.events);
        let artifact_summary = artifact_index_summary(&entry.run_dir)?;

        let payload = redact_json(json!({
            "source": "event_replay",
            "redacted": true,
            "session_root": display_path(&session_root),
            "run_dir": display_path(&entry.run_dir),
            "catalog": entry.catalog,
            "metadata": load_session_metadata(&entry.run_dir)?.map(|metadata| json!({
                "run_id": metadata.run_id,
                "run_name": metadata.run_name,
                "workspace_root": metadata.workspace_root,
                "profile_preset": metadata.profile_preset,
                "provider": metadata.provider,
                "model": metadata.model,
                "mode_source": metadata.mode_source,
            })),
            "lineage": {
                "parent_session_id": entry.catalog.parent_session_id,
                "child_session_count": entry.catalog.child_session_count,
            },
            "event_counts": event_counts,
            "artifact_index_summary": artifact_summary,
            "recovery": {
                "is_resumable": entry.catalog.is_resumable,
                "resume_disabled_reason": entry.catalog.resume_disabled_reason,
                "parse_errors": entry.parse_errors,
            },
        }));

        maybe_spill_json(
            &ctx,
            "session-info.json",
            format!("session {}", entry.catalog.run_id),
            payload,
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct ClampedLimit {
    requested: Option<usize>,
    effective: usize,
    clamped: bool,
}

fn clamp_limit(requested: Option<u32>, default_value: usize, max_value: usize) -> ClampedLimit {
    let requested = requested.map(|value| value as usize);
    let raw = requested.unwrap_or(default_value);
    let effective = raw.clamp(1.min(max_value), max_value);
    ClampedLimit {
        requested,
        effective,
        clamped: raw != effective,
    }
}

fn resolve_session_root(ctx: &ToolContext, selector: Option<&str>) -> Result<PathBuf, ToolError> {
    let workspace = canonical_workspace_root(ctx)?;
    let relative = selector.unwrap_or(DEFAULT_SESSION_ROOT);
    let root = normalize_workspace_target_path(&workspace, Path::new(relative))?;
    if root.exists() {
        let canonical = root.canonicalize().map_err(|err| {
            ToolError::Execution(format!("failed to resolve session root: {err}"))
        })?;
        ensure_within_workspace_path(&workspace, &canonical)?;
        Ok(canonical)
    } else {
        Ok(root)
    }
}

fn load_session_entries(session_root: &Path) -> Result<Vec<SessionEntry>, ToolError> {
    if !session_root.exists() {
        return Ok(Vec::new());
    }
    let canonical_root = session_root
        .canonicalize()
        .tool_err("failed to resolve session root")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(session_root).tool_err("failed to read session root")? {
        let entry = entry.tool_err("failed to inspect session")?;
        let path = entry.path();
        if path.is_dir() {
            let canonical = path.canonicalize().map_err(|err| {
                ToolError::Execution(format!(
                    "failed to resolve session entry {}: {err}",
                    path.display()
                ))
            })?;
            ensure_within_workspace_path(&canonical_root, &canonical)?;
            entries.push(load_session_entry(&canonical)?);
        }
    }
    Ok(entries)
}

fn resolve_session_entry(
    ctx: &ToolContext,
    session_root: &Path,
    selector: &str,
) -> Result<SessionEntry, ToolError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(ToolError::InvalidArguments(
            "session selector is required".to_string(),
        ));
    }
    if selector.contains("..") {
        return Err(ToolError::InvalidArguments(
            "session selector cannot contain parent traversal".to_string(),
        ));
    }

    let run_dir =
        if selector.contains('/') || selector.contains('\\') || Path::new(selector).is_absolute() {
            let workspace = canonical_workspace_root(ctx)?;
            let path = normalize_workspace_target_path(&workspace, Path::new(selector))?;
            let canonical = path.canonicalize().map_err(|err| {
                ToolError::Execution(format!("failed to resolve session path: {err}"))
            })?;
            let canonical_root = session_root.canonicalize().map_err(|err| {
                ToolError::Execution(format!("failed to resolve session root: {err}"))
            })?;
            ensure_within_workspace_path(&canonical_root, &canonical)?;
            canonical
        } else {
            let direct = session_root.join(selector);
            if direct.is_dir() {
                let canonical = direct.canonicalize().map_err(|err| {
                    ToolError::Execution(format!("failed to resolve session path: {err}"))
                })?;
                let canonical_root = session_root.canonicalize().map_err(|err| {
                    ToolError::Execution(format!("failed to resolve session root: {err}"))
                })?;
                ensure_within_workspace_path(&canonical_root, &canonical)?;
                canonical
            } else {
                let matches = load_session_entries(session_root)?
                    .into_iter()
                    .filter(|entry| {
                        entry.catalog.run_id == selector
                            || entry
                                .run_dir
                                .file_name()
                                .and_then(|name| name.to_str())
                                .is_some_and(|name| name.starts_with(selector))
                    })
                    .collect::<Vec<_>>();
                return match matches.len() {
                    1 => Ok(matches.into_iter().next().unwrap_or_abort()),
                    0 => Err(ToolError::InvalidArguments(format!(
                        "unknown session `{selector}` in {}",
                        session_root.display()
                    ))),
                    _ => Err(ToolError::InvalidArguments(format!(
                        "multiple sessions matched `{selector}`; use a unique run id or path"
                    ))),
                };
            }
        };
    load_session_entry(&run_dir)
}

fn load_session_entry(run_dir: &Path) -> Result<SessionEntry, ToolError> {
    let (events, parse_errors) = load_events_lossy(run_dir)?;
    let last_updated_at = run_dir
        .join(EVENTS_FILE_NAME)
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_unix_ms)
        .map(|value| value.to_string());
    let sort_unix_ms = last_updated_at
        .as_deref()
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or(0);
    let metadata = load_session_metadata(run_dir)?;
    let fallback_run_id = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown-run>");
    let degraded_reason =
        (!parse_errors.is_empty()).then(|| format!("{} event parse error(s)", parse_errors.len()));
    let catalog = project_session_catalog_entry(
        events.iter(),
        fallback_run_id,
        metadata.as_ref(),
        last_updated_at,
        degraded_reason,
    )
    .unwrap_or_else(|err| SessionCatalogEntry {
        run_id: events
            .first()
            .map(|event| event.run_id.to_string())
            .unwrap_or_else(|| fallback_run_id.to_string()),
        run_name: None,
        status: None,
        last_updated_at: None,
        workspace_root: None,
        profile_preset: None,
        provider_model: None,
        mode_source: harness_core::proj::SessionModeSource::Unknown,
        is_resumable: false,
        resume_disabled_reason: Some(format!("projection unavailable: {err}")),
        artifact_count: artifact_index_summary(run_dir).map_or(0, |summary| {
            summary
                .get("artifact_count")
                .and_then(Value::as_u64)
                .map_or(0, |v| usize::try_from(v).unwrap_or(0))
        }),
        child_session_count: 0,
        parent_session_id: None,
    });
    Ok(SessionEntry {
        run_dir: run_dir.to_path_buf(),
        catalog,
        events,
        parse_errors,
        sort_unix_ms,
    })
}

fn load_events_lossy(run_dir: &Path) -> Result<(Vec<EventEnvelopeV1>, Vec<String>), ToolError> {
    let Some(path) = resolve_existing_session_child_path(run_dir, EVENTS_FILE_NAME)? else {
        return Ok((
            Vec::new(),
            vec![format!(
                "missing events file {}",
                run_dir.join(EVENTS_FILE_NAME).display()
            )],
        ));
    };
    let text = fs::read_to_string(&path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read events file {}: {err}",
            path.display()
        ))
    })?;
    let mut events = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<EventEnvelopeV1>(line) {
            Ok(event) => events.push(event),
            Err(err) => errors.push(format!("line {}: {err}", index + 1)),
        }
    }
    Ok((events, errors))
}

fn load_session_metadata(run_dir: &Path) -> Result<Option<SessionCatalogMetadata>, ToolError> {
    let Some(path) = resolve_existing_session_child_path(run_dir, META_FILE_NAME)? else {
        return Ok(None);
    };
    let body = fs::read_to_string(&path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read session metadata {}: {err}",
            path.display()
        ))
    })?;
    Ok(serde_json::from_str(&body).ok())
}

fn resolve_existing_session_child_path(
    run_dir: &Path,
    child_name: &str,
) -> Result<Option<PathBuf>, ToolError> {
    let path = run_dir.join(child_name);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(ToolError::Execution(format!(
                "failed to inspect session path {}: {err}",
                path.display()
            )))
        }
    }
    let canonical_run_dir = run_dir.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "failed to resolve session directory {}: {err}",
            run_dir.display()
        ))
    })?;
    let canonical = path.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "failed to resolve session path {}: {err}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(&canonical_run_dir) {
        return Err(ToolError::Execution(format!(
            "session path {} resolved outside session directory {}",
            canonical.display(),
            canonical_run_dir.display()
        )));
    }
    Ok(Some(canonical))
}

fn artifact_index_summary(run_dir: &Path) -> Result<Value, ToolError> {
    let Some(artifact_root) = resolve_existing_session_child_path(run_dir, ARTIFACTS_DIR_NAME)?
    else {
        return Ok(json!({
            "artifact_count": 0,
            "total_bytes": 0,
            "paths": [],
        }));
    };
    let mut paths = Vec::new();
    let mut artifact_count = 0usize;
    let mut total_bytes = 0u64;
    for entry in walkdir::WalkDir::new(&artifact_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        artifact_count = artifact_count.saturating_add(1);
        total_bytes = total_bytes.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
        if paths.len() < 25 {
            paths.push(
                path.strip_prefix(run_dir)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    Ok(json!({
        "artifact_count": artifact_count,
        "total_bytes": total_bytes,
        "paths": paths,
    }))
}

fn status_matches(filter: Option<&str>, status: Option<RunStatus>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    matches!(
        (filter.trim().to_ascii_lowercase().as_str(), status),
        ("running", Some(RunStatus::Running))
            | ("finished", Some(RunStatus::Finished))
            | ("failed", Some(RunStatus::Failed))
            | ("unavailable", None)
    )
}

fn session_filter_matches(entry: &SessionEntry, filter: &str) -> bool {
    let filter = filter.to_ascii_lowercase();
    [
        entry.catalog.run_id.as_str(),
        entry.catalog.run_name.as_deref().unwrap_or(""),
        entry.catalog.profile_preset.as_deref().unwrap_or(""),
        entry.catalog.provider_model.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .any(|value| value.to_ascii_lowercase().contains(&filter))
}

fn sort_session_entries(entries: &mut [SessionEntry], sort: Option<&str>) -> Result<(), ToolError> {
    match sort.unwrap_or("updated_desc") {
        "updated_desc" => entries.sort_by(|a, b| b.sort_unix_ms.cmp(&a.sort_unix_ms).then_with(|| a.catalog.run_id.cmp(&b.catalog.run_id))),
        "updated_asc" => entries.sort_by(|a, b| a.sort_unix_ms.cmp(&b.sort_unix_ms).then_with(|| a.catalog.run_id.cmp(&b.catalog.run_id))),
        "run_id_asc" => entries.sort_by(|a, b| a.catalog.run_id.cmp(&b.catalog.run_id)),
        "run_id_desc" => entries.sort_by(|a, b| b.catalog.run_id.cmp(&a.catalog.run_id)),
        other => {
            return Err(ToolError::InvalidArguments(format!(
                "unsupported session sort `{other}`; expected updated_desc, updated_asc, run_id_asc, or run_id_desc"
            )))
        }
    }
    Ok(())
}

fn system_time_unix_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn redact_json(value: Value) -> Value {
    redact_value(&DefaultRedactor::default(), &value)
}

fn events_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn maybe_spill_json(
    ctx: &ToolContext,
    artifact_name: &str,
    display_text: String,
    payload: Value,
) -> Result<ToolResult, ToolError> {
    let body = serde_json::to_string_pretty(&payload).tool_err("failed to serialize output")?;
    if body.len() <= MAX_TOOL_INLINE_JSON_CHARS {
        return Ok(text_json_tool_result(display_text, payload));
    }
    let artifact = ctx
        .artifact_store()
        .map_err(|err| ToolError::Execution(err.to_string()))?
        .write_text(artifact_name, &body)
        .map_err(|err| ToolError::Execution(err.to_string()))?;
    let artifact_ref = ArtifactRef {
        path: artifact.path,
        digest: artifact.digest,
    };
    Ok(text_json_artifacts_tool_result(
        format!(
            "{display_text}; full output spilled to {}",
            artifact_ref.path
        ),
        json!({
            "source": "event_replay",
            "redacted": true,
            "spilled": true,
            "artifact": artifact_ref,
        }),
        vec![artifact_ref],
    ))
}

/// Load events from a child session by session_id (agent_id).
/// Returns `Ok(None)` when the session directory or events file does not exist.
pub(crate) fn load_child_session_events(
    ctx: &ToolContext,
    session_id: &str,
) -> Result<Option<Vec<EventEnvelopeV1>>, ToolError> {
    if session_id.trim().is_empty()
        || session_id.contains("..")
        || session_id.contains('/')
        || session_id.contains('\\')
    {
        return Err(ToolError::InvalidArguments(
            "session_id must be a non-empty identifier without path separators or parent traversal"
                .to_string(),
        ));
    }
    let session_root = ctx
        .artifacts_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| {
            ToolError::Execution("cannot derive session root from artifacts_dir".to_string())
        })?;
    let session_dir = session_root.join(session_id);
    if !session_dir.is_dir() {
        return Ok(None);
    }
    let events_path = session_dir.join(EVENTS_FILE_NAME);
    if !events_path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&events_path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read events file {}: {err}",
            events_path.display()
        ))
    })?;
    let mut events = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<EventEnvelopeV1>(line) {
            events.push(event);
        }
    }
    Ok(Some(events))
}

pub(crate) fn summarize_event(event: &EventEnvelopeV1) -> Value {
    summaries::safe_event_summary(event)
}
