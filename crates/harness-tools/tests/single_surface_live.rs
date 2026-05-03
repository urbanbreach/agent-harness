use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{load_config_from_file, McpConfig, PermissionMode};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::edit::hashline::compute_line_hash;
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};
use tokio::time::{sleep, Duration, Instant};

const SURFACE_LIVE_PROFILE: &str = "surface_live";

fn surface_live_toolset() -> Vec<String> {
    [
        "bash",
        "batch",
        "codesearch",
        "edit",
        "glob",
        "grep",
        "invalid",
        "list",
        "lsp",
        "question",
        "read",
        "skill",
        "task",
        "todoread",
        "todowrite",
        "webfetch",
        "websearch",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn example_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("configs")
        .join("harness.example.jsonc")
}

fn actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn spawn_test_http_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test http server");
    let addr = listener.local_addr().expect("local addr");
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nhello fetch\n",
            );
        }
    });
    format!("http://{addr}")
}

fn read_events(path: &std::path::Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}

async fn wait_for_question_permission(path: &std::path::Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(permission_id) =
            read_events(path)
                .into_iter()
                .find_map(|event| match event.payload {
                    EventV1::PermissionRequested(data) if data.kind == "question" => {
                        Some(data.permission_id)
                    }
                    _ => None,
                })
        {
            return permission_id;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for question permission");
        }
        sleep(Duration::from_millis(20)).await;
    }
}

fn example_profiles(
    config: &harness_core::config::HarnessConfig,
) -> BTreeMap<String, AgentProfile> {
    let mut profiles = config
        .agents
        .iter()
        .map(|(name, profile)| {
            (
                name.clone(),
                AgentProfile {
                    name: name.clone(),
                    category: name.clone(),
                    model_ref: profile.model_ref.clone(),
                    system_prompt: profile.description.clone(),
                    max_iters: profile.max_iters,
                    temperature: profile.temperature,
                    tool_failure_mode: profile.tool_failure_mode,
                    toolset: profile.tools.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let build_profile = profiles
        .get("build")
        .expect("build profile present in example config")
        .clone();
    profiles.insert(
        SURFACE_LIVE_PROFILE.to_string(),
        AgentProfile {
            name: SURFACE_LIVE_PROFILE.to_string(),
            category: SURFACE_LIVE_PROFILE.to_string(),
            model_ref: build_profile.model_ref,
            system_prompt: "Single-surface live registry test profile.".to_string(),
            temperature: build_profile.temperature,
            max_iters: build_profile.max_iters,
            tool_failure_mode: build_profile.tool_failure_mode,
            toolset: surface_live_toolset(),
        },
    );
    profiles
}

#[tokio::test]
async fn example_config_exposes_single_surface_tools_through_live_registry() {
    let config = load_config_from_file(&example_config_path()).expect("load example config");
    let registry = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        McpConfig::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    );
    for tool_id in [
        "read",
        "list",
        "glob",
        "grep",
        "bash",
        "edit",
        "webfetch",
        "todowrite",
        "todoread",
        "task",
        "batch",
        "skill",
        "question",
        "websearch",
        "codesearch",
        "lsp",
    ] {
        assert!(registry.get(tool_id).is_some(), "missing tool {tool_id}");
        assert!(
            config
                .agents
                .values()
                .any(|profile| profile.tools.iter().any(|tool| tool == tool_id)),
            "example config does not expose tool {tool_id} in any shipped profile"
        );
    }

    assert!(registry.get("invalid").is_some(), "missing tool invalid");
}

#[tokio::test]
async fn single_surface_tools_execute_under_example_config() {
    let config = load_config_from_file(&example_config_path()).expect("load example config");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path().join("sessions");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("existing.txt"), "alpha\nbeta\n").expect("seed existing file");
    fs::create_dir_all(workspace.join("src")).expect("src dir");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"compat_lsp\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("seed cargo manifest");
    fs::write(
        workspace.join("src/lib.rs"),
        "fn helper() {}\n\nfn caller() {\n    helper();\n}\n",
    )
    .expect("seed rust file");

    let mut coordinator_config = CoordinatorConfig::new(session_dir.clone());
    coordinator_config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );
    coordinator_config.tool_registry = Arc::new(coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        McpConfig::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    ));
    coordinator_config.agent_profiles = example_profiles(&config);

    let handle = spawn_coordinator(
        coordinator_config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("single_surface_live", &workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent(
            EventActor::new(ActorKind::Supervisor, None),
            SURFACE_LIVE_PROFILE,
            None,
        )
        .await
        .expect("spawn worker");

    let create = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "edit",
            serde_json::json!({
                "filePath": "written.txt",
                "editId": "surface-create",
                "edits": [
                    {
                        "op": "append",
                        "lines": ["hello from surface"],
                    }
                ],
            }),
        )
        .await
        .expect("edit create tool");
    assert!(create.display_text.contains("Edit applied successfully"));

    let escaped = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "edit",
            serde_json::json!({
                "filePath": "../escape.txt",
                "edits": [
                    {
                        "op": "append",
                        "lines": ["blocked"],
                    }
                ],
            }),
        )
        .await;
    assert!(escaped.is_err(), "workspace escape edit should fail");

    let read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "read",
            serde_json::json!({ "filePath": "written.txt" }),
        )
        .await
        .expect("read tool");
    assert!(read.display_text.contains("1#"));
    assert!(read.display_text.contains("|hello from surface"));

    let listed = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "list",
            serde_json::json!({ "path": "." }),
        )
        .await
        .expect("list tool");
    assert!(listed.display_text.contains("written.txt"));

    let globbed = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "glob",
            serde_json::json!({ "pattern": "**/*.txt" }),
        )
        .await
        .expect("glob tool");
    assert!(globbed.display_text.contains("written.txt"));

    let grepped = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "grep",
            serde_json::json!({ "pattern": "surface", "path": "." }),
        )
        .await
        .expect("grep tool");
    assert!(grepped
        .display_text
        .contains("written.txt:1: hello from surface"));

    let bashed = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "bash",
            serde_json::json!({
                "command": "printf 'cargo surface\\n'",
                "description": "Emit shell smoke output",
            }),
        )
        .await
        .expect("bash tool");
    assert!(bashed.display_text.contains("cargo"));

    let large_bash = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "bash",
            serde_json::json!({
                "command": "printf 'surface%.0s' {1..10000}",
                "description": "Emit many surface lines",
            }),
        )
        .await
        .expect("large bash tool");
    let large_bash_json = large_bash
        .structured_json
        .clone()
        .expect("large bash structured json");
    assert!(
        large_bash.display_text.contains("[truncated:"),
        "display_text:\n{}\nstructured_json:\n{}",
        large_bash.display_text,
        serde_json::to_string_pretty(&large_bash_json).expect("render large bash json")
    );
    assert_eq!(large_bash.artifacts.len(), 1);
    assert_eq!(
        large_bash_json.get("truncated"),
        Some(&serde_json::json!(true))
    );
    let artifact_relative = large_bash.artifacts[0]
        .path
        .strip_prefix("artifacts/")
        .expect("artifact path prefix");
    let spilled_output = fs::read_to_string(run.artifacts_dir.join(artifact_relative))
        .expect("read spilled large bash output");
    assert_eq!(spilled_output.len(), 70_000);

    let edited = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "edit",
            serde_json::json!({
                "filePath": "written.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", compute_line_hash("hello from surface")),
                        "lines": ["hello from edit"],
                    }
                ],
            }),
        )
        .await
        .expect("edit tool");
    assert!(edited.display_text.contains("Edit applied successfully"));

    let reread_after_edit = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "read",
            serde_json::json!({
                "filePath": "written.txt"
            }),
        )
        .await
        .expect("reread after edit");
    assert!(reread_after_edit.display_text.contains("|hello from edit"));

    let patched = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "edit",
            serde_json::json!({
                "filePath": "written.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", compute_line_hash("hello from edit")),
                        "lines": ["hello from patch"],
                    }
                ],
            }),
        )
        .await
        .expect("second edit tool");
    assert!(patched.display_text.contains("Edit applied successfully"));

    let reread = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "read",
            serde_json::json!({ "filePath": "written.txt" }),
        )
        .await
        .expect("reread tool");
    assert!(reread.display_text.contains("hello from patch"));

    let todos = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "todowrite",
            serde_json::json!({
                "todos": [
                    {"content": "one", "status": "pending", "priority": "high"}
                ]
            }),
        )
        .await
        .expect("todowrite tool");
    assert!(todos.display_text.contains("pending"));

    let todo_read = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "todoread",
            serde_json::json!({}),
        )
        .await
        .expect("todoread tool");
    assert!(todo_read.display_text.contains("one"));

    let question_handle = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor(&worker_id),
                    Some(SURFACE_LIVE_PROFILE.to_string()),
                    "question",
                    serde_json::json!({
                        "questions": [
                            {
                                "question": "Pick one",
                                "header": "Choice",
                                "options": [{"label": "A", "description": "Option A"}]
                            }
                        ]
                    }),
                )
                .await
        })
    };
    let question_permission_id = wait_for_question_permission(&run.events_path).await;
    handle
        .resolve_permission(
            question_permission_id,
            PermissionDecision::Allow,
            Some("[[\"A\"]]".to_string()),
        )
        .await
        .expect("resolve question permission");
    let question = question_handle
        .await
        .expect("question task join")
        .expect("question tool");
    assert!(question.display_text.contains("\"Pick one\"=\"A\""));

    let invalid = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "invalid",
            serde_json::json!({ "tool": "missing_tool", "error": "bad args" }),
        )
        .await
        .expect("invalid tool");
    assert!(invalid.display_text.contains("bad args"));

    let lsp = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "lsp",
            serde_json::json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("lsp tool");
    assert!(!lsp.display_text.trim().is_empty());

    let batch = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "batch",
            serde_json::json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "written.txt"}},
                    {"tool": "grep", "parameters": {"pattern": "patch", "path": "."}}
                ]
            }),
        )
        .await
        .expect("batch tool");
    assert!(batch
        .display_text
        .contains("All 2 tools executed successfully"));

    let task = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "task",
            serde_json::json!({
                "category": SURFACE_LIVE_PROFILE,
                "description": "Background subtask",
                "prompt": "Say hello",
                "run_in_background": true,
                "load_skills": [],
            }),
        )
        .await
        .expect("task tool");
    assert!(task.display_text.contains("Background task scheduled"));

    let fetch_url = spawn_test_http_server();
    let fetched = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "webfetch",
            serde_json::json!({
                "url": fetch_url,
                "format": "text",
                "timeout": 10,
            }),
        )
        .await
        .expect("webfetch tool");
    assert!(fetched.display_text.contains("hello fetch"));

    let websearch = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "websearch",
            serde_json::json!({
                "query": "tokio rust runtime",
                "numResults": 1,
                "type": "fast",
            }),
        )
        .await
        .expect("websearch tool");
    assert!(websearch.display_text.to_lowercase().contains("tokio"));

    let codesearch = handle
        .execute_agent_tool_call(
            actor(&worker_id),
            Some(SURFACE_LIVE_PROFILE.to_string()),
            "codesearch",
            serde_json::json!({
                "query": "Tokio JoinSet rust example",
                "tokensNum": 1500,
            }),
        )
        .await
        .expect("codesearch tool");
    assert!(!codesearch.display_text.trim().is_empty());

    handle.stop_run().await.expect("stop run");
}
