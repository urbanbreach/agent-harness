use super::*;

pub(super) fn date(value: &str) -> Result<(), String> {
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

pub(super) struct Record {
    origin: (String, u64),
    session: String,
    provider: String,
    model: String,
    project: String,
    category: &'static str,
    timestamp: String,
    usage: CompletionUsage,
    tokens: BilledTokens,
    pub(super) cost: Option<u128>,
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

pub(super) fn groups(
    records: &[Record],
    key: impl Fn(&Record) -> String,
    nested: bool,
) -> Vec<Value> {
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

pub(super) fn collect_records(
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
                finished.remove(start.request_id.as_str());
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
                    origin: (event.run_id.to_string(), event.seq),
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
                    origin: (event.run_id.to_string(), event.seq),
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

pub(super) fn history(
    home: &Path,
    session_dir: &Path,
    config: Option<&HarnessConfig>,
    params: &Value,
) -> Result<Value, String> {
    let from = super::super::required(params, "from")?;
    let to = super::super::required(params, "to")?;
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
    let mut origins = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    let (mut scanned, mut skipped) = (0, 0);
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for directory in &directories {
        let metadata_file = match harness_core::store::open_private_file(
            &directory.join(harness_core::proj::META_FILE_NAME),
        ) {
            Ok(file) => Some(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        if let Some(file) = metadata_file {
            use std::io::Read;
            let metadata: Value =
                serde_json::from_reader(file.take(1_048_577)).map_err(|e| e.to_string())?;
            let lineage = &metadata["harness_lineage"];
            if lineage["relationship"] == "child_session_materialization"
                && let (Some(run), Some(source), Some(cutoff)) = (
                    metadata["run_id"].as_str(),
                    lineage["source_run_id"].as_str(),
                    lineage["source_cutoff_seq"].as_u64(),
                )
            {
                origins.insert(run.to_owned(), (source.to_owned(), cutoff));
            }
        }
        scanned += 1;
        let session = directory.to_string_lossy();
        match crate::replay::read_bounded_history(directory) {
            Ok(events) => {
                for event in &events {
                    if !matches!(
                        event.payload,
                        EventV1::ProviderRequestFinished(_) | EventV1::SessionCompaction(_)
                    ) {
                        continue;
                    }
                    if let Some((run, seq)) = event
                        .event_id
                        .strip_prefix("source:")
                        .and_then(|id| id.rsplit_once(':'))
                        .and_then(|(run, seq)| seq.parse::<u64>().ok().map(|seq| (run, seq)))
                    {
                        aliases
                            .insert((event.run_id.to_string(), event.seq), (run.to_owned(), seq));
                    }
                }
                let found = collect_records(&events, &session, from, to, &prices, &mut errors);
                if found.is_empty() {
                    skipped += 1;
                }
                records.extend(found);
            }
            Err(_) => errors.push(format!("{session}: journal could not be read")),
        }
    }
    let mut seen = BTreeSet::new();
    records.retain(|record| {
        let mut key = record.origin.clone();
        let mut visited = BTreeSet::new();
        while visited.insert(key.clone()) {
            if let Some(source) = aliases.get(&key) {
                key = source.clone();
            } else if let Some((source, cutoff)) = origins.get(&key.0) {
                if key.1 > *cutoff {
                    break;
                }
                key.0.clone_from(source);
            } else {
                break;
            }
        }
        // Mirrors and materialized prefixes retain the original billing identity.
        seen.insert(key)
    });
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
