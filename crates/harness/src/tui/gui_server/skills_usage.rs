use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, UNIX_EPOCH};

use harness_core::config::{BilledTokens, HarnessConfig, ModelCost, SkillsConfig};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::provider_catalog::ProviderCatalog;
use harness_providers::CompletionUsage;
use serde_json::{json, Value};

// Keep grouped mutations from racing another request's discovery/preflight.
static SKILLS_LOCK: Mutex<()> = Mutex::new(());

pub(super) async fn dispatch(
    server: &super::Server,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let cwd = params.get("cwd").map_or_else(
        || Ok(server.config_context.discovery.current_dir.clone()),
        |value| {
            value
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| "invalid_params: cwd must be a string".to_owned())
        },
    )?;
    let settings = server.settings(&cwd).await?;
    if method == "usage.subscription" {
        return subscription(&settings, params).await;
    }
    let method = method.to_owned();
    let params = params.clone();
    tokio::task::spawn_blocking(move || match method.as_str() {
        "skills.list" | "skills.read" | "skills.setEnabled" | "skills.trash" => {
            let _lock = SKILLS_LOCK
                .lock()
                .map_err(|_| "skills mutation lock unavailable")?;
            let config = settings
                .config
                .as_ref()
                .map(|c| c.skills.clone())
                .unwrap_or_default();
            skills(
                &settings.workspace_root,
                &settings.data_dir,
                &config,
                &method,
                &params,
            )
        }
        "usage.history" => history(
            &settings.data_dir,
            &settings.session_dir,
            settings.config.as_ref(),
            &params,
        ),
        _ => Err(format!("method_not_found: {method}")),
    })
    .await
    .map_err(|error| error.to_string())?
}

fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params[key]
        .as_str()
        .ok_or_else(|| format!("invalid_params: {key} must be a string"))
}

fn roots(cwd: &Path, home: &Path, config: &SkillsConfig) -> Result<Vec<(String, PathBuf)>, String> {
    let cwd = cwd.canonicalize().map_err(|e| e.to_string())?;
    let mut roots = Vec::new();
    for parent in cwd.ancestors() {
        roots.extend(
            config
                .project_roots
                .iter()
                .map(|p| ("project".into(), parent.join(p))),
        );
        if !config.walk_to_git_root || parent.join(".git").exists() {
            break;
        }
    }
    for path in &config.global_roots {
        let path = if path == Path::new("~/.harness/skills") {
            home.join("skills")
        } else if let Ok(suffix) = path.strip_prefix("~") {
            let Some(home) = std::env::var_os("HOME") else {
                continue;
            };
            PathBuf::from(home).join(suffix)
        } else {
            cwd.join(path)
        };
        roots.push(("user".into(), path));
    }
    let mut seen = BTreeSet::new();
    roots.retain(|(_, path)| seen.insert(path.clone()));
    Ok(roots)
}

fn manifests(
    root: &Path,
    depth: usize,
    found: &mut Vec<PathBuf>,
    scanned: &mut usize,
) -> Result<(), String> {
    if depth > 8 {
        return Ok(());
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *scanned += 1;
        if *scanned > 4096 {
            return Err("skill catalog exceeds 4096 entries".into());
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            manifests(&entry.path(), depth + 1, found, scanned)?;
        } else if kind.is_file()
            && matches!(
                entry.file_name().to_str(),
                Some("SKILL.md" | "SKILL.md.disabled")
            )
        {
            found.push(entry.path());
        }
    }
    Ok(())
}

fn contents(directory: &Path, depth: usize) -> Result<(u64, u64, Option<u64>), String> {
    if depth > 32 {
        return Err("skill contents exceed 32 directory levels".into());
    }
    let (mut files, mut bytes, mut updated) = (0, 0, None);
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        // Never follow supporting-file links outside the skill directory.
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let (f, b, u) = contents(&entry.path(), depth + 1)?;
            files += f;
            bytes += b;
            updated = updated.max(u);
        } else if metadata.is_file() {
            files += 1;
            bytes += metadata.len();
            updated = updated.max(
                metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| u64::try_from(d.as_millis()).ok()),
            );
        }
    }
    Ok((files, bytes, updated))
}

fn body(text: &str) -> &str {
    let mut lines = text.split_inclusive('\n');
    if lines.next().is_none_or(|line| line.trim_end() != "---") {
        return text.trim();
    }
    let mut offset = text.find('\n').map_or(text.len(), |at| at + 1);
    for line in lines {
        offset += line.len();
        if line.trim_end() == "---" {
            return text[offset..].trim();
        }
    }
    text.trim()
}

fn skills(
    cwd: &Path,
    home: &Path,
    config: &SkillsConfig,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let roots = roots(cwd, home, config)?;
    // Use native discovery for authoritative enabled metadata and precedence.
    let mut discovery = config.clone();
    discovery.project_roots.clear();
    discovery.global_roots = roots.iter().map(|(_, root)| root.clone()).collect();
    let native = harness_tools::discover_skill_catalog_with_config(cwd, &discovery)
        .map_err(|e| e.to_string())?;
    let mut grouped: BTreeMap<String, Value> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    let mut scanned = 0;
    for (scope, root) in &roots {
        if fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let mut paths = Vec::new();
        manifests(root, 0, &mut paths, &mut scanned)?;
        for file in paths {
            if !seen.insert(file.clone()) {
                continue;
            }
            let metadata = fs::metadata(&file).map_err(|e| e.to_string())?;
            if metadata.len() > 1024 * 1024 {
                errors.push(format!("{}: skill manifest exceeds 1 MiB", file.display()));
                continue;
            }
            let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
            let enabled = file.file_name().is_some_and(|n| n == "SKILL.md");
            let directory = file.parent().ok_or("skill manifest has no directory")?;
            let entry = native.entries.iter().find(|entry| entry.location == file);
            // Core's YAML frontmatter parser also accepts skill name/description/tools.
            let parsed = if entry.is_none() {
                let header = text
                    .find("\n---")
                    .map_or(text.as_str(), |end| &text[..end + 4]);
                match harness_core::config::parse_subagent_definition(
                    &header
                        .replace("\nallowed-tools:", "\ntools:")
                        .replace("\nallowed_tools:", "\ntools:"),
                ) {
                    Ok(parsed) => Some(parsed),
                    Err(_) => {
                        errors.push(format!("{}: malformed skill metadata", file.display()));
                        None
                    }
                }
            } else {
                None
            };
            let fallback = directory.file_name().unwrap_or_default().to_string_lossy();
            let name = entry
                .map(|e| e.name.as_str())
                .or_else(|| parsed.as_ref().map(|e| e.name.as_str()))
                .unwrap_or(&fallback);
            let description = entry
                .map(|e| e.description.as_str())
                .or_else(|| parsed.as_ref().map(|e| e.description.as_str()))
                .unwrap_or("");
            let disabled_tools = parsed
                .as_ref()
                .map(|entry| {
                    entry
                        .tools
                        .iter()
                        .flat_map(|tool| tool.split_whitespace())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let tools = entry.map_or(disabled_tools.as_slice(), |entry| {
                entry.allowed_tools.as_slice()
            });
            let id = format!("{scope}:{name}");
            let (files, size, updated) = contents(directory, 0)?;
            let install = json!({"directory":directory,"file":file,"root":root,"scope":scope,"source":if root.components().any(|c| c.as_os_str() == ".agents") {"Shared"} else {"Harness"},"enabled":enabled});
            if let Some(row) = grouped.get_mut(&id) {
                row["enabled"] = json!(row["enabled"].as_bool().unwrap_or(false) && enabled);
                if let Some(installs) = row["installs"].as_array_mut() {
                    installs.push(install);
                }
                row["supportingFiles"] =
                    json!(row["supportingFiles"].as_u64().unwrap_or(0) + files.saturating_sub(1));
                row["sizeBytes"] = json!(row["sizeBytes"].as_u64().unwrap_or(0) + size);
                row["updatedAt"] = json!(row["updatedAt"].as_u64().max(updated));
            } else {
                grouped.insert(id.clone(), json!({"id":id,"name":name,"description":description,"scope":scope,"project":if scope == "project" {Some(cwd)} else {None},"enabled":enabled,"body":body(&text),"allowedTools":tools,"primaryDirectory":directory,"primaryFile":file,"installs":[install],"supportingFiles":files.saturating_sub(1),"sizeBytes":size,"updatedAt":updated}));
            }
        }
    }
    if method == "skills.list" {
        return Ok(json!({"skills":grouped.into_values().collect::<Vec<_>>(),"errors":errors}));
    }
    let id = string(params, "id")?;
    let row = grouped.get(id).ok_or("not_found: skill not found")?;
    if method == "skills.read" {
        return Ok(json!({"id":id,"body":row["body"]}));
    }
    let installs = row["installs"].as_array().ok_or("invalid skill installs")?;
    let enabled = if method == "skills.setEnabled" {
        Some(
            params["enabled"]
                .as_bool()
                .ok_or("invalid_params: enabled must be a boolean")?,
        )
    } else {
        None
    };
    let mut operations = Vec::new();
    for install in installs {
        let root = Path::new(string(install, "root")?);
        let file = Path::new(string(install, "file")?);
        let directory = validated_directory(root, file)?;
        if let Some(enabled) = enabled {
            let destination = directory.join(if enabled {
                "SKILL.md"
            } else {
                "SKILL.md.disabled"
            });
            if destination != file {
                if fs::symlink_metadata(&destination).is_ok() {
                    return Err("conflict: both enabled and disabled manifests exist".into());
                }
                operations.push((file.to_path_buf(), destination));
            }
        } else {
            operations.push((directory, PathBuf::new()));
        }
    }
    let mut changed = 0;
    for (source, destination) in operations {
        let result = if enabled.is_some() {
            fs::rename(&source, &destination).map_err(|e| e.to_string())
        } else {
            // gio implements the desktop user's trash, including its restore metadata.
            std::process::Command::new("gio")
                .arg("trash")
                .arg("--")
                .arg(&source)
                .output()
                .map_err(|_| "unsupported: desktop trash service (gio) is unavailable".to_owned())
                .and_then(|output| {
                    if output.status.success() {
                        Ok(())
                    } else {
                        Err("desktop trash operation failed".into())
                    }
                })
        };
        result.map_err(|error| format!("{error}; {changed} installs changed before failure"))?;
        changed += 1;
    }
    Ok(if let Some(enabled) = enabled {
        json!({"id":id,"enabled":enabled,"changed":changed})
    } else {
        json!({"id":id,"trashed":changed})
    })
}

fn validated_directory(root: &Path, file: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let directory = file.parent().ok_or("invalid skill path")?;
    if !matches!(
        file.file_name().and_then(|s| s.to_str()),
        Some("SKILL.md" | "SKILL.md.disabled")
    ) {
        return Err("invalid skill manifest".into());
    }
    let canonical = directory.canonicalize().map_err(|e| e.to_string())?;
    if canonical == root || !canonical.starts_with(&root) {
        return Err(
            "invalid_params: skill directory must be strictly inside a discovered root".into(),
        );
    }
    let relative = file
        .strip_prefix(&root)
        .map_err(|_| "invalid_params: skill path escapes root")?;
    let mut path = root.clone();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("invalid skill path component".into());
        }
        path.push(component);
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("invalid_params: skill mutation cannot follow symlinks".into());
        }
    }
    Ok(canonical)
}

fn date(value: &str) -> Result<(), String> {
    let invalid = || "invalid_params: from/to must be calendar dates YYYY-MM-DD".to_owned();
    if !value.is_ascii()
        || value.len() != 10
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
    {
        return Err(invalid());
    }
    let y: u32 = value[..4].parse().map_err(|_| invalid())?;
    let m: usize = value[5..7].parse().map_err(|_| invalid())?;
    let d: u32 = value[8..].parse().map_err(|_| invalid())?;
    let days = [
        31,
        if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if y == 0 || m == 0 || m > 12 || d == 0 || d > days[m - 1] {
        return Err(invalid());
    }
    Ok(())
}

struct Record {
    session: String,
    provider: String,
    model: String,
    project: String,
    category: &'static str,
    timestamp: String,
    usage: CompletionUsage,
    tokens: BilledTokens,
    cost: Option<u128>,
}

#[derive(Default)]
struct Aggregate {
    records: u64,
    sessions: BTreeSet<String>,
    input: u64,
    cached: u64,
    writes: u64,
    output: u64,
    reasoning: u64,
    total: u64,
    cost: u128,
    unknown: u64,
    last: Option<String>,
}
impl Aggregate {
    fn add(&mut self, record: &Record) {
        self.records += 1;
        self.sessions.insert(record.session.clone());
        self.input += u64::from(record.tokens.input);
        self.cached += u64::from(record.tokens.cache_read);
        self.writes += u64::from(record.tokens.cache_write);
        self.output += u64::from(record.tokens.output);
        self.reasoning += u64::from(record.usage.reasoning_tokens.unwrap_or(0));
        self.total +=
            u64::from(record.usage.prompt_tokens) + u64::from(record.usage.completion_tokens);
        if let Some(cost) = record.cost {
            self.cost += cost;
        } else {
            self.unknown += 1;
        }
        if self
            .last
            .as_ref()
            .is_none_or(|last| last < &record.timestamp)
        {
            self.last = Some(record.timestamp.clone());
        }
    }
    fn value(&self, key: &str) -> Value {
        #[expect(
            clippy::cast_precision_loss,
            reason = "RPC dollar estimates are floating point; internal cost accumulation remains integer picodollars"
        )]
        let known = self.cost as f64 / 1_000_000_000_000.0;
        json!({"key":key,"records":self.records,"sessions":self.sessions.len(),"inputTokens":self.input,"cachedInputTokens":self.cached,"cacheWriteTokens":self.writes,"outputTokens":self.output,"reasoningTokens":self.reasoning,"totalTokens":self.total,"costUsd":if self.unknown == 0 {Some(known)} else {None},"knownCostUsd":known,"unpricedRecords":self.unknown,"lastActive":self.last,"providers":[],"models":[]})
    }
}

fn groups(records: &[Record], key: impl Fn(&Record) -> String, nested: bool) -> Vec<Value> {
    let mut groups: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for record in records {
        groups.entry(key(record)).or_default().push(record);
    }
    groups
        .into_iter()
        .map(|(key, records)| {
            let mut aggregate = Aggregate::default();
            let mut providers: BTreeMap<&str, Aggregate> = BTreeMap::new();
            let mut models: BTreeMap<(&str, &str), Aggregate> = BTreeMap::new();
            for record in records {
                aggregate.add(record);
                if nested {
                    providers.entry(&record.provider).or_default().add(record);
                    models
                        .entry((&record.provider, &record.model))
                        .or_default()
                        .add(record);
                }
            }
            let mut row = aggregate.value(&key);
            row["providers"] = json!(providers
                .into_iter()
                .map(|(provider, sum)| {
                    let mut row = sum.value(provider);
                    row["provider"] = json!(provider);
                    row
                })
                .collect::<Vec<_>>());
            row["models"] = json!(models
                .into_iter()
                .map(|((provider, model), sum)| {
                    let mut row = sum.value(&format!("{provider}/{model}"));
                    row["provider"] = json!(provider);
                    row["model"] = json!(model);
                    row
                })
                .collect::<Vec<_>>());
            row
        })
        .collect()
}

fn prices(home: &Path, config: Option<&HarnessConfig>) -> BTreeMap<(String, String), ModelCost> {
    let mut result = BTreeMap::new();
    for catalog in [
        ProviderCatalog::from_embedded().ok(),
        ProviderCatalog::from_path(&home.join("models-cache.json")).ok(),
    ]
    .into_iter()
    .flatten()
    {
        for provider in catalog.providers() {
            for (model, definition) in &provider.models {
                if let Some(cost) = &definition.definition.cost {
                    result.insert((provider.id.clone(), model.clone()), cost.clone());
                }
            }
        }
    }
    if let Some(config) = config {
        for (provider, definition) in &config.providers {
            for (model, definition) in definition.models() {
                if let Some(cost) = &definition.cost {
                    result.insert((provider.clone(), model.clone()), cost.clone());
                }
            }
        }
    }
    result
}

fn collect_records(
    events: &[EventEnvelopeV1],
    session: &str,
    from: &str,
    to: &str,
    prices: &BTreeMap<(String, String), ModelCost>,
    errors: &mut Vec<String>,
) -> Vec<Record> {
    let project = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::RunStarted(start) => Some(start.workspace_root.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let mut starts = BTreeMap::new();
    let mut finished = BTreeSet::new();
    let mut records = Vec::new();
    for event in events {
        match &event.payload {
            EventV1::ProviderRequestStarted(start) => {
                starts.insert(start.request_id.to_string(), (start, event.ts.as_deref()));
            }
            EventV1::ProviderRequestFinished(end) => {
                let Some(usage) = &end.usage else { continue };
                if !finished.insert(end.request_id.to_string()) {
                    continue;
                }
                let Some(timestamp) = event.ts.as_deref() else {
                    errors.push(format!("{session}: usage record has no timestamp"));
                    continue;
                };
                let Ok(time) = humantime::parse_rfc3339(timestamp) else {
                    errors.push(format!("{session}: usage record has invalid timestamp"));
                    continue;
                };
                let timestamp = humantime::format_rfc3339_millis(time).to_string();
                let day = &timestamp[..10];
                if day < from || day > to {
                    continue;
                }
                let start = starts.get(&end.request_id.to_string());
                let (provider, model) = start
                    .map(|(s, _)| (s.provider_id.clone(), s.model_id.clone()))
                    .unwrap_or_else(|| ("unknown".into(), "unknown".into()));
                let metadata = end.metadata.as_ref();
                let tokens = BilledTokens::new(
                    usage,
                    metadata
                        .and_then(|m| m.cache_read_tokens)
                        .unwrap_or(usage.cache_read_tokens),
                    metadata
                        .and_then(|m| m.cache_write_tokens)
                        .unwrap_or(usage.cache_write_tokens),
                    metadata.and_then(|m| m.cache_write_1h_tokens).unwrap_or(0),
                );
                let reported = metadata.and_then(|m| m.cost_picodollars);
                let source = match provider.as_str() {
                    "codex" => "openai",
                    "anthropic-subscription" => "anthropic",
                    _ => &provider,
                };
                let price = prices
                    .get(&(provider.clone(), model.clone()))
                    .or_else(|| prices.get(&(source.to_owned(), model.clone())));
                let started_ms = start
                    .and_then(|(_, ts)| *ts)
                    .and_then(|ts| humantime::parse_rfc3339(ts).ok())
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_millis()).ok());
                let cost = reported
                    .map(u128::from)
                    .or_else(|| price.and_then(|price| price.picodollars(&tokens, started_ms)));
                records.push(Record {
                    session: session.into(),
                    provider,
                    model,
                    project: project.clone(),
                    category: if end
                        .request_id
                        .to_string()
                        .starts_with(harness_core::event::EVAL_COMPLETION_REQUEST_PREFIX)
                    {
                        "eval"
                    } else {
                        "conversation"
                    },
                    timestamp,
                    usage: usage.clone(),
                    tokens,
                    cost,
                });
            }
            EventV1::SessionCompaction(compaction) => {
                let Some(usage) = &compaction.summary_usage else {
                    continue;
                };
                let Some(time) = event
                    .ts
                    .as_deref()
                    .and_then(|ts| humantime::parse_rfc3339(ts).ok())
                else {
                    errors.push(format!(
                        "{session}: compaction usage has no valid timestamp"
                    ));
                    continue;
                };
                let timestamp = humantime::format_rfc3339_millis(time).to_string();
                if &timestamp[..10] < from || &timestamp[..10] > to {
                    continue;
                }
                let provider = compaction
                    .summary_provider_id
                    .clone()
                    .unwrap_or_else(|| "unknown".into());
                let model = compaction
                    .summary_model_id
                    .clone()
                    .unwrap_or_else(|| "unknown".into());
                let tokens =
                    BilledTokens::new(usage, usage.cache_read_tokens, usage.cache_write_tokens, 0);
                // The compaction journal has no request start time; scheduled prices stay unknown.
                let cost = prices
                    .get(&(provider.clone(), model.clone()))
                    .and_then(|price| price.picodollars(&tokens, None));
                records.push(Record {
                    session: session.into(),
                    provider,
                    model,
                    project: project.clone(),
                    category: "compaction",
                    timestamp,
                    usage: usage.clone(),
                    tokens,
                    cost,
                });
            }
            _ => {}
        }
    }
    records
}

fn history(
    home: &Path,
    session_dir: &Path,
    config: Option<&HarnessConfig>,
    params: &Value,
) -> Result<Value, String> {
    let from = string(params, "from")?;
    let to = string(params, "to")?;
    date(from)?;
    date(to)?;
    if from > to {
        return Err("invalid_params: from must not be after to".into());
    }
    let started = Instant::now();
    let prices = prices(home, config);
    let mut roots = BTreeSet::from([session_dir.to_path_buf()]);
    let sessions_root = home.join("sessions");
    if sessions_root.is_dir() {
        roots.insert(sessions_root.clone());
        for entry in fs::read_dir(&sessions_root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                roots.insert(entry.path());
            }
        }
    }
    let mut directories = BTreeSet::new();
    for root in roots {
        directories.extend(crate::replay::session_directories(&root)?);
    }
    let (mut scanned, mut skipped) = (0, 0);
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for directory in directories {
        scanned += 1;
        let session = directory.to_string_lossy();
        match crate::replay::read_bounded_history(&directory) {
            Ok(events) => {
                let found = collect_records(&events, &session, from, to, &prices, &mut errors);
                if found.is_empty() {
                    skipped += 1;
                }
                records.extend(found);
            }
            Err(_) => errors.push(format!("{session}: journal could not be read")),
        }
    }
    let mut totals = Aggregate::default();
    for record in &records {
        totals.add(record);
    }
    // Persisted cost_picodollars is catalog-priced by the runtime, not a provider bill.
    let reported = 0;
    let unpriced = records.iter().filter(|r| r.cost.is_none()).count();
    let mut models = groups(&records, |r| format!("{}/{}", r.provider, r.model), false);
    for row in &mut models {
        if let Some((provider, model)) = row["key"]
            .as_str()
            .and_then(|k| k.split_once('/'))
            .map(|(p, m)| (p.to_owned(), m.to_owned()))
        {
            row["provider"] = json!(provider);
            row["model"] = json!(model);
        }
    }
    let mut providers = groups(&records, |r| r.provider.clone(), false);
    for row in &mut providers {
        row["provider"] = row["key"].clone();
    }
    let mut projects = groups(&records, |r| r.project.clone(), true);
    for row in &mut projects {
        row["project"] = row["key"].clone();
    }
    Ok(
        json!({"from":from,"to":to,"timeZone":"UTC","totals":totals.value("total"),"byCategory":groups(&records,|r|r.category.to_owned(),true),"byModel":models,"byProvider":providers,"byProject":projects,"byDay":groups(&records,|r|r.timestamp[..10].into(),true),"byMonth":groups(&records,|r|r.timestamp[..7].into(),true),"quality":{"providerReportedRecords":reported,"modelPricedRecords":records.len()-reported-unpriced,"unpricedRecords":unpriced},"scan":{"scanned":scanned,"skipped":skipped,"records":records.len(),"elapsedMs":started.elapsed().as_millis()},"errors":errors,"ratesAvailable":!prices.is_empty(),"cacheSavingsUsd":null}),
    )
}

async fn subscription(settings: &super::LiveSettings, params: &Value) -> Result<Value, String> {
    let provider = string(params, "provider")?;
    let id = settings
        .config
        .as_ref()
        .and_then(|config| config.providers.get(provider))
        .and_then(|configured| configured.credential_provider(provider))
        .or_else(|| harness_core::auth::ProviderId::parse(provider))
        .ok_or("invalid_params: invalid provider")?;
    let operation = async {
        if id == harness_core::auth::ProviderId::anthropic_subscription() {
            super::subscription_anthropic::fetch(&settings.data_dir).await
        } else if id == harness_core::auth::ProviderId::codex() {
            super::subscription_codex::fetch(&settings.data_dir).await
        } else {
            Err("Subscription usage is not supported for this provider".into())
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), operation).await;
    Ok(match result {
        Ok(Ok(Some(mut usage))) => {
            usage["provider"] = json!(provider);
            usage["status"] = json!("available");
            usage
        }
        Ok(Ok(None)) => {
            json!({"provider":provider,"status":"unconfigured","plan":null,"windows":[],"message":"No subscription OAuth credential is configured"})
        }
        Ok(Err(message)) => {
            json!({"provider":provider,"status":"error","plan":null,"windows":[],"message":message})
        }
        Err(_) => {
            json!({"provider":provider,"status":"error","plan":null,"windows":[],"message":"Subscription usage request timed out"})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_toggle_preserves_bodies_and_refuses_collision(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("skills");
        for name in ["one", "two"] {
            fs::create_dir_all(root.join(name))?;
            fs::write(root.join(name).join("SKILL.md"), "---\nname: deploy\ndescription: Deploy safely\nallowed-tools: read bash\n---\nKeep this body.\n")?;
        }
        let config = SkillsConfig {
            project_roots: vec![],
            global_roots: vec![root.clone()],
            ..SkillsConfig::default()
        };
        let list = skills(temp.path(), temp.path(), &config, "skills.list", &json!({}))?;
        assert_eq!(
            list["skills"][0]["installs"].as_array().map(Vec::len),
            Some(2)
        );
        let result = skills(
            temp.path(),
            temp.path(),
            &config,
            "skills.setEnabled",
            &json!({"id":"user:deploy","enabled":false}),
        )?;
        assert_eq!(result["changed"], 2);
        let disabled = skills(temp.path(), temp.path(), &config, "skills.list", &json!({}))?;
        assert_eq!(disabled["skills"][0]["body"], "Keep this body.");
        assert_eq!(disabled["skills"][0]["enabled"], false);
        assert_eq!(
            disabled["skills"][0]["allowedTools"],
            json!(["read", "bash"])
        );
        fs::write(root.join("two/SKILL.md"), "do not overwrite")?;
        assert!(skills(
            temp.path(),
            temp.path(),
            &config,
            "skills.setEnabled",
            &json!({"id":"user:deploy","enabled":true})
        )
        .is_err());
        assert!(!root.join("one/SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(root.join("two/SKILL.md"))?,
            "do not overwrite"
        );
        fs::remove_file(root.join("two/SKILL.md"))?;
        let restored = skills(
            temp.path(),
            temp.path(),
            &config,
            "skills.setEnabled",
            &json!({"id":"user:deploy","enabled":true}),
        )?;
        assert_eq!(restored["changed"], 2);
        assert_eq!(
            skills(
                temp.path(),
                temp.path(),
                &config,
                "skills.read",
                &json!({"id":"user:deploy"})
            )?["body"],
            "Keep this body."
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mutation_rejects_symlink_escape_and_root_deletion() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("skills");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&outside)?;
        fs::write(outside.join("SKILL.md"), "body")?;
        std::os::unix::fs::symlink(&outside, root.join("escape"))?;
        assert!(validated_directory(&root, &root.join("escape/SKILL.md")).is_err());
        fs::write(root.join("SKILL.md"), "body")?;
        assert!(validated_directory(&root, &root.join("SKILL.md")).is_err());
        Ok(())
    }

    #[test]
    fn journal_usage_prices_cache_and_keeps_unknown_cost_explicit(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut events = Vec::new();
        let payloads = [
            json!({"event_type":"run_started","data":{"run_name":"test","workspace_root":"/workspace"}}),
            json!({"event_type":"provider_request_started","data":{"request_id":"a","provider_id":"local","model_id":"priced","prompt_summary":"","request_digest":"","metadata":null}}),
            json!({"event_type":"provider_request_finished","data":{"request_id":"a","finish_reason":"stop","output_digest":null,"usage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"cache_read_tokens":40},"metadata":null}}),
            json!({"event_type":"provider_request_started","data":{"request_id":"b","provider_id":"local","model_id":"unknown","prompt_summary":"","request_digest":"","metadata":null}}),
            json!({"event_type":"provider_request_finished","data":{"request_id":"b","finish_reason":"stop","output_digest":null,"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7},"metadata":null}}),
        ];
        for (seq, payload) in payloads.into_iter().enumerate() {
            events.push(serde_json::from_value(json!({"schema_version":1,"event_id":format!("e{seq}"),"seq":seq+1,"run_id":"run","mono_ms":seq,"ts":"2026-10-16T12:00:00Z","actor":{"kind":"system"},"payload":payload}))?);
        }
        let cost: ModelCost =
            serde_json::from_value(json!({"input":2.0,"output":4.0,"cache_read":0.5}))?;
        let prices = BTreeMap::from([(("local".into(), "priced".into()), cost)]);
        let mut errors = Vec::new();
        let records = collect_records(
            &events,
            "run",
            "2026-10-16",
            "2026-10-16",
            &prices,
            &mut errors,
        );
        let totals = groups(&records, |_| "total".into(), true);
        assert_eq!(totals[0]["inputTokens"], 65);
        assert_eq!(totals[0]["cachedInputTokens"], 40);
        assert_eq!(totals[0]["totalTokens"], 117);
        assert_eq!(totals[0]["unpricedRecords"], 1);
        assert_eq!(totals[0]["costUsd"], Value::Null);
        assert_eq!(records[0].cost, Some(180_000_000));
        assert!(collect_records(
            &events,
            "run",
            "2026-10-17",
            "2026-10-18",
            &prices,
            &mut errors
        )
        .is_empty());
        assert!(errors.is_empty());
        let temp = tempfile::tempdir()?;
        let run = temp.path().join("sessions/project/run");
        harness_core::store::create_private_dir(&run)?;
        if let EventV1::ProviderRequestFinished(end) = &mut events[2].payload {
            end.metadata = Some(harness_core::event::ProviderRequestFinishedMetadata {
                cost_picodollars: Some(180_000_000),
                ..Default::default()
            });
        }
        let mut journal = Vec::new();
        for event in &events {
            serde_json::to_writer(&mut journal, event)?;
            journal.push(b'\n');
        }
        harness_core::store::write_private_atomic(&run.join("events.jsonl"), &journal)?;
        let snapshot = history(
            temp.path(),
            &temp.path().join("sessions/project"),
            None,
            &json!({"from":"2026-10-16","to":"2026-10-16"}),
        )?;
        assert_eq!(snapshot["totals"]["totalTokens"], 117);
        assert_eq!(snapshot["totals"]["sessions"], 1);
        assert_eq!(snapshot["totals"]["unpricedRecords"], 1);
        assert_eq!(snapshot["quality"]["modelPricedRecords"], 1);
        assert_eq!(snapshot["byProject"][0]["project"], "/workspace");
        assert_eq!(snapshot["scan"]["scanned"], 1);
        Ok(())
    }

    #[test]
    fn date_validation_rejects_invalid_calendar_days_without_panicking() {
        for value in ["2026-02-29", "2026-13-01", "0000-01-01", "ééééé"] {
            assert!(date(value).is_err(), "{value}");
        }
        assert!(date("2024-02-29").is_ok());
    }
}
