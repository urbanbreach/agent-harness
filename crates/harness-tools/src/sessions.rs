mod args;
pub(crate) mod history;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use history::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub(crate) enum SessionTool {
    List,
    Read,
    Search,
    Info,
}
#[async_trait::async_trait]
impl Tool for SessionTool {
    fn id(&self) -> &str {
        match self {
            Self::List => "session_list",
            Self::Read => "session_read",
            Self::Search => "session_search",
            Self::Info => "session_info",
        }
    }
    fn description(&self) -> &str {
        match self {
        Self::List => "List saved sessions using status, profile, resumable and text filters. Reads journals only.",
        Self::Read => "Read redacted event and message windows from a saved session. Offsets are zero-based; from_end returns newest first.",
        Self::Search => "Search redacted saved session text. Never returns reasoning or raw tool arguments.",
        Self::Info => "Inspect a saved session's status, lineage, event counts, artifacts and resume readiness.",
    }
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        match self {
            Self::List => schemars::schema_for!(args::List).to_value(),
            Self::Read => schemars::schema_for!(args::Read).to_value(),
            Self::Search => schemars::schema_for!(args::Search).to_value(),
            Self::Info => schemars::schema_for!(args::Info).to_value(),
        }
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        args.get("sessionRoot")
            .or_else(|| args.get("session_root"))
            .filter(|value| !value.is_null())
            .map_or_else(
                || Ok(Vec::new()),
                |value| {
                    value
                        .as_str()
                        .map(|path| vec![PathBuf::from(path)])
                        .ok_or_else(|| invalid("sessionRoot must be a path"))
                },
            )
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let selector = ["session", "run_id", "path"]
            .into_iter()
            .find_map(|key| args.get(key).and_then(Value::as_str))
            .unwrap_or("*");
        let mut requests = vec![(self.id().into(), selector.into())];
        if let Some(name) = Path::new(selector)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|name| *name != selector)
        {
            requests.push((self.id().into(), name.into()));
        }
        if let Some(path) = args
            .get("sessionRoot")
            .or_else(|| args.get("session_root"))
            .and_then(Value::as_str)
        {
            requests.push(("read".into(), path.into()));
        }
        requests
    }
    async fn call(&self, ctx: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let run = ctx.coordinator.run_info().await.map_err(invalid)?;
        let default = run
            .run_dir
            .parent()
            .ok_or_else(|| invalid("session root missing"))?
            .to_path_buf();
        let tool = *self;
        tokio::task::spawn_blocking(move || {
            cancelled(&ctx)?;
            let mut result = match tool {
                Self::List => list(&ctx, &default, parse(value)?),
                Self::Read => read(&ctx, &default, parse(value)?),
                Self::Search => search(&ctx, &default, parse(value)?),
                Self::Info => info(&ctx, &default, parse(value)?),
            }?;
            result["source"] = "event_replay".into();
            result["redacted"] = true.into();
            harness_core::redact::redact_in_place(ctx.redactor.as_ref(), &mut result);
            Ok(ToolResult::structured(result.to_string(), result))
        })
        .await
        .map_err(invalid)?
    }
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(invalid)
}
fn invalid(error: impl std::fmt::Display) -> ToolError {
    ToolError::InvalidArguments(error.to_string())
}
fn limit(value: Option<usize>, default: usize) -> usize {
    value.unwrap_or(default).clamp(1, 200)
}

fn list(ctx: &ToolContext, default: &Path, args: args::List) -> Result<Value, ToolError> {
    if args
        .status
        .as_deref()
        .is_some_and(|s| !["running", "finished", "failed"].contains(&s))
    {
        return Err(invalid("status must be running, finished or failed"));
    }
    let sort = args.sort.as_deref().unwrap_or("updated_desc");
    if ![
        "updated_desc",
        "updated_asc",
        "name",
        "run_id",
        "run_id_asc",
        "run_id_desc",
    ]
    .contains(&sort)
    {
        return Err(invalid(
            "sort must be updated_desc, updated_asc, name, run_id_asc or run_id_desc",
        ));
    }
    let root = root(ctx, default, args.session_root.as_deref())?;
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for dir in directories(ctx, &root)? {
        match load(ctx, &dir) {
            Ok(entry) => {
                let value = harness_core::redact::redact_value(
                    ctx.redactor.as_ref(),
                    &json!(entry.catalog),
                );
                if args.status.as_deref().is_some_and(|s| value["status"] != s)
                    || args
                        .profile
                        .as_deref()
                        .is_some_and(|p| value["profile_preset"] != p)
                    || args.resumable.is_some_and(|b| value["is_resumable"] != b)
                    || args.filter.as_ref().is_some_and(|q| {
                        !value.to_string().to_lowercase().contains(&q.to_lowercase())
                    })
                {
                    continue;
                }
                entries.push(value);
            }
            Err(ToolError::Cancelled) => return Err(ToolError::Cancelled),
            Err(error) => {
                errors.push(json!({"session":dir.file_name(),"error":error.to_string()}));
            }
        }
    }
    let key = match sort {
        "name" => "run_name",
        "run_id" | "run_id_asc" | "run_id_desc" => "run_id",
        _ => "last_updated_at",
    };
    entries.sort_by(|a, b| {
        let order = a[key]
            .as_str()
            .cmp(&b[key].as_str())
            .then_with(|| a["run_id"].as_str().cmp(&b["run_id"].as_str()));
        if matches!(sort, "updated_desc" | "run_id_desc") {
            order.reverse()
        } else {
            order
        }
    });
    let total = entries.len();
    entries.truncate(limit(args.limit, 50));
    Ok(
        json!({"sessions":entries,"errors":errors,"total_count":total,"returned_count":entries.len(),"truncated":entries.len()<total,"effective_limit":limit(args.limit,50)}),
    )
}
fn read(ctx: &ToolContext, default: &Path, args: args::Read) -> Result<Value, ToolError> {
    let root = root(ctx, default, args.session_root.as_deref())?;
    let entry = load(ctx, &directory(ctx, &root, &args.session)?)?;
    let active = harness_core::conversation_rewind::active_events(&entry.events);
    let mut events: Vec<_> = active.iter().collect();
    if args.from_end {
        events.reverse();
    }
    let messages: Vec<_> = events
        .iter()
        .copied()
        .filter(|e| {
            matches!(
                e.payload,
                harness_core::event::EventV1::UserMessageSubmitted(_)
                    | harness_core::event::EventV1::AssistantMessageFinished(_)
            )
        })
        .collect();
    let event_limit = limit(args.event_limit, 25);
    let message_limit = limit(args.message_limit, 25);
    let window: Vec<_> = events
        .iter()
        .skip(args.event_offset)
        .take(event_limit)
        .map(|e| summary(ctx, e))
        .collect();
    let message_window: Vec<_> = messages
        .iter()
        .skip(args.message_offset)
        .take(message_limit)
        .map(|e| summary(ctx, e))
        .collect();
    let mut value = json!({"catalog":entry.catalog,"events":window,"messages":message_window,
        "event_offset":args.event_offset,"effective_event_limit":event_limit,"total_event_count":events.len(),"returned_event_count":window.len(),"truncated":args.event_offset.saturating_add(window.len())<events.len(),
        "message_offset":args.message_offset,"effective_message_limit":message_limit,"total_message_count":messages.len(),"returned_message_count":message_window.len(),"message_truncated":args.message_offset.saturating_add(message_window.len())<messages.len(),"from_end":args.from_end});
    if args.include_todos {
        value["todos"] = crate::todos::project(&entry.events);
    }
    Ok(value)
}
fn search(ctx: &ToolContext, default: &Path, args: args::Search) -> Result<Value, ToolError> {
    if args.query.trim().is_empty() || args.query.len() > 4096 {
        return Err(invalid("query must contain 1–4096 bytes of text"));
    }
    let root = root(ctx, default, args.session_root.as_deref())?;
    let dirs = match &args.session {
        Some(id) => vec![directory(ctx, &root, id)?],
        None => directories(ctx, &root)?,
    };
    let query = regex::RegexBuilder::new(&regex::escape(&args.query))
        .case_insensitive(!args.case_sensitive)
        .build()
        .map_err(invalid)?;
    let mut matches = Vec::new();
    let mut count = 0;
    let mut errors = Vec::new();
    for dir in &dirs {
        let entry = match load(ctx, dir) {
            Ok(entry) => entry,
            Err(error) if args.session.is_none() && !matches!(error, ToolError::Cancelled) => {
                errors.push(json!({"session":dir.file_name(),"error":error.to_string()}));
                continue;
            }
            Err(error) => return Err(error),
        };
        for event in harness_core::conversation_rewind::active_events(&entry.events).iter() {
            cancelled(ctx)?;
            let Some(text) = document(event) else {
                continue;
            };
            let text = ctx.redactor.redact_text(&text);
            let Some(found) = query.find(&text) else {
                continue;
            };
            count += 1;
            if matches.len() < limit(args.limit, 50) {
                matches.push(json!({"session_id":entry.catalog.run_id,"seq":event.seq,"event_id":event.event_id,"matched_field":"text","excerpt":excerpt(&text,found.range(),args.context_limit.unwrap_or(80).clamp(1,500))}));
            }
        }
    }
    Ok(
        json!({"matches":matches,"errors":errors,"total_count":count,"returned_count":matches.len(),"truncated":count>matches.len(),"searched_session_count":dirs.len(),"effective_limit":limit(args.limit,50)}),
    )
}
fn excerpt(text: &str, found: std::ops::Range<usize>, context: usize) -> String {
    let start = text[..found.start]
        .char_indices()
        .rev()
        .nth(context - 1)
        .map_or(0, |(i, _)| i);
    let end = text[found.end..]
        .char_indices()
        .nth(context)
        .map_or(text.len(), |(i, _)| found.end + i);
    text[start..end].into()
}
fn info(ctx: &ToolContext, default: &Path, args: args::Info) -> Result<Value, ToolError> {
    let root = root(ctx, default, args.session_root.as_deref())?;
    let entry = load(ctx, &directory(ctx, &root, &args.session)?)?;
    Ok(json!({"catalog":entry.catalog,"event_counts":entry.counts,
        "lineage":{"parent_session_id":entry.catalog.parent_session_id,"child_session_count":entry.catalog.child_session_count},
        "artifact_index_summary":artifacts(ctx,&entry.events),
        "recovery":{"is_resumable":entry.catalog.is_resumable,"resume_disabled_reason":entry.catalog.resume_disabled_reason}}))
}
