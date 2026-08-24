use harness::UnwrapOrAbort;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use harness_core::clock::RealClock;
use harness_core::config::ShellAllowlist;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1,
    EventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFinishedEvent,
    RunStartedEvent, TaskCompletedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolRunState};
use harness_tools::coordinator_registry;
use serde_json::json;

mod common;

use common::CliHarness;

const SESSION_COUNT: usize = 120;
const TURNS_PER_SESSION: usize = 6;
const SEARCH_NEEDLE: &str = "needle-large-session";

#[test]
fn perf_large_session_list_reopen_and_session_search_write_artifact() {
    // arrange
    let workspace = tempfile::tempdir().unwrap_or_abort();
    let session_root = workspace.path().join(".agent-harness/sessions");
    fs::create_dir_all(&session_root).unwrap_or_abort();
    write_large_session_corpus(&session_root, workspace.path());

    let session_dir_arg = session_root.display().to_string();
    let list_started = Instant::now();
    let list_output = CliHarness::new()
        .args([
            "--session-dir".to_string(),
            session_dir_arg.clone(),
            "sessions".to_string(),
            "list".to_string(),
            "--json".to_string(),
        ])
        .output();
    // act
    let list_elapsed_ms = list_started.elapsed().as_millis();
    // assert
    assert!(
        list_output.status.success(),
        "sessions list stderr:\n{}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_json: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).unwrap_or_abort();
    assert_eq!(list_json.as_array().map(Vec::len), Some(SESSION_COUNT));

    let target_run_id = format!("run_perf_large_{:03}", SESSION_COUNT - 1);
    let reopen_started = Instant::now();
    let reopen_output = CliHarness::new()
        .args([
            "--session-dir".to_string(),
            session_dir_arg,
            "sessions".to_string(),
            "reopen".to_string(),
            "--session".to_string(),
            target_run_id.clone(),
            "--json".to_string(),
        ])
        .output();
    let reopen_elapsed_ms = reopen_started.elapsed().as_millis();
    assert!(
        reopen_output.status.success(),
        "sessions reopen stderr:\n{}",
        String::from_utf8_lossy(&reopen_output.stderr)
    );
    let reopen_json: serde_json::Value =
        serde_json::from_slice(&reopen_output.stdout).unwrap_or_abort();
    assert_eq!(reopen_json["summary"]["run_id"], target_run_id);
    assert_eq!(reopen_json["summary"]["resumable"], true);

    let registry = coordinator_registry(ShellAllowlist::default());
    let search_started = Instant::now();
    let workspace_root = workspace.path().to_path_buf();
    let search = run_async_tool(async move {
        registry
            .get("session_search")
            .unwrap_or_abort()
            .call(
                tool_context(&workspace_root, "perf-session-search"),
                json!({"query": SEARCH_NEEDLE, "limit": 200}),
            )
            .await
    })
    .unwrap_or_abort();
    let search_elapsed_ms = search_started.elapsed().as_millis();
    let search_json = search.structured_json.as_ref().unwrap_or_abort();
    let search_json = if search_json
        .get("spilled")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        let artifact_path = search_json
            .pointer("/artifact/path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_abort();
        serde_json::from_slice(&fs::read(workspace.path().join(artifact_path)).unwrap_or_abort())
            .unwrap_or_abort()
    } else {
        search_json.clone()
    };
    assert_eq!(
        search_json
            .get("searched_session_count")
            .and_then(serde_json::Value::as_u64),
        Some(SESSION_COUNT as u64)
    );
    assert_eq!(
        search_json
            .get("returned_count")
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
    assert!(
        search_json
            .get("total_count")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|count| count >= SESSION_COUNT as u64),
        "search should scan the full corpus"
    );

    let artifact_root = perf_artifact_root();
    fs::create_dir_all(&artifact_root).unwrap_or_abort();
    let artifact_path = artifact_root.join("large-session-surfaces.json");
    let total_events = SESSION_COUNT * events_per_session();
    let artifact = json!({
        "schema_version": "harness-large-session-perf-v1",
        "timestamp_unix_ms": now_unix_ms(),
        "corpus": {
            "session_count": SESSION_COUNT,
            "turns_per_session": TURNS_PER_SESSION,
            "events_per_session": events_per_session(),
            "total_events": total_events,
        },
        "measurements": {
            "sessions_list_ms": list_elapsed_ms,
            "sessions_list_returned": SESSION_COUNT,
            "sessions_reopen_ms": reopen_elapsed_ms,
            "sessions_reopen_run_id": target_run_id,
            "session_search_ms": search_elapsed_ms,
            "session_search_total_matches": search_json["total_count"],
            "session_search_returned": search_json["returned_count"],
            "session_search_searched_sessions": search_json["searched_session_count"],
        },
        "provenance": {
            "command_hint": "scripts/test-lanes.sh perf",
            "artifact_root_env": std::env::var("HARNESS_PERF_ARTIFACT_DIR").ok(),
            "workspace_root": workspace.path().display().to_string(),
        },
    });
    fs::write(
        &artifact_path,
        serde_json::to_vec_pretty(&artifact).unwrap_or_abort(),
    )
    .unwrap_or_abort();

    let recorded: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact_path).unwrap_or_abort()).unwrap_or_abort();
    assert_eq!(recorded["schema_version"], "harness-large-session-perf-v1");
    assert_eq!(recorded["corpus"]["session_count"], SESSION_COUNT);
    assert_eq!(recorded["corpus"]["total_events"], total_events);
    assert!(
        recorded["measurements"]["sessions_list_ms"].is_number()
            && recorded["measurements"]["sessions_reopen_ms"].is_number()
            && recorded["measurements"]["session_search_ms"].is_number()
    );
}

fn write_large_session_corpus(session_root: &Path, workspace_root: &Path) {
    for index in 0..SESSION_COUNT {
        let run_id = format!("run_perf_large_{index:03}");
        let run_dir = session_root.join(run_id.as_str());
        fs::create_dir_all(&run_dir).unwrap_or_abort();
        let events = large_session_events(run_id.as_str(), index, workspace_root);
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap_or_abort())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
    }
}

fn large_session_events(run_id: &str, index: usize, workspace_root: &Path) -> Vec<EventEnvelopeV1> {
    let mut seq = 1_u64;
    let agent_id = "agent_000001";
    let mut events = vec![
        envelope(
            run_id,
            seq,
            EventActor::new(ActorKind::System, None),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: workspace_root.display().to_string(),
            }),
        ),
        envelope(
            run_id,
            seq + 1,
            EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: agent_id.to_string(),
                profile: "build".to_string(),
                parent_agent_id: None,
            }),
        ),
    ];
    seq += 2;

    for turn in 0..TURNS_PER_SESSION {
        let counter = (index * TURNS_PER_SESSION) + turn + 1;
        let request_id = format!("req_{counter}");
        let task_id = format!("task_{counter}");
        for payload in [
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone().into(),
                text: format!("{SEARCH_NEEDLE} user prompt {index:03}/{turn:03}"),
            }),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.clone().into(),
                provider_id: "mock".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: format!("{SEARCH_NEEDLE} provider prompt {index:03}/{turn:03}"),
                request_digest: format!("digest-{index:03}-{turn:03}"),
                metadata: None,
            }),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.clone().into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("output-digest-{index:03}-{turn:03}")),
                usage: None,
                metadata: None,
            }),
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: request_id.clone().into(),
                tool_call_count: 0,
                parts: Vec::new(),
                provenance: None,
                assistant_message: None,
            }),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: task_id.clone().into(),
                result_summary: format!("{SEARCH_NEEDLE} completed turn {index:03}/{turn:03}"),
                result_digest: format!("task-digest-{index:03}-{turn:03}"),
                metadata: None,
            }),
        ] {
            let mut event = envelope(
                run_id,
                seq,
                EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
                payload,
            );
            event.correlation_id = Some(request_id.clone());
            events.push(event);
            seq += 1;
        }
    }

    events.push(envelope(
        run_id,
        seq,
        EventActor::new(ActorKind::System, None),
        EventV1::RunFinished(RunFinishedEvent {
            summary: format!("{SEARCH_NEEDLE} finished session {index:03}"),
        }),
    ));
    events
}

fn envelope(run_id: &str, seq: u64, actor: EventActor, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:06}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn events_per_session() -> usize {
    2 + (TURNS_PER_SESSION * 5) + 1
}

fn tool_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: "run-perf-session-surfaces".into(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: EventActor::new(ActorKind::Supervisor, None),
        profile: None,
        tool_call_id: tool_call_id.into(),
        current_model_ref: None,
        current_model_settings: None,
        tool_state: ToolRunState::default(),
        external_directory_allow_prefixes: Vec::new(),
        coordinator,
    }
}

fn run_async_tool<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_abort();
    runtime.block_on(future)
}

fn perf_artifact_root() -> PathBuf {
    std::env::var_os("HARNESS_PERF_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("target/perf-artifacts/standalone")
        })
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_abort()
        .as_millis()
}
