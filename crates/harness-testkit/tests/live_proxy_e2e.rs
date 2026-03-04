use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEFAULT_LIVE_PROXY_PROVIDER: &str = "default";
const DEFAULT_LIVE_PROXY_PROFILE: &str = "live_proxy_smoke";
const DEFAULT_LIVE_PROXY_PROMPT: &str = "Say hello in exactly five words.";
const DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS: &str = "120000";

#[derive(Debug, Clone)]
struct PromptRunConfig {
    config_path: PathBuf,
    profile: String,
    model_id: String,
}

#[derive(Debug, Default)]
struct ProviderTurnEvidence {
    request_id: Option<String>,
    saw_started: bool,
    saw_finished: bool,
    delta_count: usize,
    task_completed_summary: Option<String>,
    run_failed: Option<String>,
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_responses_smoke() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let source_config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("configs").join("harness.example.jsonc"));
    assert!(
        source_config_path.exists(),
        "live proxy config not found at {}",
        source_config_path.display()
    );

    let run_config = prepare_live_prompt_run_config(&source_config_path)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy prompt config: {err}"));

    let harness_bin = resolve_harness_bin();
    let events_path = unique_temp_file("live-proxy-events", "jsonl");
    let prompt_text =
        env::var("HARNESS_LIVE_PROXY_PROMPT").unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROMPT.into());
    let wait_timeout_ms = env::var("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS")
        .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string());

    let output = Command::new(&harness_bin)
        .arg("prompt")
        .arg("--text")
        .arg(&prompt_text)
        .arg("--profile")
        .arg(&run_config.profile)
        .arg("--config")
        .arg(&run_config.config_path)
        .arg("--out")
        .arg(&events_path)
        .env("HARNESS_PROMPT_WAIT_TIMEOUT_MS", &wait_timeout_ms)
        .current_dir(&repo_root)
        .output()
        .expect("spawn harness prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "harness prompt failed with status {:?}\nstdout:\n{}\nstderr:\n{}\nPrepared config: {}\nSelected profile: {}\nSelected model: {}\nHint: ensure CLIproxyAPI is running and reachable, HARNESS_LIVE_PROXY_MODEL (if set) is valid, and provider api_mode is responses or auto",
        output.status.code(),
        stdout,
        stderr,
        run_config.config_path.display(),
        run_config.profile,
        run_config.model_id
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed to read event log {}: {err}", events_path.display()));
    assert_events_show_successful_provider_turn(&events_body);
}

#[tokio::test(flavor = "current_thread")]
async fn live_proxy_prompt_wiremock_smoke_uses_responses_and_model_override() {
    let server = MockServer::start().await;
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(deterministic_responses_sse_fixture(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(response_template.clone())
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let provider_name = "proxy";
    let configured_model = "configured-model";
    let overridden_model = "wiremock-model-override";
    let session_dir = unique_temp_dir("live-proxy-wiremock-session");
    let source_config_path = unique_temp_file("live-proxy-wiremock-config", "jsonc");
    let source_config = build_live_proxy_test_config(
        provider_name,
        &server.uri(),
        "auto",
        configured_model,
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize wiremock config"),
    )
    .expect("write wiremock config");

    let run_config = prepare_prompt_run_config(
        &source_config_path,
        provider_name,
        Some(overridden_model),
        "wiremock_live_profile",
    )
    .expect("prepare prompt run config");

    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();
    let events_path = unique_temp_file("live-proxy-wiremock-events", "jsonl");

    let harness_bin_for_run = harness_bin.clone();
    let repo_root_for_run = repo_root.clone();
    let events_path_for_run = events_path.clone();
    let run_config_for_run = run_config.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&harness_bin_for_run)
            .arg("prompt")
            .arg("--text")
            .arg("Return hello from wiremock")
            .arg("--profile")
            .arg(&run_config_for_run.profile)
            .arg("--config")
            .arg(&run_config_for_run.config_path)
            .arg("--out")
            .arg(&events_path_for_run)
            .env(
                "HARNESS_PROMPT_WAIT_TIMEOUT_MS",
                DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS,
            )
            .current_dir(&repo_root_for_run)
            .output()
            .expect("spawn harness prompt")
    })
    .await
    .expect("join blocking harness run");

    assert!(
        output.status.success(),
        "wiremock harness prompt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", events_path.display()));
    assert_events_show_successful_provider_turn(&events_body);

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    let responses_request = requests
        .iter()
        .find(|request| request.url.path() == "/v1/responses")
        .expect("expected at least one /v1/responses request");
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path() == "/v1/chat/completions"),
        "did not expect /v1/chat/completions fallback"
    );

    let request_body: Value = responses_request
        .body_json()
        .expect("responses request body must be JSON");
    assert_eq!(
        request_body.get("model"),
        Some(&Value::String(overridden_model.to_string()))
    );
}

#[test]
fn prepare_prompt_run_config_rejects_chat_completions_mode() {
    let source_config_path = unique_temp_file("live-proxy-chat-mode", "jsonc");
    let session_dir = unique_temp_dir("live-proxy-chat-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "chat_completions",
        "chat-model",
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize chat mode config"),
    )
    .expect("write chat mode config");

    let err = prepare_prompt_run_config(
        &source_config_path,
        "default",
        Some("chat-model"),
        "chat_profile",
    )
    .expect_err("chat_completions mode should be rejected for live CLI proxy test");

    assert!(
        err.contains("responses or auto"),
        "unexpected error message: {err}"
    );
}

fn prepare_live_prompt_run_config(source_config_path: &Path) -> Result<PromptRunConfig, String> {
    let provider_name = env::var("HARNESS_LIVE_PROXY_PROVIDER")
        .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROVIDER.into());
    let model_override = env::var("HARNESS_LIVE_PROXY_MODEL").ok();
    let profile_name = env::var("HARNESS_LIVE_PROXY_PROFILE")
        .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROFILE.into());

    prepare_prompt_run_config(
        source_config_path,
        &provider_name,
        model_override.as_deref(),
        &profile_name,
    )
}

fn prepare_prompt_run_config(
    source_config_path: &Path,
    provider_name: &str,
    model_override: Option<&str>,
    profile_name: &str,
) -> Result<PromptRunConfig, String> {
    if provider_name.trim().is_empty() {
        return Err("provider name cannot be empty".to_string());
    }
    if profile_name.trim().is_empty() {
        return Err("profile name cannot be empty".to_string());
    }

    let mut config = load_json5_config(source_config_path)?;

    let provider = provider_from_config(&config, provider_name)?;
    let api_mode = provider_api_mode(provider);
    ensure_provider_uses_responses_compatible_mode(&api_mode)?;

    let selected_model = if let Some(model) = model_override {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            first_model_from_provider(provider)?
        } else {
            trimmed.to_string()
        }
    } else {
        first_model_from_provider(provider)?
    };

    rewrite_selected_provider_to_default(&mut config, provider_name)?;
    normalize_category_model_refs_to_default(&mut config)?;
    ensure_profile_model_ref(&mut config, profile_name, &selected_model)?;

    let prepared_config_path = unique_temp_file("live-proxy-prepared-config", "jsonc");
    let rendered = serde_json::to_string_pretty(&config)
        .map_err(|err| format!("failed to render prepared config JSON: {err}"))?;
    fs::write(&prepared_config_path, rendered).map_err(|err| {
        format!(
            "failed to write prepared config {}: {err}",
            prepared_config_path.display()
        )
    })?;

    Ok(PromptRunConfig {
        config_path: prepared_config_path,
        profile: profile_name.to_string(),
        model_id: selected_model,
    })
}

fn assert_events_show_successful_provider_turn(events_body: &str) {
    let evidence = collect_provider_turn_evidence(events_body);

    assert!(
        evidence.run_failed.is_none(),
        "run failed before provider completion: {}",
        evidence
            .run_failed
            .unwrap_or_else(|| "unknown run failure".to_string())
    );
    assert!(
        evidence.saw_started,
        "expected provider_request_started event"
    );
    assert!(
        evidence.saw_finished,
        "expected provider_request_finished event"
    );

    let has_task_summary = evidence
        .task_completed_summary
        .as_deref()
        .map(str::trim)
        .map(|text| !text.is_empty())
        .unwrap_or(false);

    assert!(
        evidence.delta_count > 0 || has_task_summary,
        "expected either provider_stream_delta events or a non-empty task_completed summary for the provider request"
    );
}

fn collect_provider_turn_evidence(events_body: &str) -> ProviderTurnEvidence {
    let mut evidence = ProviderTurnEvidence::default();

    for (idx, line) in events_body.lines().enumerate() {
        let event: Value = serde_json::from_str(line).unwrap_or_else(|err| {
            panic!("events line {} is invalid JSON: {err}", idx + 1);
        });

        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);

        match event_type {
            "provider_request_started" => {
                if evidence.request_id.is_none() {
                    evidence.request_id = data
                        .get("request_id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if evidence.request_id.is_some() {
                    evidence.saw_started = true;
                }
            }
            "provider_stream_delta" => {
                if same_request(&evidence.request_id, &data) {
                    evidence.delta_count += 1;
                }
            }
            "provider_request_finished" => {
                if same_request(&evidence.request_id, &data) {
                    evidence.saw_finished = true;
                }
            }
            "task_completed" => {
                let Some(request_id) = evidence.request_id.as_deref() else {
                    continue;
                };

                let is_matching_request = data
                    .get("task_id")
                    .and_then(Value::as_str)
                    .map(|task_id| task_id == request_id)
                    .unwrap_or(false);

                if is_matching_request {
                    evidence.task_completed_summary = data
                        .get("result_summary")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
            }
            "run_failed" => {
                evidence.run_failed = data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| Some("run_failed event missing error detail".to_string()));
            }
            _ => {}
        }
    }

    evidence
}

fn load_json5_config(config_path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read config {}: {err}", config_path.display()))?;
    json5::from_str(&raw).map_err(|err| {
        format!(
            "failed to parse JSON5 config {}: {err}",
            config_path.display()
        )
    })
}

fn provider_from_config<'a>(config: &'a Value, provider_name: &str) -> Result<&'a Value, String> {
    let providers = config
        .get("providers")
        .and_then(Value::as_object)
        .ok_or_else(|| "config must define providers as an object".to_string())?;

    providers
        .get(provider_name)
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))
}

fn provider_api_mode(provider: &Value) -> String {
    provider
        .get("api_mode")
        .or_else(|| provider.get("apiMode"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .trim()
        .to_ascii_lowercase()
}

fn ensure_provider_uses_responses_compatible_mode(api_mode: &str) -> Result<(), String> {
    match api_mode {
        "responses" | "auto" => Ok(()),
        "chat_completions" => Err(
            "live CLI proxy E2E requires provider api_mode set to responses or auto; found chat_completions"
                .to_string(),
        ),
        other => Err(format!(
            "unsupported api_mode `{other}` for live CLI proxy E2E; expected responses or auto"
        )),
    }
}

fn first_model_from_provider(provider: &Value) -> Result<String, String> {
    let Some(models) = provider.get("models").and_then(Value::as_object) else {
        return Err(
            "provider config has no `models` object; set HARNESS_LIVE_PROXY_MODEL explicitly"
                .to_string(),
        );
    };

    models.keys().next().cloned().ok_or_else(|| {
        "provider config has an empty `models` map; set HARNESS_LIVE_PROXY_MODEL".to_string()
    })
}

fn rewrite_selected_provider_to_default(
    config: &mut Value,
    provider_name: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;

    let selected_provider = providers
        .get(provider_name)
        .cloned()
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))?;

    providers.insert(DEFAULT_LIVE_PROXY_PROVIDER.to_string(), selected_provider);
    Ok(())
}

fn normalize_category_model_refs_to_default(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("categories")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.categories must be an object".to_string())?;

    for (category_name, category_value) in categories.iter_mut() {
        let Some(category_obj) = category_value.as_object_mut() else {
            return Err(format!("category `{category_name}` must be an object"));
        };

        let model_ref = category_obj
            .get("model_ref")
            .or_else(|| category_obj.get("modelRef"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if model_ref.is_empty() {
            continue;
        }

        let model_id = model_ref
            .split_once(':')
            .map(|(_, model_id)| model_id)
            .unwrap_or(model_ref)
            .trim();
        if model_id.is_empty() {
            continue;
        }

        category_obj.insert(
            "model_ref".to_string(),
            Value::String(format!("default:{model_id}")),
        );
    }

    Ok(())
}

fn ensure_profile_model_ref(
    config: &mut Value,
    profile_name: &str,
    model_id: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;

    let categories = root
        .entry("categories".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.categories must be an object".to_string())?;

    let mut profile = categories.get(profile_name).cloned().unwrap_or_else(|| {
        json!({
            "description": "Live proxy smoke profile",
            "tools": []
        })
    });

    let profile_obj = profile
        .as_object_mut()
        .ok_or_else(|| format!("category `{profile_name}` must be an object"))?;
    profile_obj.insert(
        "model_ref".to_string(),
        Value::String(format!("default:{model_id}")),
    );
    profile_obj
        .entry("description".to_string())
        .or_insert_with(|| Value::String("Live proxy smoke profile".to_string()));
    profile_obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));

    categories.insert(profile_name.to_string(), profile);
    Ok(())
}

fn build_live_proxy_test_config(
    provider_name: &str,
    provider_base_uri: &str,
    api_mode: &str,
    configured_model: &str,
    session_dir: &Path,
) -> Value {
    let mut providers = serde_json::Map::new();
    providers.insert(
        provider_name.to_string(),
        json!({
            "type": "openai_compatible",
            "base_url": format!("{provider_base_uri}/v1"),
            "api_key": "test-key",
            "api_mode": api_mode,
            "timeout_ms": 60000,
            "models": {
                configured_model: {
                    "display_name": "Configured model"
                }
            }
        }),
    );

    let mut categories = serde_json::Map::new();
    categories.insert(
        "deep".to_string(),
        json!({
            "description": "Deep profile",
            "model_ref": format!("{provider_name}:{configured_model}"),
            "tools": []
        }),
    );

    json!({
        "backgroundTask": {
            "defaultConcurrency": 2,
            "providerConcurrency": 2,
            "modelConcurrency": 2,
            "staleTimeoutMs": 30000,
            "messageStalenessTimeoutMs": 10000
        },
        "providers": providers,
        "categories": categories,
        "permissions": {
            "edit": "allow",
            "shell": "allow",
            "network": "allow"
        },
        "paths": {
            "session_dir": session_dir.display().to_string()
        },
        "ui": {
            "default_profile": "deep"
        }
    })
}

fn deterministic_responses_sse_fixture() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn same_request(request_id: &Option<String>, data: &Value) -> bool {
    let Some(expected) = request_id else {
        return false;
    };
    data.get("request_id")
        .and_then(Value::as_str)
        .map(|current| current == expected)
        .unwrap_or(false)
}

fn resolve_harness_bin() -> PathBuf {
    if let Ok(path) = env::var("HARNESS_BIN") {
        let harness_bin = PathBuf::from(path);
        assert!(
            harness_bin.exists(),
            "HARNESS_BIN points to missing path: {}",
            harness_bin.display()
        );
        return harness_bin;
    }

    let repo = repo_root();
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("harness")
        .current_dir(&repo)
        .status()
        .expect("spawn cargo build -p harness");
    assert!(
        status.success(),
        "cargo build -p harness failed with status {status}"
    );

    let harness_bin = repo
        .join("target")
        .join("debug")
        .join(binary_name("harness"));
    assert!(
        harness_bin.exists(),
        "expected harness binary at {}",
        harness_bin.display()
    );
    harness_bin
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn unique_temp_file(prefix: &str, ext: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    base.join(format!("{prefix}-{}-{nanos}.{ext}", std::process::id()))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    let dir = base.join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir)
        .unwrap_or_else(|err| panic!("failed creating temp dir {}: {err}", dir.display()));
    dir
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
