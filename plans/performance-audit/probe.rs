#![deny(unsafe_code)]

// Audit-only driver. Links the existing release rlibs; no production instrumentation.
use std::hint::black_box;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use harness_core::event::*;
use harness_core::session::history_index::*;
use harness_core::session::{AssistantPart, CanonicalSessionProjection};
use harness_tui::app::AppState;
use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type OpenAiResponseBody = harness_providers::openai::OpenAiResponseBody;
mod sse {
    include!(env!("AUDIT_SSE_SOURCE"));
}

fn envelope(seq: u64, turn: usize, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq}"),
        seq,
        run_id: "run-audit".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-1".into())),
        correlation_id: Some(format!("turn-{turn}")),
        causation_id: None,
        stream_key: Some("agent:agent-1".into()),
        payload,
    }
}

fn turn_events(turn: usize, start: u64) -> Vec<EventEnvelopeV1> {
    let request_id = format!("provider-{turn}");
    vec![
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: format!("turn-{turn}").into(), text: format!("question {turn}"),
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.clone().into(), provider_id: "mock".into(), model_id: "model".into(),
            prompt_summary: "question".into(), request_digest: "digest".into(), metadata: None,
        }),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.clone().into(), finish_reason: "stop".into(), output_digest: None,
            usage: None, metadata: None,
        }),
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: request_id.into(), tool_call_count: 0,
            parts: vec![AssistantPart::Text { text: format!("answer {turn}: {}", "stable text ".repeat(20)) }],
            provenance: None, assistant_message: None,
        }),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: format!("task-{turn}").into(), result_summary: "done".into(),
            result_digest: "digest".into(), metadata: None,
        }),
    ].into_iter().enumerate().map(|(i, payload)| envelope(start + i as u64, turn, payload)).collect()
}

fn history(turns: usize) -> Vec<EventEnvelopeV1> {
    let mut events = vec![
        envelope(1, 0, EventV1::RunStarted(RunStartedEvent { run_name: "interactive".into(), workspace_root: "/audit".into() })),
        envelope(2, 0, EventV1::AgentSpawned(AgentSpawnedEvent { agent_id: "agent-1".into(), profile: "default".into(), parent_agent_id: None })),
    ];
    for turn in 0..turns { events.extend(turn_events(turn, events.len() as u64 + 1)); }
    events
}

fn measure(mut f: impl FnMut() -> Result, count: usize) -> Result<serde_json::Value> {
    let mut samples = Vec::new();
    for _ in 0..count {
        let start = Instant::now(); f()?; samples.push(start.elapsed().as_micros());
    }
    samples.sort_unstable();
    Ok(json!({"median_us": samples[count / 2], "max_us": samples[count - 1], "samples_us": samples}))
}

fn projections(turns: usize) -> Result {
    let events = history(turns);
    let full = measure(|| { black_box(CanonicalSessionProjection::from_event_history(black_box(&events))?); Ok(()) }, 5)?;
    let conversation = measure(|| { black_box(harness_core::conversation::project_conversation(&events, &[])?); Ok(()) }, 5)?;
    let transcript = measure(|| { black_box(harness_core::transcript_projection::project_transcript(&events)?); Ok(()) }, 5)?;
    let legacy = measure(|| { black_box(harness_core::session::legacy::LegacyEventLogAdapter::new().project(&events)?); Ok(()) }, 5)?;
    let clone = measure(|| { black_box(events.clone()); Ok(()) }, 5)?;
    let mut projection = CanonicalSessionProjection::from_event_history(&events)?;
    let next = turn_events(turns, events.len() as u64 + 1);
    let start = Instant::now(); projection.apply_events(&next)?; let append_us = start.elapsed().as_micros();
    println!("{}", json!({"workload":"projection", "turns":turns, "events":events.len(), "full":full, "conversation":conversation, "transcript":transcript, "legacy":legacy, "clone":clone, "append_one_turn_us":append_us}));
    Ok(())
}

fn tui(turns: usize) -> Result {
    let events = history(turns);
    let mut app = AppState::new_live(None, false, None);
    let start = Instant::now(); app.replace_events(events.clone()); let load_us = start.elapsed().as_micros();
    assert!(app.canonical_projection_error().is_none());
    let mut terminal = Terminal::new(TestBackend::new(120, 40))?;
    let start = Instant::now(); terminal.draw(|f| harness_tui::ui::render_app(f, &app))?; let cold_us = start.elapsed().as_micros();
    let hot = measure(|| { terminal.draw(|f| harness_tui::ui::render_app(f, &app))?; Ok(()) }, 10)?;
    let next = turn_events(turns, events.len() as u64 + 1);
    for event in &next[..2] { app.ingest_event(event.clone()); }
    let live = RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: "live-audit".into(), run_id: "run-audit".into(), mono_ms: events.len() as u64 + 3, ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-1".into())),
        correlation_id: Some(format!("turn-{turns}")), causation_id: None, stream_key: Some("agent:agent-1".into()),
        payload: LiveEventV1::ProviderTextDelta { request_id: format!("provider-{turns}").into(), delta: " token".into() },
    }));
    let ingest = measure(|| { app.ingest_runtime_event(live.clone()); Ok(()) }, 10)?;
    terminal.draw(|f| harness_tui::ui::render_app(f, &app))?;
    let dirty_samples = std::env::var("AUDIT_DIRTY_SAMPLES").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let dirty_render = measure(|| {
        app.ingest_runtime_event(live.clone());
        terminal.draw(|f| harness_tui::ui::render_app(f, &app))?;
        Ok(())
    }, dirty_samples)?;
    let mut settle_us = Vec::new();
    for event in &next[2..] {
        let start = Instant::now(); app.ingest_event(event.clone()); settle_us.push(start.elapsed().as_micros());
    }
    assert!(app.canonical_projection_error().is_none());
    println!("{}", json!({"workload":"tui", "turns":turns,"load_us":load_us,"cold_render_us":cold_us,"hot_render":hot,"live_ingest":ingest,"live_ingest_and_render":dirty_render,"settle_us":settle_us}));
    Ok(())
}

fn index(sessions: usize) -> Result {
    let root = tempfile::tempdir_in("target/perf-artifacts")?;
    let path = root.path().join("events.jsonl");
    let event = history(1).remove(0);
    let mut journal = std::fs::File::create(&path)?;
    serde_json::to_writer(&mut journal, &event)?; journal.write_all(b"\n")?;
    let mut reducer = SessionHistoryRowReducer::new(root.path().join("active"), "run-audit".into(), "interactive".into(), "/audit".into(), None);
    persist_committed_history_row(root.path(), &path, &mut reducer, &event)?;
    let index_path = root.path().join(SESSION_HISTORY_INDEX_FILE_NAME);
    let mut state = read_valid_index(&index_path).ok_or("missing index")?;
    let entry = state.entries.values().next().ok_or("missing row")?.clone();
    for i in 1..sessions {
        let mut entry = entry.clone(); entry.entry.run_dir = root.path().join(format!("run-{i}")); entry.entry.catalog.run_id = format!("run-{i}");
        state.entries.insert(entry.entry.run_dir.clone(), entry);
    }
    write_history_index(&index_path, &state)?;
    let bytes = index_path.metadata()?.len();
    let parse = measure(|| { black_box(read_valid_index(&index_path).ok_or("read index")?); Ok(()) }, 20)?;
    let serialize = measure(|| { black_box(serde_json::to_vec_pretty(&state)?); Ok(()) }, 20)?;
    let compact_body = serde_json::to_vec(&state)?;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&compact_body)?, serde_json::to_value(&state)?);
    let compact_path = root.path().join("compact.json");
    std::fs::write(&compact_path, &compact_body)?;
    let compact_parse = measure(|| { black_box(read_valid_index(&compact_path).ok_or("read compact index")?); Ok(()) }, 20)?;
    let compact_serialize = measure(|| { black_box(serde_json::to_vec(&state)?); Ok(()) }, 20)?;
    let persist = measure(|| {
        serde_json::to_writer(&mut journal, &event)?; journal.write_all(b"\n")?;
        persist_committed_history_row(root.path(), &path, &mut reducer, &event)?; Ok(())
    }, 20)?;
    println!("{}", json!({"workload":"index", "sessions":sessions,"index_bytes":bytes,"parse":parse,"serialize":serialize,"append_and_persist":persist,"compact":{"bytes":compact_body.len(),"parse":compact_parse,"serialize":compact_serialize}}));
    Ok(())
}

fn streams(frame_bytes: usize, chunk_bytes: usize) -> Result {
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let data = format!("data: {}\n\n", "x".repeat(frame_bytes));
    let timing = measure(|| {
        let chunks: Vec<_> = data.as_bytes().chunks(chunk_bytes).map(|c| Ok(c.to_vec())).collect();
        let mut body: OpenAiResponseBody = Box::pin(tokio_stream::iter(chunks));
        let mut buffer = Vec::new();
        let event = runtime.block_on(sse::next_sse_event(&mut body, &mut buffer))?.ok_or("no event")?;
        assert_eq!(event.data.len(), frame_bytes); black_box(event); Ok(())
    }, 5)?;
    println!("{}", json!({"workload":"sse", "frame_bytes":frame_bytes,"chunk_bytes":chunk_bytes,"timing":timing}));
    Ok(())
}

fn workspace() -> Result {
    let timing = measure(|| { black_box(harness_core::workspace::WorkspaceEnvironment::current()); Ok(()) }, 30)?;
    println!("{}", json!({"workload":"workspace_discovery", "timing":timing}));
    Ok(())
}

fn filesystem(files: usize, tool_id: &str, sparse: bool) -> Result {
    let root = tempfile::tempdir_in("target/perf-artifacts")?;
    let root_path = root.path().canonicalize()?;
    let mut paths = Vec::new();
    for i in 0..files {
        let path = root_path.join(format!("file-{i:06}.txt"));
        let mut file = std::fs::File::create(&path)?;
        let line = if sparse && i % 100 != 0 { "ordinary stable line\n" } else { "needle stable line\n" };
        file.write_all(line.repeat(50).as_bytes())?;
        file.set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs((i * 37 % files) as u64)))?;
        paths.push(path);
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let _guard = runtime.enter();
    let coordinator = harness_core::coord::spawn_coordinator(
        harness_core::coord::CoordinatorConfig::default(),
        Arc::new(harness_core::clock::RealClock::new()),
        Arc::new(harness_core::redact::DefaultRedactor::default()),
    );
    let ctx = harness_core::tool::ToolContext {
        run_id: "run-audit".into(), workspace_root: root_path.clone(), artifacts_dir: root_path.join(".agent-harness/sessions/artifacts"),
        actor: EventActor::new(ActorKind::Supervisor, None), profile: None, tool_call_id: "tool-audit".into(),
        current_model_ref: None, current_model_settings: None, tool_state: Default::default(),
        external_directory_allow_prefixes: Vec::new(), coordinator,
    };
    let registry = harness_tools::coordinator_registry(Default::default());
    let tool = registry.get(tool_id).ok_or("missing tool")?;
    let args = if tool_id == "glob" { json!({"pattern":"*.txt", "limit":100}) } else { json!({"pattern":"needle", "include":"*.txt", "limit":100}) };
    let call = measure(|| { black_box(runtime.block_on(tool.call(ctx.clone(), args.clone()))?); Ok(()) }, 5)?;
    let mut sort = serde_json::Value::Null;
    if tool_id == "glob" {
        let stat_count = std::cell::Cell::new(0);
        let mtime = |p: &std::path::PathBuf| {
            stat_count.set(stat_count.get() + 1);
            std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH)
        };
        let repeated = measure(|| { let mut p = paths.clone(); p.sort_by(|a,b| mtime(b).cmp(&mtime(a))); black_box(p); Ok(()) }, 5)?;
        let repeated_stats = stat_count.replace(0) / 5;
        let cached = measure(|| { let mut p = paths.clone(); p.sort_by_cached_key(|p| std::cmp::Reverse(mtime(p))); black_box(p); Ok(()) }, 5)?;
        let cached_stats = stat_count.get() / 5;
        let mut original = paths.clone(); original.sort_by(|a,b| mtime(b).cmp(&mtime(a)));
        let mut candidate = paths; candidate.sort_by_cached_key(|p| std::cmp::Reverse(mtime(p)));
        assert_eq!(original, candidate);
        sort = json!({"original":repeated,"cached_key":cached,"original_metadata_calls":repeated_stats,"cached_metadata_calls":cached_stats});
    }
    println!("{}", json!({"workload":if sparse { "grep_sparse" } else { tool_id },"files":files,"tool_call":call,"sort":sort}));
    Ok(())
}

fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    let size = args.get(2).map(|v| v.parse()).transpose()?.unwrap_or(100);
    match args.get(1).map(String::as_str) {
        Some("projection") => projections(size),
        Some("tui") => tui(size),
        Some("index") => index(size),
        Some("sse") => streams(size, args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(64)),
        Some("workspace") => workspace(),
        Some("glob") => filesystem(size, "glob", false),
        Some("grep") => filesystem(size, "grep", false),
        Some("grep_sparse") => filesystem(size, "grep", true),
        _ => Err("usage: probe projection|tui|index|sse|glob|grep|grep_sparse SIZE [CHUNK_SIZE], or probe workspace".into()),
    }
}
