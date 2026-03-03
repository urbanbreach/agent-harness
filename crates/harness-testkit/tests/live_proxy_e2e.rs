use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_responses_smoke() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("configs").join("harness.example.jsonc"));
    assert!(
        config_path.exists(),
        "live proxy config not found at {}",
        config_path.display()
    );
    assert_config_mentions_responses_mode(&config_path);

    let harness_bin = resolve_harness_bin();
    let events_path = unique_temp_file("live-proxy-events", "jsonl");

    let output = Command::new(&harness_bin)
        .arg("prompt")
        .arg("--text")
        .arg("Say hello")
        .arg("--config")
        .arg(&config_path)
        .arg("--out")
        .arg(&events_path)
        .current_dir(&repo_root)
        .output()
        .expect("spawn harness prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "harness prompt failed with status {:?}\nstdout:\n{}\nstderr:\n{}\nHint: ensure CLIproxyAPI is running locally and config points to /v1 with api_mode=responses",
        output.status.code(),
        stdout,
        stderr
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed to read event log {}: {err}", events_path.display()));

    let mut request_id: Option<String> = None;
    let mut saw_started = false;
    let mut saw_finished = false;
    let mut delta_count = 0usize;

    for (idx, line) in events_body.lines().enumerate() {
        let event: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|err| {
            panic!("events line {} is invalid json: {err}", idx + 1);
        });

        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        match event_type {
            "provider_request_started" => {
                if request_id.is_none() {
                    request_id = data
                        .get("request_id")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if request_id.is_some() {
                    saw_started = true;
                }
            }
            "provider_stream_delta" => {
                if same_request(&request_id, &data) {
                    delta_count += 1;
                }
            }
            "provider_request_finished" => {
                if same_request(&request_id, &data) {
                    saw_finished = true;
                    break;
                }
            }
            _ => {}
        }
    }

    assert!(saw_started, "expected provider_request_started event");
    assert!(
        delta_count > 0,
        "expected at least one provider_stream_delta event for the request"
    );
    assert!(saw_finished, "expected provider_request_finished event");
}

fn same_request(request_id: &Option<String>, data: &serde_json::Value) -> bool {
    let Some(expected) = request_id else {
        return false;
    };
    data.get("request_id")
        .and_then(serde_json::Value::as_str)
        .map(|current| current == expected)
        .unwrap_or(false)
}

fn assert_config_mentions_responses_mode(config_path: &Path) {
    let raw = fs::read_to_string(config_path)
        .unwrap_or_else(|err| panic!("failed to read config {}: {err}", config_path.display()));

    let mentions_responses = raw.contains("api_mode: \"responses\"")
        || raw.contains("\"api_mode\": \"responses\"")
        || raw.contains("apiMode: \"responses\"")
        || raw.contains("\"apiMode\": \"responses\"");

    assert!(
        mentions_responses,
        "live proxy config must set api_mode to responses: {}",
        config_path.display()
    );
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

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
