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
        workspace.join("web/app.ts"),
        "const answer = 42;\nexport function read() {\n  return answer;\n}\n",
    )
    .expect("write typescript fixture");
    fs::write(
        workspace.join("custom/schema.foo"),
        "thing Example\n\nreference Example\n",
    )
    .expect("write custom fixture");
    fs::write(workspace.join("unsupported.py"), "print('unsupported')\n")
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

struct FakeLspSpec<'a> {
    result_uri: &'a str,
    diagnostic_message: &'a str,
    env_key: &'a str,
    env_value: &'a str,
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
EXPECT_INIT_PATH = __EXPECT_INIT_PATH__
EXPECT_INIT_VALUE = __EXPECT_INIT_VALUE__

opened_uri = None
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


while True:
    message = read_message()
    if message is None:
        break

    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params", {})

    if method == "textDocument/didOpen":
        opened_uri = params.get("textDocument", {}).get("uri")
        continue

    if method == "initialize" and message_id is not None:
        initialization_options = params.get("initializationOptions", {})
        send({"jsonrpc": "2.0", "id": message_id, "result": {"capabilities": {}}})
        continue

    if message_id is None:
        continue

    env_ok = EXPECT_ENV_KEY == "" or os.environ.get(EXPECT_ENV_KEY) == EXPECT_ENV_VALUE
    init_value = lookup(initialization_options, EXPECT_INIT_PATH)
    init_ok = EXPECT_INIT_PATH == "" or str(init_value).lower() == EXPECT_INIT_VALUE.lower()
    target_uri = params.get("textDocument", {}).get("uri") or opened_uri or "file:///workspace/unknown"
    send({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": target_uri,
            "diagnostics": [{
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 3}
                },
                "severity": 1,
                "source": method,
                "message": f"{DIAGNOSTIC_MESSAGE}; env_ok={env_ok}; init_ok={init_ok}"
            }]
        }
    })
    position = params.get("position", {})
    line = position.get("line", 0)
    character = position.get("character", 0)
    send({
        "jsonrpc": "2.0",
        "id": message_id,
        "result": [{
            "uri": RESULT_URI,
            "range": {
                "start": {"line": line, "character": character},
                "end": {"line": line, "character": character}
            }
        }]
    })
    break
"#
    .replace("__RESULT_URI__", &serde_json::to_string(spec.result_uri).expect("result uri json"))
    .replace(
        "__DIAGNOSTIC_MESSAGE__",
        &serde_json::to_string(spec.diagnostic_message).expect("diagnostic json"),
    )
    .replace(
        "__EXPECT_ENV_KEY__",
        &serde_json::to_string(spec.env_key).expect("env key json"),
    )
    .replace(
        "__EXPECT_ENV_VALUE__",
        &serde_json::to_string(spec.env_value).expect("env value json"),
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
            env_key: "RUST_LOG",
            env_value: "trace",
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
            env_key: "TS_SERVER_LOG",
            env_value: "verbose",
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
            env_key: "CUSTOM_LSP_MODE",
            env_value: "enabled",
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
    let native = registry.get("code.lsp").expect("code.lsp tool");
    let compat = registry.get("lsp").expect("lsp alias tool");

    let rust_result = native
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
    assert!(first_diagnostic_message(&rust_json).contains("env_ok=True; init_ok=True"));

    let compat_ts = compat
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
    assert!(compat_ts
        .display_text
        .contains("typescript override diagnostic"));
    let ts_json = compat_ts
        .structured_json
        .clone()
        .expect("configured typescript structured json");
    assert_eq!(ts_json["server"]["name"], json!("typescript"));
    assert_eq!(ts_json["server"]["command"], json!(["custom-ts-lsp"]));
    assert!(first_diagnostic_message(&ts_json).contains("env_ok=True; init_ok=True"));

    let custom_result = native
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
    assert!(first_diagnostic_message(&custom_json).contains("env_ok=True; init_ok=True"));
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
    let native = registry.get("code.lsp").expect("code.lsp tool");
    let compat = registry.get("lsp").expect("lsp alias tool");

    let disabled_rust = native
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
        "configured code.lsp server `rust` is disabled for extension .rs",
    );

    let compat_disabled_custom = compat
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
        compat_disabled_custom,
        "configured code.lsp server `custom-local` is disabled for extension .foo",
    );

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
}

#[test]
fn native_code_lsp_rejects_disabled_or_unsupported_server_requests() {
    native_code_lsp_rejects_disabled_or_unsupported_servers();
}

#[tokio::test]
async fn native_code_lsp_rejects_unsupported_operation_cleanly() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let registry = coordinator_registry(ShellAllowlist::default());
    let native = registry.get("code.lsp").expect("code.lsp tool");
    let compat = registry.get("lsp").expect("lsp alias tool");

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
        "use code.lsp.rename for the explicit write-capable rename flow",
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
        "use code.lsp.rename for the explicit write-capable rename flow",
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
    let rename_tool = registry
        .get("code.lsp.rename")
        .expect("code.lsp.rename tool");

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
    let rename_tool = registry
        .get("code.lsp.rename")
        .expect("code.lsp.rename tool");
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
