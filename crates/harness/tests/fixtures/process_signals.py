"""Real PTYs and process signals; invoked only by binary signoff."""
import http.server
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import sys
import tempfile
import termios
import threading
import time


def wait_until(predicate, child, master=None):
    deadline = time.monotonic() + 15
    while not predicate():
        if child.poll() is not None:
            assert predicate(), "harness exited before reaching the fixture"
            return
        assert time.monotonic() < deadline, "fixture deadline exceeded"
        if master is not None:
            if select.select([master], [], [], 0.01)[0]:
                os.read(master, 65536)
        else:
            threading.Event().wait(0.01)


class Provider(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        call = {"index": 0, "id": "running-tool", "type": "function", "function": {
            "name": "bash", "arguments": json.dumps({
                "command": "python3 -c 'import os,time; open(\"child.pid\",\"w\").write(str(os.getpid())); time.sleep(60)'",
                "description": "Wait for termination"
            })}}
        body = "data: " + json.dumps({"choices": [{"delta": {"tool_calls": [call]},
                                                    "finish_reason": "tool_calls"}]}) + "\n\ndata: [DONE]\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body.encode())

    def log_message(self, *args):
        pass


binary = str(Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix="harness-signals-") as directory:
    root = Path(directory)
    environment = {"HOME": directory, "HARNESS_HOME": str(root / "data"),
                   "PATH": "/usr/bin:/bin", "TERM": "xterm-256color", "HARNESS_CONFIG_CONTENT": "{}"}
    snapshots = {}
    for signum in (signal.SIGTERM, signal.SIGHUP):
        master, slave = pty.openpty()
        original = termios.tcgetattr(slave)
        child = subprocess.Popen([binary, "tui", "--mock", "--deterministic", "--session-dir", "sessions"],
                                 stdin=slave, stdout=slave, stderr=slave, cwd=root, env=environment)
        try:
            wait_until(lambda: not termios.tcgetattr(slave)[3] & termios.ICANON, child, master)
            os.write(master, b"hello\r")
            wait_until(lambda: len(list((root / "sessions").glob("*/events.jsonl"))) > len(snapshots)
                       and not termios.tcgetattr(slave)[3] & termios.ICANON, child, master)
            os.kill(child.pid, signum)
            wait_until(lambda: child.poll() is not None, child, master)
            assert child.returncode == 1, (signum, child.returncode)
            assert termios.tcgetattr(slave) == original, "terminal modes were not restored"
            for path, contents in snapshots.items():
                assert path.read_bytes() == contents, "deterministic live startup replaced history"
            snapshots = {p: p.read_bytes() for p in (root / "sessions").glob("*/events.jsonl")}
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            termios.tcsetattr(slave, termios.TCSANOW, original)
            os.close(master)
            os.close(slave)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    config = {"provider": {"fixture": {"type": "openai_compatible", "apiMode": "chat_completions",
              "baseURL": f"http://127.0.0.1:{server.server_port}/v1", "apiKeyEnv": [],
              "models": {"fixture": {"limit": {"context": 128000, "output": 1000}}}}},
              "model": "fixture/fixture", "agent": {"default": {"tools": ["bash"]}},
              "permission": {"bash": "allow"}}
    environment["HARNESS_CONFIG_CONTENT"] = json.dumps(config)
    try:
        for signum in (signal.SIGTERM, signal.SIGHUP):
            pid_file = root / "child.pid"
            pid_file.unlink(missing_ok=True)
            with tempfile.TemporaryFile() as output:
                child = subprocess.Popen([binary, "--session-dir", "headless", "run", "Wait"],
                                         stdout=output, stderr=output, cwd=root, env=environment)
                tool_pid = None
                try:
                    wait_until(lambda: pid_file.exists() and pid_file.read_text().strip(), child)
                    tool_pid = int(pid_file.read_text())
                    os.kill(child.pid, signum)
                    assert child.wait(timeout=15) == 1
                    assert not Path(f"/proc/{tool_pid}").exists(), "tool shell survived shutdown"
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait()
                    if tool_pid is not None and Path(f"/proc/{tool_pid}").exists():
                        os.kill(tool_pid, signal.SIGKILL)
                    output.seek(0)
                    print(output.read().decode(errors="replace"), file=sys.stderr)
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
