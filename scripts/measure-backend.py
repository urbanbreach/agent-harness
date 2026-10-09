#!/usr/bin/env python3
"""Measure a release CLI against a local SSE fixture; never contact a live provider."""

import argparse
from datetime import datetime, timezone
import hashlib
import http.server
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import threading
import time


def cpu_seconds(pid):
    # The command name may contain spaces or parentheses.
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")


def sample(binary):
    entered, stream, done = (threading.Event() for _ in range(3))
    fragments = 5000
    failures, first_output, closed, output = [], [], [], bytearray()
    with tempfile.TemporaryDirectory(prefix="harness-perf-") as temporary:
        root = Path(temporary)

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                try:
                    request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                    assert request["model"] == "fixture"
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Connection", "close")
                    self.end_headers()
                    self.wfile.flush()
                    entered.set()
                    if not stream.wait(15):
                        raise TimeoutError("stream was not released")
                    delta = 'data: {"choices":[{"index":0,"delta":{"content":"word "}}]}\n\n'
                    for _ in range(fragments):
                        self.wfile.write(delta.encode())
                    self.wfile.write(b'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n')
                    self.wfile.flush()
                except Exception as error:
                    failures.append(str(error))
                finally:
                    done.set()

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        server.daemon_threads = True
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        config = root / "fixture.json"
        config.write_text(json.dumps({
            "provider": {"local": {"type": "openai_compatible", "apiMode": "chat_completions",
                "baseURL": f"http://127.0.0.1:{server.server_port}/v1", "apiKeyEnv": [],
                "apiKey": "local-fixture-credential",
                "models": {"fixture": {"name": "Fixture", "limit": {"context": 128000, "output": 32000}}}}},
            "model": "local/fixture", "agent": {"default": {"tools": []}},
            "runtime": {"prompt": {"wait_timeout_ms": 30000}, "provider_retry": {"max_retries": 0}}
        }))
        environment = {name: os.environ[name] for name in ["PATH", "LANG", "LC_ALL", "LD_LIBRARY_PATH"] if name in os.environ}
        environment.update(HOME=str(root / "home"), HARNESS_HOME=str(root / "harness-home"))
        launched = time.perf_counter()
        process = subprocess.Popen([
            str(binary), "--cwd", str(root), "--config", str(config),
            "prompt", "--text", "Stream the fixture output"
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment, cwd=root)

        def read_output():
            while chunk := process.stdout.read1(8192):
                if not first_output:
                    first_output.append(time.perf_counter())
                output.extend(chunk)
            closed.append(time.perf_counter())

        reader = threading.Thread(target=read_output)
        reader.start()
        try:
            if not entered.wait(10):
                detail = process.stderr.read().decode(errors="replace") if process.poll() is not None else "process is still running"
                raise RuntimeError(f"CLI did not request the local provider: {detail}")
            startup_ms = (time.perf_counter() - launched) * 1000
            idle_start, cpu_start = time.perf_counter(), cpu_seconds(process.pid)
            if done.wait(1):
                raise RuntimeError("provider exited during the idle sample")
            idle_seconds = time.perf_counter() - idle_start
            idle_cpu = cpu_seconds(process.pid) - cpu_start
            streaming = time.perf_counter()
            stream.set()
            deadline = time.monotonic() + 30
            while True:
                pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                if time.monotonic() > deadline:
                    raise subprocess.TimeoutExpired(process.args, 30)
                time.sleep(0.01)
            reader.join(timeout=3)
            if reader.is_alive():
                raise RuntimeError("CLI output did not close")
            errors = process.stderr.read().decode(errors="replace")
            if process.returncode != 0 or failures:
                raise RuntimeError(f"CLI status {process.returncode}; fixture errors {failures}; {errors}")
            if output != b"word " * fragments + b"\n":
                raise RuntimeError("streamed output was lost or repeated")
            journal = next((root / "harness-home/sessions").glob("*/*/events.jsonl"))
            events = [json.loads(line)["payload"]["event_type"] for line in journal.read_text().splitlines()]
            if events[-1] != "run_finished" or any(event in ["provider_stream_delta", "provider_reasoning_delta"] for event in events):
                raise RuntimeError("streaming produced an invalid durable history")
            return {"cpu_user_s": usage.ru_utime, "cpu_system_s": usage.ru_stime,
                    "peak_rss_kib": usage.ru_maxrss, "startup_ms": startup_ms,
                    "idle_cpu_percent": idle_cpu / idle_seconds * 100,
                    "first_fragment_ms": (first_output[0] - streaming) * 1000,
                    "stream_and_commit_ms": (closed[0] - streaming) * 1000,
                    "fragments": fragments, "output_bytes": len(output),
                    "journal_bytes": journal.stat().st_size, "durable_events": len(events)}
        finally:
            stream.set()
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            reader.join(timeout=3)
            server.shutdown()
            server.server_close()
            server_thread.join()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repetitions", type=int, default=3)
    args = parser.parse_args()
    if platform.system() != "Linux":
        parser.error("Linux /proc and wait4 resource reporting are required")
    if not 1 <= args.repetitions <= 20:
        parser.error("repetitions must be between 1 and 20")
    binary = args.binary.resolve(strict=True)
    samples = [sample(binary) for _ in range(args.repetitions)]
    with binary.open("rb") as executable:
        digest = hashlib.file_digest(executable, "sha256").hexdigest()
    report = {"schema_version": 1, "fixture_version": 3,
              "captured_at": datetime.now(timezone.utc).isoformat(),
              "platform": platform.platform(), "machine": platform.machine(),
              "binary_sha256": digest,
              "binary_bytes": binary.stat().st_size,
              "cpu_resolution_ms": 1000 / os.sysconf("SC_CLK_TCK"),
              "samples": samples,
              "medians": {name: statistics.median(s[name] for s in samples) for name in samples[0]}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["medians"], indent=2))


if __name__ == "__main__":
    main()
