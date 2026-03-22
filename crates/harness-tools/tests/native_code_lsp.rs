use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use harness_core::clock::RealClock;
use harness_core::config::ShellAllowlist;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolError};
use harness_tools::coordinator_registry;
use serde_json::json;

fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: "run-native-code-lsp-tests".to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
        category: Some("deep".to_string()),
        plan_mode: false,
        plan_exit_target_profile: None,
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).expect("src");
    fs::create_dir_all(workspace.join("web")).expect("web");

    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"code_lsp_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write cargo manifest");
    fs::write(
        workspace.join("src/lib.rs"),
        "fn helper() {}\n\nfn caller() {\n    helper();\n}\n",
    )
    .expect("write rust fixture");
    fs::write(
        workspace.join("package.json"),
        "{\"name\":\"code-lsp-test\"}\n",
    )
    .expect("write package manifest");
    fs::write(
        workspace.join("web/app.ts"),
        "const answer = 42;\nexport function read() {\n  return answer;\n}\n",
    )
    .expect("write typescript fixture");
    fs::write(workspace.join("unsupported.py"), "print('unsupported')\n")
        .expect("write unsupported fixture");

    temp_dir
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct PathEnvGuard {
    previous: Option<String>,
}

impl PathEnvGuard {
    fn prepend(directory: &Path) -> Self {
        let previous = std::env::var("PATH").ok();
        let mut next = directory.display().to_string();
        if let Some(existing) = &previous {
            next.push(':');
            next.push_str(existing);
        }
        std::env::set_var("PATH", next);
        Self { previous }
    }
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn install_fake_lsp_binary(directory: &Path, name: &str, result_uri: &str) {
    let script = r#"#!/usr/bin/env python3
import json
import sys

RESULT_URI = "__RESULT_URI__"


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    if length <= 0:
        return None
    body = sys.stdin.buffer.read(length)
    if not body:
        return None
    return json.loads(body.decode("utf-8"))


def send(payload):
    data = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(data)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()


while True:
    message = read_message()
    if message is None:
        break

    method = message.get("method")
    message_id = message.get("id")

    if method == "initialize" and message_id is not None:
        send({"jsonrpc": "2.0", "id": message_id, "result": {"capabilities": {}}})
        continue

    if message_id is None:
        continue

    if method == "textDocument/hover":
        send({"jsonrpc": "2.0", "id": message_id, "result": {"contents": "stub hover"}})
        break

    params = message.get("params", {})
    position = params.get("position", {})
    line = position.get("line", 0)
    character = position.get("character", 0)
    send(
        {
            "jsonrpc": "2.0",
            "id": message_id,
            "result": [
                {
                    "uri": RESULT_URI,
                    "range": {
                        "start": {"line": line, "character": character},
                        "end": {"line": line, "character": character},
                    },
                }
            ],
        }
    )
    break
"#
    .replace("__RESULT_URI__", result_uri);
    let path = directory.join(name);
    fs::write(&path, script).expect("write fake lsp binary");
    let mut permissions = fs::metadata(&path)
        .expect("fake lsp metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("set fake lsp permissions");
}

fn expect_invalid_arguments(error: ToolError, expected_fragment: &str) {
    match error {
        ToolError::InvalidArguments(message) => assert!(
            message.contains(expected_fragment),
            "expected invalid-arguments error containing {expected_fragment:?}, got {message:?}"
        ),
        other => panic!("expected invalid-arguments error, got {other:?}"),
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide PATH mutation across awaits"
)]
async fn native_code_lsp_supports_rust_and_typescript_requests() {
    let _env_guard = env_lock().lock().expect("env lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "rust-analyzer",
        "file:///fake/rust-definition.rs",
    );
    install_fake_lsp_binary(
        &fake_bin,
        "typescript-language-server",
        "file:///fake/typescript-definition.ts",
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);

    let registry = coordinator_registry(ShellAllowlist::default());
    let native = registry.get("code.lsp").expect("code.lsp tool");
    let compat = registry.get("lsp").expect("lsp alias tool");

    let rust_result = native
        .call(
            test_context(&workspace, "native-rust"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("native rust lsp request");
    assert!(rust_result.display_text.contains("rust-definition.rs"));
    let rust_json = rust_result
        .structured_json
        .clone()
        .expect("native rust structured json");
    assert_eq!(rust_json["operation"], json!("goToDefinition"));
    assert_eq!(rust_json["line"], json!(4));
    assert_eq!(rust_json["character"], json!(6));
    assert!(rust_json["filePath"]
        .as_str()
        .expect("native rust file path")
        .ends_with("src/lib.rs"));

    let ts_result = native
        .call(
            test_context(&workspace, "native-ts"),
            json!({
                "operation": "goToDefinition",
                "filePath": "web/app.ts",
                "line": 2,
                "character": 9,
            }),
        )
        .await
        .expect("native typescript lsp request");
    assert!(ts_result.display_text.contains("typescript-definition.ts"));
    let ts_json = ts_result
        .structured_json
        .expect("native ts structured json");
    assert_eq!(ts_json["operation"], json!("goToDefinition"));
    assert_eq!(ts_json["line"], json!(2));
    assert_eq!(ts_json["character"], json!(9));
    assert!(ts_json["filePath"]
        .as_str()
        .expect("native ts file path")
        .ends_with("web/app.ts"));

    let compat_rust = compat
        .call(
            test_context(&workspace, "compat-rust"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("compat rust lsp request");
    assert_eq!(compat_rust.display_text, rust_result.display_text);
    assert_eq!(compat_rust.structured_json, rust_result.structured_json);
}

#[tokio::test]
async fn native_code_lsp_rejects_unsupported_language_and_operation_cleanly() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let native = registry.get("code.lsp").expect("code.lsp tool");
    let compat = registry.get("lsp").expect("lsp alias tool");

    let unsupported_language = native
        .call(
            test_context(&workspace, "unsupported-language"),
            json!({
                "operation": "goToDefinition",
                "filePath": "unsupported.py",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("unsupported language should fail");
    expect_invalid_arguments(
        unsupported_language,
        "unsupported code.lsp language extension: .py",
    );

    let unsupported_operation = native
        .call(
            test_context(&workspace, "unsupported-operation"),
            json!({
                "operation": "renameSymbol",
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("unsupported operation should fail");
    expect_invalid_arguments(
        unsupported_operation,
        "unsupported code.lsp operation: renameSymbol",
    );

    let compat_unsupported_operation = compat
        .call(
            test_context(&workspace, "compat-unsupported-operation"),
            json!({
                "operation": "renameSymbol",
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("compat unsupported operation should fail");
    expect_invalid_arguments(
        compat_unsupported_operation,
        "unsupported code.lsp operation: renameSymbol",
    );
}
