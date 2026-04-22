use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use harness_core::clock::RealClock;
use harness_core::config::{
    registered_lsp_config, set_registered_lsp_config, LspConfig, LspServerConfig, ShellAllowlist,
};
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
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).expect("src");
    fs::create_dir_all(workspace.join("web")).expect("web");
    fs::create_dir_all(workspace.join("python")).expect("python");
    fs::create_dir_all(workspace.join("go")).expect("go");
    fs::create_dir_all(workspace.join("config")).expect("config");
    fs::create_dir_all(workspace.join("custom")).expect("custom");

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
    fs::write(workspace.join("src/extra.rs"), "pub fn extra() {}\n")
        .expect("write extra rust fixture");
    fs::write(
        workspace.join("src/other.rs"),
        "fn another() {\n    helper();\n}\n",
    )
    .expect("write secondary rust fixture");
    fs::write(
        workspace.join("package.json"),
        "{\"name\":\"code-lsp-test\"}\n",
    )
    .expect("write package manifest");
    fs::write(
        workspace.join("pyproject.toml"),
        "[project]\nname = \"code-lsp-test\"\nversion = \"0.1.0\"\n",
    )
    .expect("write python manifest");
    fs::write(
        workspace.join("go.mod"),
        "module example.com/code-lsp-test\n",
    )
    .expect("write go module");
    fs::write(
        workspace.join("web/app.ts"),
        "const answer = 42;\nexport function read() {\n  return answer;\n}\n",
    )
    .expect("write typescript fixture");
    fs::write(
        workspace.join("python/app.py"),
        "ANSWER = 42\n\ndef read() -> int:\n    return ANSWER\n",
    )
    .expect("write python fixture");
    fs::write(
        workspace.join("go/main.go"),
        "package main\n\nfunc helper() {}\n\nfunc main() {\n\thelper()\n}\n",
    )
    .expect("write go fixture");
    fs::write(
        workspace.join("config/settings.json"),
        "{\n  \"answer\": 42\n}\n",
    )
    .expect("write json fixture");
    fs::write(workspace.join("config/service.yaml"), "answer: 42\n").expect("write yaml fixture");
    fs::write(
        workspace.join("custom/schema.foo"),
        "thing Example\n\nreference Example\n",
    )
    .expect("write custom fixture");
    fs::write(workspace.join("unsupported.lua"), "print('unsupported')\n")
        .expect("write unsupported fixture");

    temp_dir
}

fn test_lock() -> &'static Mutex<()> {
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

struct LspConfigGuard {
    previous: LspConfig,
}

impl LspConfigGuard {
    fn install(config: LspConfig) -> Self {
        let previous = registered_lsp_config();
        set_registered_lsp_config(config);
        Self { previous }
    }
}

impl Drop for LspConfigGuard {
    fn drop(&mut self) {
        set_registered_lsp_config(self.previous.clone());
    }
}

fn install_empty_definition_lsp_binary(directory: &Path, name: &str, counter_path: &Path) {
    let script = r#"#!/usr/bin/env python3
import json
import pathlib
import sys

COUNTER_PATH = pathlib.Path(__COUNTER_PATH__)


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode("utf-8").split(":", 1)
        headers[key.strip().lower()] = value.strip()
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

    if method == "textDocument/didOpen":
        continue

    if method == "textDocument/definition" and message_id is not None:
        count = 0
        if COUNTER_PATH.exists():
            count = int(COUNTER_PATH.read_text())
        COUNTER_PATH.write_text(str(count + 1))
        send({"jsonrpc": "2.0", "id": message_id, "result": []})
        break

    if message_id is not None:
        send({"jsonrpc": "2.0", "id": message_id, "result": []})
"#
    .replace(
        "__COUNTER_PATH__",
        &serde_json::to_string(&counter_path.display().to_string()).expect("counter path json"),
    );
    let path = directory.join(name);
    fs::write(&path, script).expect("write empty definition lsp binary");
    let mut permissions = fs::metadata(&path)
        .expect("empty definition lsp metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("set empty definition lsp permissions");
}

struct FakeLspSpec<'a> {
    result_uri: &'a str,
    diagnostic_message: &'a str,
    empty_diagnostics: bool,
    env_key: &'a str,
    env_value: &'a str,
    expected_language_id: &'a str,
    initialization_path: &'a str,
    initialization_value: &'a str,
}

fn install_fake_lsp_binary(directory: &Path, name: &str, spec: &FakeLspSpec<'_>) {
    let script = r#"#!/usr/bin/env python3
import json
import os
import sys

RESULT_URI = __RESULT_URI__
DIAGNOSTIC_MESSAGE = __DIAGNOSTIC_MESSAGE__
EXPECT_ENV_KEY = __EXPECT_ENV_KEY__
EXPECT_ENV_VALUE = __EXPECT_ENV_VALUE__
EXPECT_LANGUAGE_ID = __EXPECT_LANGUAGE_ID__
EXPECT_INIT_PATH = __EXPECT_INIT_PATH__
EXPECT_INIT_VALUE = __EXPECT_INIT_VALUE__
EMPTY_DIAGNOSTICS = __EMPTY_DIAGNOSTICS__

opened_uri = None
opened_language_id = None
initialization_options = {}


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode("utf-8").split(":", 1)
        headers[key.strip().lower()] = value.strip()
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


def lookup(obj, path):
    cur = obj
    for part in path.split("."):
        if not part:
            continue
        if not isinstance(cur, dict):
            return None
        cur = cur.get(part)
    return cur


def diagnostics_payload(method, target_uri, env_ok, language_ok, init_ok):
    diagnostics = []
    if not EMPTY_DIAGNOSTICS:
        message = f"{DIAGNOSTIC_MESSAGE}; env_ok={env_ok}; init_ok={init_ok}"
        if EXPECT_LANGUAGE_ID != "":
            message = f"{DIAGNOSTIC_MESSAGE}; env_ok={env_ok}; lang_ok={language_ok}; init_ok={init_ok}"
        diagnostics = [{
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 3}
            },
            "severity": 1,
            "source": method,
            "message": message
        }]
    return {
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": target_uri,
            "diagnostics": diagnostics
        }
    }


while True:
    message = read_message()
    if message is None:
        break

    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params", {})

    if method == "textDocument/didOpen":
        opened_uri = params.get("textDocument", {}).get("uri")
        opened_language_id = params.get("textDocument", {}).get("languageId")
        env_ok = EXPECT_ENV_KEY == "" or os.environ.get(EXPECT_ENV_KEY) == EXPECT_ENV_VALUE
        language_ok = EXPECT_LANGUAGE_ID == "" or opened_language_id == EXPECT_LANGUAGE_ID
        init_value = lookup(initialization_options, EXPECT_INIT_PATH)
        init_ok = EXPECT_INIT_PATH == "" or str(init_value).lower() == EXPECT_INIT_VALUE.lower()
        send(diagnostics_payload(method, opened_uri or "file:///workspace/unknown", env_ok, language_ok, init_ok))
        continue

    if method == "initialize" and message_id is not None:
        initialization_options = params.get("initializationOptions", {})
        send({"jsonrpc": "2.0", "id": message_id, "result": {"capabilities": {}}})
        continue

    if message_id is None:
        continue

    env_ok = EXPECT_ENV_KEY == "" or os.environ.get(EXPECT_ENV_KEY) == EXPECT_ENV_VALUE
    language_ok = EXPECT_LANGUAGE_ID == "" or opened_language_id == EXPECT_LANGUAGE_ID
    init_value = lookup(initialization_options, EXPECT_INIT_PATH)
    init_ok = EXPECT_INIT_PATH == "" or str(init_value).lower() == EXPECT_INIT_VALUE.lower()
    target_uri = params.get("textDocument", {}).get("uri") or opened_uri or "file:///workspace/unknown"
    send(diagnostics_payload(method, target_uri, env_ok, language_ok, init_ok))
    position = params.get("position", {})
    line = position.get("line", 0)
    character = position.get("character", 0)
    query = params.get("query", "")
    send({
        "jsonrpc": "2.0",
        "id": message_id,
        "result": [{
            "uri": RESULT_URI,
            "range": {
                "start": {"line": line, "character": character},
                "end": {"line": line, "character": character}
            },
            "query": query
        }]
    })
    if method not in ("textDocument/diagnostic", "workspace/diagnostic"):
        break
"#
    .replace("__RESULT_URI__", &serde_json::to_string(spec.result_uri).expect("result uri json"))
    .replace(
        "__DIAGNOSTIC_MESSAGE__",
        &serde_json::to_string(spec.diagnostic_message).expect("diagnostic json"),
    )
    .replace("__EMPTY_DIAGNOSTICS__", if spec.empty_diagnostics { "True" } else { "False" })
    .replace(
        "__EXPECT_ENV_KEY__",
        &serde_json::to_string(spec.env_key).expect("env key json"),
    )
    .replace(
        "__EXPECT_ENV_VALUE__",
        &serde_json::to_string(spec.env_value).expect("env value json"),
    )
    .replace(
        "__EXPECT_LANGUAGE_ID__",
        &serde_json::to_string(spec.expected_language_id).expect("language id json"),
    )
    .replace(
        "__EXPECT_INIT_PATH__",
        &serde_json::to_string(spec.initialization_path).expect("init path json"),
    )
    .replace(
        "__EXPECT_INIT_VALUE__",
        &serde_json::to_string(spec.initialization_value).expect("init value json"),
    );
    let path = directory.join(name);
    fs::write(&path, script).expect("write fake lsp binary");
    let mut permissions = fs::metadata(&path)
        .expect("fake lsp metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("set fake lsp permissions");
}

fn install_fake_rename_lsp_binary(
    directory: &Path,
    name: &str,
    primary_uri: &str,
    secondary_uri: &str,
    prepare_mode: &str,
    rename_mode: &str,
) {
    let script = r#"#!/usr/bin/env python3
import json
import sys

PRIMARY_URI = __PRIMARY_URI__
SECONDARY_URI = __SECONDARY_URI__
PREPARE_MODE = __PREPARE_MODE__
RENAME_MODE = __RENAME_MODE__

opened_uri = None
prepare_ok = False
workspace_ok = False


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode("utf-8").split(":", 1)
        headers[key.strip().lower()] = value.strip()
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
    params = message.get("params", {})

    if method == "initialize" and message_id is not None:
        capabilities = params.get("capabilities", {})
        prepare_ok = capabilities.get("textDocument", {}).get("rename", {}).get("prepareSupport") is True
        resource_ops = capabilities.get("workspace", {}).get("workspaceEdit", {}).get("resourceOperations", [])
        workspace_ok = capabilities.get("workspace", {}).get("workspaceEdit", {}).get("documentChanges") is True and resource_ops == ["create", "rename", "delete"]
        send({"jsonrpc": "2.0", "id": message_id, "result": {"capabilities": {"renameProvider": {"prepareProvider": True}}}})
        continue

    if method == "textDocument/didOpen":
        opened_uri = params.get("textDocument", {}).get("uri")
        continue

    if message_id is None:
        continue

    if method == "textDocument/prepareRename":
        if PREPARE_MODE == "null":
            send({"jsonrpc": "2.0", "id": message_id, "result": None})
        elif PREPARE_MODE == "error":
            send({"jsonrpc": "2.0", "id": message_id, "error": {"code": -32601, "message": "prepare rename unsupported"}})
        else:
            send({
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "range": {
                        "start": {"line": 0, "character": 3},
                        "end": {"line": 0, "character": 9}
                    },
                    "placeholder": "helper"
                }
            })
        continue

    if method != "textDocument/rename":
        send({"jsonrpc": "2.0", "id": message_id, "result": None})
        continue

    target_uri = params.get("textDocument", {}).get("uri") or opened_uri or PRIMARY_URI
    send({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": target_uri,
            "diagnostics": [{
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 6}
                },
                "severity": 2,
                "source": method,
                "message": f"rename diagnostic; prepare_ok={prepare_ok}; workspace_ok={workspace_ok}"
            }]
        }
    })

    if RENAME_MODE == "null":
        send({"jsonrpc": "2.0", "id": message_id, "result": None})
    elif RENAME_MODE == "error":
        send({"jsonrpc": "2.0", "id": message_id, "error": {"code": -32602, "message": "rename unsupported"}})
    elif RENAME_MODE == "changes":
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {
                "changes": {
                    PRIMARY_URI: [{
                        "range": {
                            "start": {"line": 0, "character": 3},
                            "end": {"line": 0, "character": 9}
                        },
                        "newText": params.get("newName", "")
                    }],
                    SECONDARY_URI: [{
                        "range": {
                            "start": {"line": 1, "character": 4},
                            "end": {"line": 1, "character": 10}
                        },
                        "newText": params.get("newName", "")
                    }]
                }
            }
        })
    else:
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {
                "changeAnnotations": {
                    "rename": {
                        "label": "Semantic rename",
                        "description": "Rename helper across files",
                        "needsConfirmation": False
                    }
                },
                "documentChanges": [{
                    "textDocument": {
                        "uri": PRIMARY_URI,
                        "version": None
                    },
                    "edits": [{
                        "range": {
                            "start": {"line": 0, "character": 3},
                            "end": {"line": 0, "character": 9}
                        },
                        "newText": params.get("newName", ""),
                        "annotationId": "rename"
                    }]
                }, {
                    "textDocument": {
                        "uri": SECONDARY_URI,
                        "version": None
                    },
                    "edits": [{
                        "range": {
                            "start": {"line": 1, "character": 4},
                            "end": {"line": 1, "character": 10}
                        },
                        "newText": params.get("newName", ""),
                        "annotationId": "rename"
                    }]
                }]
            }
        })
"#
    .replace("__PRIMARY_URI__", &serde_json::to_string(primary_uri).expect("primary uri json"))
    .replace(
        "__SECONDARY_URI__",
        &serde_json::to_string(secondary_uri).expect("secondary uri json"),
    )
    .replace(
        "__PREPARE_MODE__",
        &serde_json::to_string(prepare_mode).expect("prepare mode json"),
    )
    .replace(
        "__RENAME_MODE__",
        &serde_json::to_string(rename_mode).expect("rename mode json"),
    );
    let path = directory.join(name);
    fs::write(&path, script).expect("write fake rename lsp binary");
    let mut permissions = fs::metadata(&path)
        .expect("fake rename lsp metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("set fake rename lsp permissions");
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

fn first_diagnostic_message(value: &serde_json::Value) -> &str {
    value["diagnostics"][0]["diagnostics"][0]["message"]
        .as_str()
        .expect("diagnostic message")
}

fn file_uri(path: &Path) -> String {
    reqwest::Url::from_file_path(path)
        .expect("file url")
        .to_string()
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_configured_custom_servers() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-definition.rs",
            diagnostic_message: "rust override diagnostic",
            empty_diagnostics: false,
            env_key: "RUST_LOG",
            env_value: "trace",
            expected_language_id: "rust",
            initialization_path: "cargo.allFeatures",
            initialization_value: "true",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-ts-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/typescript-definition.ts",
            diagnostic_message: "typescript override diagnostic",
            empty_diagnostics: false,
            env_key: "TS_SERVER_LOG",
            env_value: "verbose",
            expected_language_id: "typescript",
            initialization_path: "preferences.importModuleSpecifierPreference",
            initialization_value: "relative",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-local-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/custom-definition.foo",
            diagnostic_message: "custom server diagnostic",
            empty_diagnostics: false,
            env_key: "CUSTOM_LSP_MODE",
            env_value: "enabled",
            expected_language_id: "foo",
            initialization_path: "feature.mode",
            initialization_value: "custom",
        },
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([
            (
                "rust".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-rust-analyzer".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("RUST_LOG".to_string(), "trace".to_string())]),
                    initialization: Some(json!({
                        "cargo": {
                            "allFeatures": true,
                        }
                    })),
                },
            ),
            (
                "typescript".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-ts-lsp".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("TS_SERVER_LOG".to_string(), "verbose".to_string())]),
                    initialization: Some(json!({
                        "preferences": {
                            "importModuleSpecifierPreference": "relative",
                        }
                    })),
                },
            ),
            (
                "custom-local".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-local-lsp".to_string()]),
                    extensions: Some(vec![".foo".to_string()]),
                    env: BTreeMap::from([("CUSTOM_LSP_MODE".to_string(), "enabled".to_string())]),
                    initialization: Some(json!({
                        "feature": {
                            "mode": "custom",
                        }
                    })),
                },
            ),
        ]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let rust_result = lsp
        .call(
            test_context(&workspace, "configured-rust"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("configured rust request");
    assert!(rust_result.display_text.contains("rust-definition.rs"));
    assert!(rust_result.display_text.contains("Diagnostics:"));
    assert!(rust_result
        .display_text
        .contains("rust override diagnostic"));
    let rust_json = rust_result
        .structured_json
        .clone()
        .expect("configured rust structured json");
    assert_eq!(rust_json["server"]["name"], json!("rust"));
    assert_eq!(
        rust_json["server"]["command"],
        json!(["custom-rust-analyzer"])
    );
    assert!(
        first_diagnostic_message(&rust_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let ts_result = lsp
        .call(
            test_context(&workspace, "configured-typescript"),
            json!({
                "operation": "goToDefinition",
                "filePath": "web/app.ts",
                "line": 2,
                "character": 9,
            }),
        )
        .await
        .expect("configured typescript request");
    assert!(ts_result
        .display_text
        .contains("typescript override diagnostic"));
    let ts_json = ts_result
        .structured_json
        .clone()
        .expect("configured typescript structured json");
    assert_eq!(ts_json["server"]["name"], json!("typescript"));
    assert_eq!(ts_json["server"]["command"], json!(["custom-ts-lsp"]));
    assert!(first_diagnostic_message(&ts_json).contains("env_ok=True; lang_ok=True; init_ok=True"));

    let custom_result = lsp
        .call(
            test_context(&workspace, "configured-custom"),
            json!({
                "operation": "goToDefinition",
                "filePath": "custom/schema.foo",
                "line": 3,
                "character": 2,
            }),
        )
        .await
        .expect("configured custom request");
    assert!(custom_result.display_text.contains("custom-definition.foo"));
    assert!(custom_result
        .display_text
        .contains("custom server diagnostic"));
    let custom_json = custom_result
        .structured_json
        .expect("configured custom structured json");
    assert_eq!(custom_json["server"]["name"], json!("custom-local"));
    assert_eq!(
        custom_json["server"]["command"],
        json!(["custom-local-lsp"])
    );
    assert!(custom_json["diagnostics"][0]["file_path"]
        .as_str()
        .expect("custom diagnostic file path")
        .ends_with("custom/schema.foo"));
    assert!(
        first_diagnostic_message(&custom_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_returns_empty_definition_without_retry_loop() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    let counter_path = temp_dir.path().join("definition-count.txt");
    install_empty_definition_lsp_binary(&bin_dir, "custom-rust-analyzer", &counter_path);
    let _path_guard = PathEnvGuard::prepend(&bin_dir);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["custom-rust-analyzer".to_string()]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");
    let result = lsp
        .call(
            test_context(&workspace, "empty-definition"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect("empty definition request should succeed");

    assert_eq!(result.display_text, "No results found for goToDefinition");
    assert_eq!(
        fs::read_to_string(&counter_path).expect("read counter"),
        "1"
    );
}

#[test]
fn native_code_lsp_supports_configured_builtin_and_custom_servers() {
    native_code_lsp_supports_configured_custom_servers();
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_additional_builtin_server_presets() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-python-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/python-definition.py",
            diagnostic_message: "python preset diagnostic",
            empty_diagnostics: false,
            env_key: "PYRIGHT_PYTHON_FORCE_VERSION",
            env_value: "latest",
            expected_language_id: "python",
            initialization_path: "python.analysis.typeCheckingMode",
            initialization_value: "basic",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-go-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/go-definition.go",
            diagnostic_message: "go preset diagnostic",
            empty_diagnostics: false,
            env_key: "GOPLS_LOG",
            env_value: "debug",
            expected_language_id: "go",
            initialization_path: "gopls.staticcheck",
            initialization_value: "true",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-json-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/json-definition.json",
            diagnostic_message: "json preset diagnostic",
            empty_diagnostics: false,
            env_key: "JSON_LSP_TRACE",
            env_value: "verbose",
            expected_language_id: "json",
            initialization_path: "json.validate.enable",
            initialization_value: "true",
        },
    );
    install_fake_lsp_binary(
        &fake_bin,
        "custom-yaml-lsp",
        &FakeLspSpec {
            result_uri: "file:///fake/yaml-definition.yaml",
            diagnostic_message: "yaml preset diagnostic",
            empty_diagnostics: false,
            env_key: "YAML_LSP_TRACE",
            env_value: "verbose",
            expected_language_id: "yaml",
            initialization_path: "yaml.keyOrdering",
            initialization_value: "false",
        },
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([
            (
                "python".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-python-lsp".to_string(), "--stdio".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([(
                        "PYRIGHT_PYTHON_FORCE_VERSION".to_string(),
                        "latest".to_string(),
                    )]),
                    initialization: Some(json!({
                        "python": {
                            "analysis": {
                                "typeCheckingMode": "basic",
                            }
                        }
                    })),
                },
            ),
            (
                "go".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-go-lsp".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("GOPLS_LOG".to_string(), "debug".to_string())]),
                    initialization: Some(json!({
                        "gopls": {
                            "staticcheck": true,
                        }
                    })),
                },
            ),
            (
                "json".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-json-lsp".to_string(), "--stdio".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("JSON_LSP_TRACE".to_string(), "verbose".to_string())]),
                    initialization: Some(json!({
                        "json": {
                            "validate": {
                                "enable": true,
                            }
                        }
                    })),
                },
            ),
            (
                "yaml".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["custom-yaml-lsp".to_string(), "--stdio".to_string()]),
                    extensions: None,
                    env: BTreeMap::from([("YAML_LSP_TRACE".to_string(), "verbose".to_string())]),
                    initialization: Some(json!({
                        "yaml": {
                            "keyOrdering": false,
                        }
                    })),
                },
            ),
        ]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let python_result = lsp
        .call(
            test_context(&workspace, "builtin-python"),
            json!({
                "operation": "goToDefinition",
                "filePath": "python/app.py",
                "line": 4,
                "character": 12,
            }),
        )
        .await
        .expect("python preset request");
    let python_json = python_result
        .structured_json
        .clone()
        .expect("python structured json");
    assert_eq!(python_json["server"]["name"], json!("python"));
    assert_eq!(
        python_json["server"]["command"],
        json!(["custom-python-lsp", "--stdio"])
    );
    assert!(
        first_diagnostic_message(&python_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let go_result = lsp
        .call(
            test_context(&workspace, "builtin-go"),
            json!({
                "operation": "goToDefinition",
                "filePath": "go/main.go",
                "line": 5,
                "character": 2,
            }),
        )
        .await
        .expect("go preset request");
    let go_json = go_result
        .structured_json
        .clone()
        .expect("go structured json");
    assert_eq!(go_json["server"]["name"], json!("go"));
    assert_eq!(go_json["server"]["command"], json!(["custom-go-lsp"]));
    assert!(first_diagnostic_message(&go_json).contains("env_ok=True; lang_ok=True; init_ok=True"));

    let json_result = lsp
        .call(
            test_context(&workspace, "builtin-json"),
            json!({
                "operation": "hover",
                "filePath": "config/settings.json",
                "line": 2,
                "character": 4,
            }),
        )
        .await
        .expect("json preset request");
    let json_json = json_result
        .structured_json
        .clone()
        .expect("json structured json");
    assert_eq!(json_json["server"]["name"], json!("json"));
    assert_eq!(
        json_json["server"]["command"],
        json!(["custom-json-lsp", "--stdio"])
    );
    assert!(
        first_diagnostic_message(&json_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let yaml_result = lsp
        .call(
            test_context(&workspace, "builtin-yaml"),
            json!({
                "operation": "documentSymbol",
                "filePath": "config/service.yaml",
            }),
        )
        .await
        .expect("yaml preset request");
    let yaml_json = yaml_result.structured_json.expect("yaml structured json");
    assert_eq!(yaml_json["server"]["name"], json!("yaml"));
    assert_eq!(
        yaml_json["server"]["command"],
        json!(["custom-yaml-lsp", "--stdio"])
    );
    assert!(
        first_diagnostic_message(&yaml_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_direct_file_and_workspace_diagnostics() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-definition.rs",
            diagnostic_message: "rust direct diagnostic",
            empty_diagnostics: false,
            env_key: "RUST_LOG",
            env_value: "trace",
            expected_language_id: "rust",
            initialization_path: "cargo.allFeatures",
            initialization_value: "true",
        },
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["custom-rust-analyzer".to_string()]),
                extensions: None,
                env: BTreeMap::from([("RUST_LOG".to_string(), "trace".to_string())]),
                initialization: Some(json!({
                    "cargo": {
                        "allFeatures": true,
                    }
                })),
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let file_result = lsp
        .call(
            test_context(&workspace, "direct-file-diagnostics"),
            json!({
                "operation": "fileDiagnostics",
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect("file diagnostics request with reusable cursor metadata");
    assert!(file_result.display_text.contains("Diagnostics for"));
    assert!(file_result
        .display_text
        .contains("src/lib.rs:1:1 Error rust direct diagnostic"));
    let file_json = file_result
        .structured_json
        .clone()
        .expect("file diagnostics structured json");
    assert_eq!(file_json["result"]["scope"], json!("file"));
    assert_eq!(file_json["result"]["diagnosticCount"], json!(1));
    assert!(file_json["diagnostics"][0]["file_path"]
        .as_str()
        .expect("file diagnostic path")
        .ends_with("src/lib.rs"));
    assert!(
        first_diagnostic_message(&file_json).contains("env_ok=True; lang_ok=True; init_ok=True")
    );

    let workspace_result = lsp
        .call(
            test_context(&workspace, "direct-workspace-diagnostics"),
            json!({
                "operation": "workspaceDiagnostics",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect("workspace diagnostics request");
    assert!(workspace_result.display_text.contains("Diagnostics for"));
    assert!(workspace_result
        .display_text
        .contains("src/lib.rs:1:1 Error rust direct diagnostic"));
    assert!(workspace_result
        .display_text
        .contains("src/extra.rs:1:1 Error rust direct diagnostic"));
    assert!(workspace_result
        .display_text
        .contains("src/other.rs:1:1 Error rust direct diagnostic"));
    let workspace_json = workspace_result
        .structured_json
        .expect("workspace diagnostics structured json");
    assert_eq!(workspace_json["result"]["scope"], json!("workspace"));
    assert_eq!(workspace_json["result"]["filesScanned"], json!(3));
    assert_eq!(workspace_json["result"]["diagnosticCount"], json!(3));
    assert_eq!(
        workspace_json["diagnostics"]
            .as_array()
            .expect("workspace diagnostics array")
            .len(),
        3
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_reports_empty_direct_diagnostics_cleanly() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-definition.rs",
            diagnostic_message: "unused because diagnostics are empty",
            empty_diagnostics: true,
            env_key: "",
            env_value: "",
            expected_language_id: "rust",
            initialization_path: "",
            initialization_value: "",
        },
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["custom-rust-analyzer".to_string()]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let result = lsp
        .call(
            test_context(&workspace, "empty-file-diagnostics"),
            json!({
                "operation": "fileDiagnostics",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect("empty file diagnostics request");
    let expected_path = workspace.join("src/lib.rs").display().to_string();
    assert_eq!(
        result.display_text,
        format!("No diagnostics found for {expected_path}")
    );
    let result_json = result
        .structured_json
        .expect("empty file diagnostics structured json");
    assert_eq!(result_json["result"]["scope"], json!("file"));
    assert_eq!(result_json["result"]["diagnosticCount"], json!(0));
    assert_eq!(result_json["diagnostics"][0]["diagnostics"], json!([]));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_rejects_disabled_or_unsupported_servers() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([
            (
                "rust".to_string(),
                LspServerConfig {
                    disabled: true,
                    command: None,
                    extensions: None,
                    env: BTreeMap::new(),
                    initialization: None,
                },
            ),
            (
                "custom-local".to_string(),
                LspServerConfig {
                    disabled: true,
                    command: Some(vec!["custom-local-lsp".to_string()]),
                    extensions: Some(vec![".foo".to_string()]),
                    env: BTreeMap::new(),
                    initialization: None,
                },
            ),
        ]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let disabled_rust = lsp
        .call(
            test_context(&workspace, "disabled-rust"),
            json!({
                "operation": "goToDefinition",
                "filePath": "src/lib.rs",
                "line": 4,
                "character": 6,
            }),
        )
        .await
        .expect_err("disabled rust should fail");
    expect_invalid_arguments(
        disabled_rust,
        "configured lsp server `rust` is disabled for extension .rs",
    );

    let disabled_custom = lsp
        .call(
            test_context(&workspace, "disabled-custom"),
            json!({
                "operation": "goToDefinition",
                "filePath": "custom/schema.foo",
                "line": 3,
                "character": 2,
            }),
        )
        .await
        .expect_err("disabled custom server should fail");
    expect_invalid_arguments(
        disabled_custom,
        "configured lsp server `custom-local` is disabled for extension .foo",
    );

    let unsupported_language = lsp
        .call(
            test_context(&workspace, "unsupported-language"),
            json!({
                "operation": "goToDefinition",
                "filePath": "unsupported.lua",
                "line": 1,
                "character": 1,
            }),
        )
        .await
        .expect_err("unsupported language should fail");
    expect_invalid_arguments(
        unsupported_language,
        "unsupported lsp language extension: .lua",
    );
}

#[test]
fn native_code_lsp_rejects_disabled_or_unsupported_server_requests() {
    native_code_lsp_rejects_disabled_or_unsupported_servers();
}

#[tokio::test]
async fn native_code_lsp_validates_inputs_by_operation_shape() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let missing_position = lsp
        .call(
            test_context(&workspace, "missing-position"),
            json!({
                "operation": "hover",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect_err("hover should require cursor coordinates");
    expect_invalid_arguments(missing_position, "missing field `line`");

    let missing_query = lsp
        .call(
            test_context(&workspace, "missing-query"),
            json!({
                "operation": "workspaceSymbol",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect_err("workspaceSymbol should require a query");
    expect_invalid_arguments(missing_query, "missing field `query`");
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_supports_non_position_operations_without_cursor_placeholders() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir");
    install_fake_lsp_binary(
        &fake_bin,
        "custom-rust-analyzer",
        &FakeLspSpec {
            result_uri: "file:///fake/rust-symbol.rs",
            diagnostic_message: "rust symbol diagnostic",
            empty_diagnostics: false,
            env_key: "",
            env_value: "",
            expected_language_id: "rust",
            initialization_path: "",
            initialization_value: "",
        },
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["custom-rust-analyzer".to_string()]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let document_symbols = lsp
        .call(
            test_context(&workspace, "document-symbol"),
            json!({
                "operation": "documentSymbol",
                "filePath": "src/lib.rs",
            }),
        )
        .await
        .expect("documentSymbol request");
    let document_json = document_symbols
        .structured_json
        .clone()
        .expect("documentSymbol structured json");
    assert_eq!(document_json["operation"], json!("documentSymbol"));
    assert_eq!(
        document_json["filePath"],
        json!(workspace.join("src/lib.rs").display().to_string())
    );
    assert!(document_json.get("line").is_none());
    assert!(document_json.get("character").is_none());

    let workspace_symbols = lsp
        .call(
            test_context(&workspace, "workspace-symbol"),
            json!({
                "operation": "workspaceSymbol",
                "filePath": "src/lib.rs",
                "query": "helper",
            }),
        )
        .await
        .expect("workspaceSymbol request");
    let workspace_json = workspace_symbols
        .structured_json
        .expect("workspaceSymbol structured json");
    assert_eq!(workspace_json["operation"], json!("workspaceSymbol"));
    assert_eq!(workspace_json["query"], json!("helper"));
    assert!(workspace_json["result"][0]["query"] == json!("helper"));
    assert!(workspace_json.get("line").is_none());
    assert!(workspace_json.get("character").is_none());
}

#[tokio::test]
async fn native_code_lsp_rejects_unsupported_operation_cleanly() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let lsp = registry.get("lsp").expect("lsp tool");

    let unsupported_operation = lsp
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
        "use lsp.rename for the explicit write-capable rename flow",
    );

    let repeated_unsupported_operation = lsp
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
        repeated_unsupported_operation,
        "use lsp.rename for the explicit write-capable rename flow",
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_rename_previews_and_applies_workspace_edits() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-rename-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake rename bin dir");
    let primary_uri = file_uri(&workspace.join("src/lib.rs"));
    let secondary_uri = file_uri(&workspace.join("src/other.rs"));
    install_fake_rename_lsp_binary(
        &fake_bin,
        "custom-rename-lsp",
        &primary_uri,
        &secondary_uri,
        "ok",
        "documentChanges",
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["custom-rename-lsp".to_string()]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let rename_tool = registry.get("lsp.rename").expect("lsp.rename tool");

    let preview = rename_tool
        .call(
            test_context(&workspace, "rename-preview"),
            json!({
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 4,
                "newName": "renamed",
                "apply": false,
            }),
        )
        .await
        .expect("rename preview");
    assert!(preview.display_text.contains("Prepared LSP rename preview"));
    assert!(preview.display_text.contains("helper"));
    assert!(preview.display_text.contains("Re-run with `apply: true`"));
    let preview_json = preview
        .structured_json
        .clone()
        .expect("rename preview structured json");
    assert_eq!(preview_json["operation"], json!("renameSymbol"));
    assert_eq!(preview_json["applied"], json!(false));
    assert_eq!(preview_json["symbol"], json!("helper"));
    assert_eq!(preview_json["preview"]["file_count"], json!(2));
    assert_eq!(preview_json["preview"]["text_edit_count"], json!(2));
    assert_eq!(
        preview_json["preview"]["annotations"][0]["label"],
        json!("Semantic rename")
    );
    assert!(first_diagnostic_message(&preview_json).contains("prepare_ok=True; workspace_ok=True"));
    assert_eq!(
        fs::read_to_string(workspace.join("src/lib.rs")).expect("read lib.rs after preview"),
        "fn helper() {}\n\nfn caller() {\n    helper();\n}\n"
    );

    let apply = rename_tool
        .call(
            test_context(&workspace, "rename-apply"),
            json!({
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 4,
                "newName": "renamed",
                "apply": true,
            }),
        )
        .await
        .expect("rename apply");
    assert!(apply.display_text.contains("Applied LSP rename"));
    assert_eq!(
        fs::read_to_string(workspace.join("src/lib.rs")).expect("read lib.rs after apply"),
        "fn renamed() {}\n\nfn caller() {\n    helper();\n}\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("src/other.rs")).expect("read other.rs after apply"),
        "fn another() {\n    renamed();\n}\n"
    );
    let apply_json = apply.structured_json.expect("rename apply structured json");
    assert_eq!(apply_json["applied"], json!(true));
    assert_eq!(apply_json["appliedEdits"].as_array().map(Vec::len), Some(2),);
    assert_eq!(apply.artifacts.len(), 2);
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global test lock intentionally serializes PATH and LSP registry mutations across awaits"
)]
async fn native_code_lsp_rename_reports_unsupported_server_behavior() {
    let _lock = test_lock().lock().expect("test lock");
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fake_bin = temp_dir.path().join("fake-rename-lsp-bin");
    fs::create_dir_all(&fake_bin).expect("fake rename bin dir");
    let primary_uri = file_uri(&workspace.join("src/lib.rs"));
    let secondary_uri = file_uri(&workspace.join("src/other.rs"));
    install_fake_rename_lsp_binary(
        &fake_bin,
        "unsupported-rename-lsp",
        &primary_uri,
        &secondary_uri,
        "null",
        "documentChanges",
    );
    let _path_guard = PathEnvGuard::prepend(&fake_bin);
    let _config_guard = LspConfigGuard::install(LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["unsupported-rename-lsp".to_string()]),
                extensions: None,
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    });

    let registry = coordinator_registry(ShellAllowlist::default());
    let rename_tool = registry.get("lsp.rename").expect("lsp.rename tool");
    let error = rename_tool
        .call(
            test_context(&workspace, "rename-unsupported"),
            json!({
                "filePath": "src/lib.rs",
                "line": 1,
                "character": 4,
                "newName": "renamed",
                "apply": false,
            }),
        )
        .await
        .expect_err("unsupported rename server should fail");
    match error {
        ToolError::Execution(message) => assert!(
            message.contains("rename is unavailable"),
            "expected unsupported rename error, got {message:?}"
        ),
        other => panic!("expected execution error, got {other:?}"),
    }
}
