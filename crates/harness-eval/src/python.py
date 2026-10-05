"""Persistent Python interpreter adapter for the native Harness eval manager."""
import ast
import asyncio
import base64
import builtins
import contextvars
import gc
import inspect
import io
import json
import os
from pathlib import Path
import queue
import re
import signal
import subprocess
import sys
import threading
import time
import tokenize
import urllib.parse
from concurrent.futures import ThreadPoolExecutor

print(json.dumps({"type": "status", "event": {"op": "kernel-startup", "stage": "runtime-init"}}), flush=True)
_native = _NativeIo()
_wire = _native.wire
_input = _native.input
_lock = threading.RLock()
_scope = contextvars.ContextVar("eval_cell", default=None)
_requests = queue.Queue(16)
_pending = {}
_active = None
_sequence = 0
_cancel = threading.Event()
_options = {}
_contributed = set()
_loop = asyncio.new_event_loop()


def _emit(message):
    cell = _scope.get()
    if message["type"] != "ready" and (not cell or cell != _active):
        return
    with _lock:
        _drain_native(cell)
        _wire.write(json.dumps({**message, "cellId": cell or ""}, ensure_ascii=False, default=repr) + "\n")
        _wire.flush()


def _drain_native(cell):
    for stream, data in _native.drain():
        if cell:
            _wire.write(json.dumps({"type": "text", "cellId": cell, "stream": stream, "data": data}, ensure_ascii=False) + "\n")
    _wire.flush()


def _poll_native():
    while True:
        time.sleep(0.05)
        with _lock:
            _drain_native(_active)


def _read_protocol():
    for line in _input:
        if len(line.encode()) > 32 * 1024 * 1024:
            break
        try:
            frame = json.loads(line)
        except ValueError:
            break
        kind = frame.get("type")
        if kind == "reply":
            with _lock:
                reply = _pending.pop(frame.get("id"), None)
            if reply is not None:
                reply.put(frame)
        elif kind in ("cancel", "shutdown"):
            if _active and (kind == "shutdown" or frame.get("id") == _active):
                _cancel.set()
                with _lock:
                    replies = list(_pending.values())
                    _pending.clear()
                for reply in replies:
                    reply.put({"error": "Python cell interrupted"})
                os.kill(os.getpid(), signal.SIGINT)
            if kind == "shutdown":
                break
        else:
            _requests.put(frame)
    # A closed owner pipe must also retire CPU-bound code and its child processes.
    if hasattr(os, "killpg") and os.getpgrp() == os.getpid():
        os.killpg(os.getpgrp(), signal.SIGKILL)
    _requests.put(None)


def _call(operation, args):
    global _sequence
    if not _scope.get() or _scope.get() != _active or _cancel.is_set():
        raise RuntimeError("eval cell is no longer active")
    with _lock:
        _sequence += 1
        call_id = str(_sequence)
        reply = queue.Queue(1)
        _pending[call_id] = reply
    _emit({"type": "call", "id": call_id, "operation": operation, "args": args})
    try:
        response = reply.get()
        if "error" in response:
            fields = response["error"]
            error = RuntimeError(fields.get("message", "eval helper failed") if isinstance(fields, dict) else str(fields))
            if isinstance(fields, dict):
                error.code = fields.get("code")
            raise error
        return response.get("result")
    finally:
        with _lock:
            _pending.pop(call_id, None)


class _Stream(io.TextIOBase):
    def __init__(self, name):
        self.name = name

    def write(self, text):
        if text:
            _emit({"type": "text", "stream": self.name, "data": str(text)})
        return len(text)

    def flush(self):
        pass


def _display(mime, data):
    if isinstance(data, str):
        data = data.encode()
    _emit({"type": "display", "mimeType": mime, "dataBase64": base64.b64encode(data).decode()})


def display(value):
    if isinstance(value, dict):
        if value.get("type") == "markdown" and isinstance(value.get("text"), str):
            return _display("text/markdown", value["text"])
        if "images" in value and isinstance(value["images"], list):
            if value.get("text"):
                print(value["text"])
            for image in value["images"]:
                display(image)
            return
        if str(value.get("mimeType", "")).startswith("image/"):
            data = value.get("dataBase64", value.get("data"))
            if isinstance(data, str):
                return _display(value["mimeType"], base64.b64decode(data))
    if isinstance(value, str) and value.startswith("data:image/") and ";base64," in value:
        mime, data = value[5:].split(";base64,", 1)
        return _display(mime, base64.b64decode(data))
    if isinstance(value, (dict, list, tuple)):
        return _display("application/json", json.dumps(value, ensure_ascii=False, default=repr))
    if isinstance(value, (bytes, bytearray, memoryview)):
        raw = bytes(value)
        mime = next((mime for prefix, mime in [(b"\x89PNG", "image/png"), (b"\xff\xd8", "image/jpeg"),
            (b"GIF8", "image/gif"), (b"BM", "image/bmp"), (b"RIFF", "image/webp")] if raw.startswith(prefix)), "application/octet-stream")
        return _display(mime, raw)
    bundle = {}
    rich = getattr(value, "_repr_mimebundle_", None)
    if callable(rich):
        try:
            result = rich()
            bundle.update(result[0] if isinstance(result, tuple) else result)
        except Exception:
            pass
    for method, mime in [("_repr_png_", "image/png"), ("_repr_jpeg_", "image/jpeg"),
        ("_repr_markdown_", "text/markdown"), ("_repr_json_", "application/json"),
        ("_repr_svg_", "image/svg+xml"), ("_repr_html_", "text/html"), ("_repr_latex_", "text/latex")]:
        representation = getattr(value, method, None)
        if mime not in bundle and callable(representation):
            try:
                bundle[mime] = representation()
            except Exception:
                pass
    if type(value).__module__.startswith("matplotlib") and callable(getattr(value, "savefig", None)):
        image = io.BytesIO()
        value.savefig(image, format="png")
        bundle["image/png"] = image.getvalue()
    for mime in ["image/png", "image/jpeg", "image/svg+xml", "text/markdown", "application/json", "text/html", "text/latex"]:
        data = bundle.get(mime)
        if data is not None:
            if mime == "application/json":
                data = json.dumps(data, ensure_ascii=False, default=repr)
            elif mime.startswith("image/") and isinstance(data, str) and mime != "image/svg+xml":
                data = base64.b64decode(data)
            return _display(mime, data)
    _display("text/plain", str(value))


def log(message):
    _emit({"type": "log", "message": str(message)})


def phase(title):
    _emit({"type": "phase", "title": str(title)})


def _status(op, **fields):
    _emit({"type": "status", "event": {"op": op, **fields}})


def env(key=None, value=None):
    if key is None:
        return dict(sorted(os.environ.items()))
    if value is not None:
        os.environ[key] = value
    result = os.environ.get(key)
    _status("env", key=key, value=result, action="get" if value is None else "set")
    return result


def _path(name):
    if isinstance(name, str) and re.match(r"^[a-z][a-z0-9+.-]*://", name, re.I):
        scheme, relative = name.split("://", 1)
        if scheme.lower() != "local":
            raise ValueError("unsupported file protocol: " + scheme)
        relative = urllib.parse.unquote(relative.replace("\\", "/"))
        if Path(relative).is_absolute() or ".." in Path(relative).parts:
            raise ValueError("local:// path must remain within its session directory")
        return Path(_options["localRoot"]) / relative
    return Path(name)


def read(name, offset=1, limit=None):
    path = _path(name)
    text = path.read_text(encoding="utf-8")
    if offset > 1 or limit is not None:
        text = "".join(text.splitlines(keepends=True)[max(0, offset - 1):None if limit is None else max(0, offset - 1) + limit])
    _status("read", path=str(path), chars=len(text), preview=text[:500])
    return text


def write(name, content):
    path = _path(name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    _status("write", path=str(path), chars=len(content))
    return path


class _Tools:
    def __getattr__(self, name):
        return self[name]

    def __getitem__(self, name):
        def invoke(args=None, **kwargs):
            if args is not None and kwargs:
                raise TypeError("tool arguments must be a dictionary or keyword arguments")
            return _call("tool", {"name": name, "parameters": kwargs if args is None else args})
        return invoke


tool = tools = _Tools()


def tool_schema(name=None):
    return _call("schema", {} if name is None else {"name": name})


def completion(prompt, model="default", system=None, schema=None, **options):
    options.update({key: value for key, value in {"model": model if model != "default" else None, "system": system, "schema": schema}.items() if value is not None})
    return _call("completion", {"prompt": prompt, "opts": options})


def agent(prompt, **options):
    return _call("agent", {"prompt": prompt, **options})


def output(*ids, **options):
    if len(ids) == 1 and isinstance(ids[0], (list, tuple)):
        ids = ids[0]
    ids = [value.get("id", value.get("handle")) if isinstance(value, dict) else value for value in ids]
    return _call("output", {"ids": [str(value).removeprefix("agent://") for value in ids], **options})


class _Workpool:
    def __init__(self, pool_id):
        self.pool_id = pool_id

    def push(self, items):
        return _call("workpool", {"op": "push", "pool_id": self.pool_id, "items": items})

    def close(self):
        return _call("workpool", {"op": "close", "pool_id": self.pool_id})

    def inspect(self):
        return _call("workpool", {"op": "inspect", "pool_id": self.pool_id})

    def cancel(self):
        return _call("workpool", {"op": "cancel", "pool_id": self.pool_id})


def workpool(agent, name, *, mode=None):
    options = {} if mode is None else {"mode": mode}
    result = _call("workpool", {"op": "create", "agent": agent, "name": name, **options})
    pool_id = result.get("details", {}).get("pool_id")
    if result.get("hasError") or not isinstance(pool_id, str):
        raise RuntimeError(result.get("text", "workpool creation failed"))
    return _Workpool(pool_id)


def parallel(thunks):
    cell = _scope.get()
    def invoke(thunk):
        token = _scope.set(cell)
        try:
            return thunk()
        finally:
            _scope.reset(token)
    with ThreadPoolExecutor(max_workers=_options.get("parallelPoolWidth", 4)) as pool:
        futures = [pool.submit(invoke, thunk) for thunk in thunks]
        # Leaving the executor waits for every submitted function, even on error.
        return [future.result() for future in futures]


def pipeline(items, *stages):
    values = list(items)
    for stage in stages:
        values = parallel([lambda value=value: stage(value) for value in values])
    return values


class _ShellResult(list):
    def __init__(self, lines, returncode):
        super().__init__(lines)
        self.returncode = returncode

    @property
    def n(self):
        return "\n".join(self)

    @property
    def s(self):
        return " ".join(self)


def _shell(command):
    process = subprocess.Popen(command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    captured = bytearray()
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    remaining_lines, truncated = 3000, False
    try:
        while True:
            chunk = process.stdout.read1(8192)
            text = decoder.decode(chunk, final=not chunk)
            if text and not truncated:
                lines = text.split("\n", remaining_lines)
                kept = "\n".join(lines[:remaining_lines]) + "\n" if len(lines) > remaining_lines else text
                if remaining_lines == 0:
                    kept = ""
                kept = kept.encode()[:max(0, 1024 * 1024 - len(captured))].decode(errors="ignore")
                captured.extend(kept.encode())
                remaining_lines -= kept.count("\n")
                sys.stdout.write(kept)
                if kept != text:
                    prefix = "" if kept.endswith("\n") else "\n"
                    sys.stdout.write(prefix + "[output truncated: shell helper exceeded 1048576 bytes or 3000 lines; remaining output discarded]\n")
                    truncated = True
            if not chunk:
                break
        return _ShellResult(captured.decode().splitlines(), process.wait())
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        process.stdout.close()


def _magic(name, args, body=None):
    if body is not None:
        if name not in ("bash", "sh"):
            raise ValueError("unsupported cell magic: %%" + name)
        return _shell(args + "\n" + body)
    if name == "cd":
        os.chdir(os.path.expanduser(args.strip() or "~"))
        _status("cd", path=os.getcwd())
        return os.getcwd()
    if name == "env":
        parts = args.split("=", 1)
        return env(*[part.strip() for part in parts]) if args.strip() else env()
    raise ValueError("unsupported line magic: %" + name)


def _transform(source):
    protected = set()
    try:
        for token in tokenize.generate_tokens(io.StringIO(source).readline):
            if token.type == tokenize.STRING and token.end[0] > token.start[0]:
                protected.update(range(token.start[0] + 1, token.end[0] + 1))
    except (tokenize.TokenError, IndentationError):
        pass
    lines = source.splitlines()
    output = []
    index = 0
    while index < len(lines):
        line = lines[index]
        number = index + 1
        index += 1
        match = re.match(r"^(\s*)(?:(\w+(?:\s*,\s*\w+)*)\s*=\s*)?([!%])(.*)$", line)
        if number in protected or not match:
            output.append(line)
            continue
        indent, assignment, kind, tail = match.groups()
        while tail.endswith("\\") and index < len(lines):
            tail = tail[:-1] + lines[index].strip()
            index += 1
        prefix = indent + ((assignment + " = ") if assignment else "")
        if kind == "!":
            output.append(prefix + f"__harness_shell({tail.strip()!r})")
        else:
            cell = tail.startswith("%")
            parts = tail[int(cell):].split(None, 1)
            name, args = (parts + [""])[:2]
            body = ", " + repr("\n".join(lines[index:])) if cell else ""
            output.append(prefix + f"__harness_magic({name!r}, {args!r}{body})")
            if cell:
                break
    return "\n".join(output)


_globals = {"__name__": "__main__", "__builtins__": builtins, **{name: globals()[name] for name in
    ["display", "log", "phase", "env", "read", "write", "parallel", "pipeline", "tool", "tools", "completion", "agent", "output", "workpool", "tool_schema"]},
    "__harness_shell": _shell, "__harness_magic": _magic}


async def _evaluate(source):
    tree = ast.parse(_transform(source), filename="<eval>", mode="exec")
    expression = tree.body.pop() if tree.body and isinstance(tree.body[-1], ast.Expr) else None
    result = eval(compile(tree, "<eval>", "exec", flags=ast.PyCF_ALLOW_TOP_LEVEL_AWAIT), _globals)
    if inspect.isawaitable(result):
        await result
    if expression is not None:
        result = eval(compile(ast.Expression(expression.value), "<eval>", "eval", flags=ast.PyCF_ALLOW_TOP_LEVEL_AWAIT), _globals)
        return await result if inspect.isawaitable(result) else result


signal.signal(signal.SIGINT, signal.SIG_IGN)
threading.Thread(target=_read_protocol, daemon=True).start()
threading.Thread(target=_poll_native, daemon=True).start()
for _request in iter(_requests.get, None):
    if _request.get("type") == "init":
        _wire.write(json.dumps({"type": "status", "event": {"op": "kernel-startup", "stage": "host-init"}}) + "\n")
        _wire.flush()
        _options = _request
        _memory = _PythonMemory(_options.get("memory", {}), _globals)
        _emit({"type": "ready", "runtime": {"name": "Python", "version": sys.version.split()[0], "path": sys.executable}})
        continue
    if _request.get("type") != "run":
        continue
    _active = _request["id"]
    with _lock:
        _native.clear()
    _scope.set(_active)
    _cancel.clear()
    _start = time.monotonic()
    signal.signal(signal.SIGINT, signal.default_int_handler)
    sys.stdout, sys.stderr = _Stream("stdout"), _Stream("stderr")
    try:
        _preludes = _request.get("preludes") or []
        _names = {name for prelude in _preludes for name in prelude["exports"]}
        for _name in _contributed - _names:
            _globals.pop(_name, None)
        for _prelude in _preludes:
            if any(name not in _globals for name in _prelude["exports"]):
                exec(_prelude.get("python", ""), _globals)
        _contributed = _names
        _value = _loop.run_until_complete(_evaluate(_request["code"]))
        _result = {"type": "result", "ok": True, "valueRepr": repr(_value) if _value is not None else None}
    except BaseException as _error:
        _result = {"type": "result", "ok": False, "error": {"message": f"{type(_error).__name__}: {_error}"}}
    finally:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
    _result["durationMs"] = int((time.monotonic() - _start) * 1000)
    try:
        _report = _memory.report()
        if _report is not None:
            _result["memory"] = _report
    except Exception as _error:
        _emit({"type": "text", "stream": "stderr", "data": f"[kernel memory measurement unavailable: {_error}]\n"})
    _emit(_result)
    _active = None
    _scope.set(None)
