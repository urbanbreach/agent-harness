"""Scripted LSP peer: real framing, pull reports, and late push notifications."""
import json
import sys
import os
import signal
from pathlib import Path
from urllib.parse import unquote, urlparse

mode = sys.argv[1]
if mode == "rust-analyzer":
    with open(os.environ["LSP_PROTOCOL_LOG"], "a") as stream:
        stream.write(json.dumps({"pid": os.getpid(), "method": "server-started"}) + "\n")
    os.execvp("rust-analyzer", ["rust-analyzer"])

documents = {}
pending = None
ready = False
result_ids = {}


def send(message):
    body = json.dumps({"jsonrpc": "2.0", **message}).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()


def diagnostics(text):
    return ([{"range": {"start": {"line": 0, "character": 0},
                       "end": {"line": 0, "character": 6}},
              "severity": 1, "code": "fixture-error", "message": "Broken source detected"}]
            if "BROKEN" in text else [])


def publish(uri, version, stale=False):
    send({"method": "textDocument/publishDiagnostics", "params": {
        "uri": uri, "version": version, "diagnostics": [] if stale else diagnostics(documents[uri]["text"])}})


while True:
    headers = {}
    while line := sys.stdin.buffer.readline():
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode().split(":", 1)
        headers[name.lower()] = value.strip()
    if not line:
        break
    if mode == "hang-write" and ready:
        with open(os.environ["LSP_PROTOCOL_LOG"], "a") as stream:
            stream.write(json.dumps({"pid": os.getpid(), "method": "blocked-write"}) + "\n")
        signal.pause()
    message = json.loads(sys.stdin.buffer.read(int(headers["content-length"])))
    if log := os.environ.get("LSP_PROTOCOL_LOG"):
        with open(log, "a") as stream:
            stream.write(json.dumps({"pid": os.getpid(), **message}) + "\n")
    method, request_id = message.get("method"), message.get("id")
    params = message.get("params", {})
    if method == "initialize":
        if mode == "hang-initialize":
            continue
        capabilities = {} if mode in ("push", "versionless") else {"diagnosticProvider": {"workspaceDiagnostics": False}}
        capabilities["textDocumentSync"] = {"change": 2, "save": {"includeText": True}}
        result = {"capabilities": capabilities}
        if mode == "cold":
            result["serverInfo"] = {"name": "rust-analyzer"}
        send({"id": request_id, "result": result})
    elif method == "initialized" and mode == "hang-write":
        ready = True
    elif method == "initialized" and mode == "cold":
        send({"method": "experimental/serverStatus", "params": {"health": "ok", "quiescent": False}})
        send({"id": "readiness-barrier", "method": "workspace/configuration", "params": {"items": [{}]}})
    elif request_id == "readiness-barrier":
        ready = True
        send({"method": "experimental/serverStatus", "params": {"health": "ok", "quiescent": True}})
    elif method in ("textDocument/didOpen", "textDocument/didChange"):
        document = params["textDocument"]
        uri = document["uri"]
        if method == "textDocument/didChange":
            assert document["version"] > documents[uri]["version"]
            document["text"] = params["contentChanges"][0]["text"]
        documents[uri] = document
        assert len(documents) <= 200
        if mode == "fallback" and method == "textDocument/didChange":
            publish(uri, document["version"])
        if mode in ("push", "versionless"):
            publish(uri, document["version"] - 1, stale=True)
            publish(uri, document["version"] + 1, stale=True)
            if mode == "versionless":
                publish(uri, None, stale=True)
            pending = uri
            # The current-version publish cannot arrive until the client services this request.
            send({"id": "diagnostic-barrier", "method": "workspace/configuration", "params": {"items": [{}]}})
        elif mode == "close":
            break
    elif request_id == "diagnostic-barrier":
        publish(pending, None if mode == "versionless" else documents[pending]["version"])
    elif method == "textDocument/diagnostic":
        uri = params["textDocument"]["uri"]
        if mode == "fallback":
            send({"id": request_id, "error": {"code": -32601, "message": "unknown request"}})
            publish(uri, documents[uri]["version"])
        elif mode == "unchanged":
            send({"id": request_id, "result": {"kind": "unchanged", "resultId": "unknown"}})
        elif mode == "hang":
            continue
        elif mode == "exit-once" and not Path(".lsp-exited").exists():
            Path(".lsp-exited").touch()
            break
        elif mode == "malformed":
            send({"id": request_id, "result": {"kind": "full"}})
        else:
            if mode == "mutate":
                Path(unquote(urlparse(uri).path)).write_text("BROKEN concurrently modified\n")
            items = [] if mode == "cold" and not ready else diagnostics(documents[uri]["text"])
            previous = result_ids.get(uri)
            result_id = f"{documents[uri]['version']}:{request_id}"
            result_ids[uri] = result_id
            report = ({"kind": "unchanged", "resultId": result_id}
                      if mode == "cached" and previous is not None and params.get("previousResultId") == previous
                      else {"kind": "full", "items": items, "resultId": result_id})
            send({"id": request_id, "result": report})
            if mode == "cached":
                send({"id": f"idle-{request_id}", "method": "workspace/configuration", "params": {"items": [{}, {}]}})
    elif method == "textDocument/hover":
        send({"id": request_id, "result": None})
    elif method == "textDocument/prepareRename":
        send({"id": request_id, "result": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}}})
    elif method == "textDocument/rename":
        send({"id": request_id, "result": {"changes": {}}})
    elif method == "shutdown":
        send({"id": request_id, "result": None})
    elif method == "exit":
        break
    elif method == "textDocument/didSave":
        assert params["text"] == documents[params["textDocument"]["uri"]]["text"]
    elif isinstance(request_id, str) and request_id.startswith("idle-"):
        assert message["result"] == [{}, {}]
    elif method == "textDocument/didClose":
        documents.pop(params["textDocument"]["uri"], None)
    elif request_id is not None:
        send({"id": request_id, "error": {"code": -32601, "message": "unknown request"}})
