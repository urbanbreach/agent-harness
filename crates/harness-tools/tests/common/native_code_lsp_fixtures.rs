use harness_tools::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use harness_core::config::{
    registered_lsp_config, set_registered_lsp_config, LspConfig, LspServerConfig, ShellAllowlist,
};
use harness_core::tool::ToolError;
use harness_tools::coordinator_registry;
use serde_json::json;

#[path = "mod.rs"]
mod common;

use common::test_context as common_test_context;

fn test_context(workspace_root: &Path, tool_call_id: &str) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-code-lsp-tests", tool_call_id)
}

fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();
    fs::create_dir_all(workspace.join("web")).unwrap_or_abort();
    fs::create_dir_all(workspace.join("python")).unwrap_or_abort();
    fs::create_dir_all(workspace.join("go")).unwrap_or_abort();
    fs::create_dir_all(workspace.join("config")).unwrap_or_abort();
    fs::create_dir_all(workspace.join("custom")).unwrap_or_abort();

    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"code_lsp_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("src/lib.rs"),
        "fn helper() {}\n\nfn caller() {\n    helper();\n}\n",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("src/extra.rs"), "pub fn extra() {}\n")
        .unwrap_or_abort();
    fs::write(
        workspace.join("src/other.rs"),
        "fn another() {\n    helper();\n}\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("package.json"),
        "{\"name\":\"code-lsp-test\"}\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("pyproject.toml"),
        "[project]\nname = \"code-lsp-test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("go.mod"),
        "module example.com/code-lsp-test\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("web/app.ts"),
        "const answer = 42;\nexport function read() {\n  return answer;\n}\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("python/app.py"),
        "ANSWER = 42\n\ndef read() -> int:\n    return ANSWER\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("go/main.go"),
        "package main\n\nfunc helper() {}\n\nfunc main() {\n\thelper()\n}\n",
    )
    .unwrap_or_abort();
    fs::write(
        workspace.join("config/settings.json"),
        "{\n  \"answer\": 42\n}\n",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("config/service.yaml"), "answer: 42\n").unwrap_or_abort();
    fs::write(
        workspace.join("custom/schema.foo"),
        "thing Example\n\nreference Example\n",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("unsupported.lua"), "print('unsupported')\n")
        .unwrap_or_abort();

    temp_dir
}

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn fake_lsp_command(directory: &Path, name: &str) -> String {
    directory.join(name).display().to_string()
}

fn assert_server_command(structured: &serde_json::Value, expected: &[&str]) {
    let actual = structured["server"]["command"]
        .as_array()
        .unwrap_or_abort();
    assert_eq!(actual.len(), expected.len());
    for (index, expected_segment) in expected.iter().enumerate() {
        let actual_segment = actual[index].as_str().unwrap_or_abort();
        if index == 0 {
            assert!(
                actual_segment.ends_with(expected_segment),
                "expected command executable {actual_segment:?} to end with {expected_segment:?}"
            );
        } else {
            assert_eq!(actual_segment, *expected_segment);
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
        &serde_json::to_string(&counter_path.display().to_string()).unwrap_or_abort(),
    );
    let path = directory.join(name);
    fs::write(&path, script).unwrap_or_abort();
    let mut permissions = fs::metadata(&path)
        .unwrap_or_abort()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap_or_abort();
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
    .replace("__RESULT_URI__", &serde_json::to_string(spec.result_uri).unwrap_or_abort())
    .replace(
        "__DIAGNOSTIC_MESSAGE__",
        &serde_json::to_string(spec.diagnostic_message).unwrap_or_abort(),
    )
    .replace("__EMPTY_DIAGNOSTICS__", if spec.empty_diagnostics { "True" } else { "False" })
    .replace(
        "__EXPECT_ENV_KEY__",
        &serde_json::to_string(spec.env_key).unwrap_or_abort(),
    )
    .replace(
        "__EXPECT_ENV_VALUE__",
        &serde_json::to_string(spec.env_value).unwrap_or_abort(),
    )
    .replace(
        "__EXPECT_LANGUAGE_ID__",
        &serde_json::to_string(spec.expected_language_id).unwrap_or_abort(),
    )
    .replace(
        "__EXPECT_INIT_PATH__",
        &serde_json::to_string(spec.initialization_path).unwrap_or_abort(),
    )
    .replace(
        "__EXPECT_INIT_VALUE__",
        &serde_json::to_string(spec.initialization_value).unwrap_or_abort(),
    );
    let path = directory.join(name);
    fs::write(&path, script).unwrap_or_abort();
    let mut permissions = fs::metadata(&path)
        .unwrap_or_abort()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap_or_abort();
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
    .replace("__PRIMARY_URI__", &serde_json::to_string(primary_uri).unwrap_or_abort())
    .replace(
        "__SECONDARY_URI__",
        &serde_json::to_string(secondary_uri).unwrap_or_abort(),
    )
    .replace(
        "__PREPARE_MODE__",
        &serde_json::to_string(prepare_mode).unwrap_or_abort(),
    )
    .replace(
        "__RENAME_MODE__",
        &serde_json::to_string(rename_mode).unwrap_or_abort(),
    );
    let path = directory.join(name);
    fs::write(&path, script).unwrap_or_abort();
    let mut permissions = fs::metadata(&path)
        .unwrap_or_abort()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap_or_abort();
}

#[allow(clippy::panic, reason = "test fixture code must panic gracefully")]
fn expect_invalid_arguments(error: ToolError, expected_fragment: &str) {
    match error {
        ToolError::InvalidArguments(message) => assert!(
            message.contains(expected_fragment),
            "expected invalid-arguments error containing {expected_fragment:?}, got {message:?}"
        ),
        _ => panic!("abort"),
    }
}

fn first_diagnostic_message(value: &serde_json::Value) -> &str {
    value["diagnostics"][0]["diagnostics"][0]["message"]
        .as_str()
        .unwrap_or_abort()
}

fn file_uri(path: &Path) -> String {
    reqwest::Url::from_file_path(path)
        .unwrap_or_abort()
        .to_string()
}
