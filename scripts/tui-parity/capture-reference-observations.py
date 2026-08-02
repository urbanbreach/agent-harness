#!/usr/bin/env python3
"""Capture sealed reference observations from the frozen reference binary.

Clean-room parity task 8 driver. Executes the exact frozen reference binary
(``inspirations/grok-build/target/debug/xai-grok-pager``, sha256
``883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5``) inside
isolated PTY sessions (default 120x40) and, for each of the 23
capture-required taxonomy families, writes:

    <root>/<family_id>/observations.json   normalized frames (cells, cursor,
                                           ANSI lifecycle flags, timing)
    <root>/<family_id>/contract.json       sealed expectations derived from the
                                           observations + pre-baked invariants
    <root>/<family_id>/mutation.json       failure mutants + RED receipt (every
                                           mutant must be detected)

Every artifact is bound to the canonical reference_epoch. Nothing is
fabricated: pre-baked invariants come from real probes of this binary; the
sealed self-check re-evaluates the contract against the captured observations
and refuses to seal a family whose contract fails. The RED receipt applies each
mutant to an in-memory copy of the observations and requires contract failure.

Usage:
    python3 scripts/tui-parity/capture-reference-observations.py [--root PATH]
        [--only family1,family2] [--cols 120] [--rows 40] [--dry-run]

Requires ``pyte``; when missing the driver bootstraps an isolated venv under
``target/reference-capture-venv`` and re-execs itself.
"""

# pyright: reportMissingImports=false

from __future__ import annotations

import argparse
import copy
import faulthandler
import hashlib
import json
import os
import re
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import threading
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BINARY = REPO_ROOT / "inspirations/grok-build/target/debug/xai-grok-pager"
EXPECTED_BINARY_SHA256 = "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5"
REFERENCE_EPOCH = "d65d98422e3e3b2ae9bcba7b6636e01ee81fa625a4928f382d93f6199a924d93"
DEFAULT_ROOT = (
    REPO_ROOT
    / ".omo/evidence/grok-build-clean-room-parity/20260727-110657/task-8-grok-build-clean-room-parity/reference-observations"
)

OBSERVATIONS_SCHEMA = "clean-room-parity-task-8-observations/v1"
CONTRACT_SCHEMA = "clean-room-parity-task-8-contract/v1"
MUTATION_SCHEMA = "clean-room-parity-task-8-mutation/v1"


def _ensure_pyte() -> None:
    """Import pyte, bootstrapping an isolated venv when it is absent."""
    try:
        import pyte  # noqa: F401

        return
    except ImportError:
        pass
    venv_dir = REPO_ROOT / "target" / "reference-capture-venv"
    venv_python = venv_dir / "bin" / "python3"
    if not venv_python.exists():
        print(f"[capture] bootstrapping pyte venv at {venv_dir} ...", flush=True)
        subprocess.run([sys.executable, "-m", "venv", str(venv_dir)], check=True)
        subprocess.run([str(venv_python), "-m", "pip", "install", "--quiet", "pyte"], check=True)
    os.execv(str(venv_python), [str(venv_python), *sys.argv])


# ---------------------------------------------------------------------------
# PTY capture engine
# ---------------------------------------------------------------------------


@dataclass
class ScenarioRun:
    scenario_id: str
    argv: list[str]
    kind: str  # "interactive" | "cli"
    cols: int
    rows: int
    raw: bytes = b""
    frames: list[dict] = field(default_factory=list)
    frame_labels: list[str] = field(default_factory=list)
    ansi: dict = field(default_factory=dict)
    exit_code: int | None = None
    exit_signal: int | None = None
    total_ms: float = 0.0
    home_files_after: dict[str, int] = field(default_factory=dict)
    file_tails: dict[str, str] = field(default_factory=dict)
    notes: str = ""


class PtyRunner:
    """Spawn scenarios in a fresh isolated HOME PTY and record everything."""

    def __init__(self, home: Path, work: Path, cols: int, rows: int) -> None:
        self.home = home
        self.work = work
        self.cols = cols
        self.rows = rows

    def _env(self, cols: int, rows: int) -> dict[str, str]:
        env = {
            "PATH": "/usr/bin:/bin",
            "HOME": str(self.home),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
            "XDG_DATA_HOME": str(self.home / ".local/share"),
            "XDG_CACHE_HOME": str(self.home / ".cache"),
            "XDG_STATE_HOME": str(self.home / ".local/state"),
            "TMPDIR": str(self.home / "tmp"),
            "TERM": "xterm-256color",
            "TZ": "UTC",
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            # Hermetic network: reject instantly instead of dialing out so
            # update/login/model paths yield deterministic offline errors.
            "http_proxy": "http://127.0.0.1:9",
            "https_proxy": "http://127.0.0.1:9",
            "all_proxy": "http://127.0.0.1:9",
            "HTTP_PROXY": "http://127.0.0.1:9",
            "HTTPS_PROXY": "http://127.0.0.1:9",
            "ALL_PROXY": "http://127.0.0.1:9",
            "NO_PROXY": "",
            "COLUMNS": str(cols),
            "LINES": str(rows),
        }
        return env

    def run(
        self,
        scenario_id: str,
        argv: list[str],
        kind: str,
        steps: list[dict],
        cols: int | None = None,
        rows: int | None = None,
        cols2: int | None = None,
        rows2: int | None = None,
        kill_timeout_s: float = 12.0,
    ) -> ScenarioRun:
        import fcntl
        import pty
        import termios

        cols = cols or self.cols
        rows = rows or self.rows
        run = ScenarioRun(scenario_id, argv, kind, cols, rows)
        master, slave = pty.openpty()
        winsz = struct.pack("HHHH", rows, cols, 0, 0)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, winsz)

        chunks: list[tuple[float, bytes]] = []
        t_start = time.monotonic()
        last_chunk_at = [t_start]
        read_error: list[BaseException] = []

        def reader() -> None:
            while True:
                try:
                    data = os.read(master, 65536)
                except OSError as exc:
                    read_error.append(exc)
                    break
                if not data:
                    break
                now = time.monotonic()
                chunks.append((now - t_start, data))
                last_chunk_at[0] = now

        th = threading.Thread(target=reader, daemon=True)
        th.start()
        env = self._env(cols, rows)
        # --leader-socket keeps any background leader inside the isolated HOME.
        full_argv = list(argv)
        if kind == "interactive" and "--leader-socket" not in full_argv:
            full_argv += ["--leader-socket", str(self.home / "leader.sock")]
        proc = subprocess.Popen(
            full_argv,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            cwd=str(self.work),
            env=env,
            start_new_session=True,
        )
        os.close(slave)

        import pyte

        screen = pyte.Screen(cols, rows)
        stream = pyte.Stream(screen)
        fed_len = 0
        frame_index = 0

        def feed_pending() -> None:
            nonlocal fed_len
            raw_so_far = b"".join(d for _, d in chunks)
            if len(raw_so_far) > fed_len:
                new = raw_so_far[fed_len:]
                fed_len = len(raw_so_far)
                stream.feed(new.decode("utf-8", "replace"))

        def quiet(s: float) -> None:
            # Wait until no PTY output has arrived for `s` seconds, bounded so a
            # continuously animating TUI cannot stall capture forever.
            deadline = time.monotonic() + max(s * 3.0, s + 5.0)
            while time.monotonic() < deadline:
                time.sleep(0.05)
                if time.monotonic() - last_chunk_at[0] >= s:
                    break
            feed_pending()

        def snapshot(label: str, styled_scan: bool = True) -> None:
            nonlocal frame_index
            feed_pending()
            # Manual grid build: iterate only touched rows. pyte's display
            # property is O(cells) with expensive defaultdict misses and takes
            # ~27s on a 120x1000 CLI screen.
            blank_row = " " * cols
            buffer = screen.buffer
            last_touched = max(buffer.keys(), default=-1)
            rows_out = min(rows, last_touched + 1)
            rows_txt: list[str] = []
            for y in range(rows_out):
                line = buffer.get(y)
                if not line:
                    rows_txt.append(blank_row)
                    continue
                chars = []
                for x in range(cols):
                    ch = line.get(x)
                    chars.append(ch.data if ch is not None else " ")
                rows_txt.append("".join(chars))
            trimmed_trailing = rows - rows_out
            grid = "\n".join(rows_txt)
            cursor = screen.cursor
            styled: dict[str, dict[str, list]] = {}
            nondefault = 0
            if styled_scan:
                # Iterate only touched cells instead of the full cols*rows grid.
                for y in sorted(screen.buffer):
                    line = screen.buffer[y]
                    for x in sorted(line):
                        ch = line[x]
                        fg = str(ch.fg) if ch.fg else "default"
                        bg = str(ch.bg) if ch.bg else "default"
                        if fg != "default" or bg != "default" or ch.bold or ch.italics or ch.underscore or ch.reverse:
                            nondefault += 1
                            styled.setdefault(str(y), {})[str(x)] = [
                                ch.data,
                                fg,
                                bg,
                                bool(ch.bold),
                            ]
            run.frames.append(
                {
                    "index": frame_index,
                    "label": label,
                    "cols": cols,
                    "rows": rows,
                    "t_ms": int((time.monotonic() - t_start) * 1000),
                    "grid_text": rows_txt,
                    "grid_sha256": hashlib.sha256(grid.encode("utf-8", "replace")).hexdigest(),
                    "trimmed_trailing_rows": trimmed_trailing,
                    "cursor": {
                        "x": cursor.x,
                        "y": cursor.y,
                        "hidden": bool(getattr(cursor, "hidden", False)),
                    },
                    "styled_cell_count": nondefault,
                    "styled_cells": styled,
                }
            )
            run.frame_labels.append(label)
            frame_index += 1

        def send(data: str) -> None:
            os.write(master, data.encode("utf-8", "replace"))

        exited_early = False
        debug_phases = os.environ.get("REFCAP_DEBUG") == "1"
        try:
            for step in steps:
                phase_t0 = time.monotonic()
                if "quiet" in step:
                    quiet(step["quiet"])
                elif "sleep" in step:
                    time.sleep(step["sleep"])
                    feed_pending()
                elif "send" in step:
                    send(step["send"])
                elif "frame" in step:
                    snapshot(step["frame"], styled_scan=step.get("styled", True))
                elif step.get("wait_exit"):
                    deadline = time.monotonic() + step.get("timeout", 6.0)
                    while time.monotonic() < deadline:
                        if proc.poll() is not None:
                            exited_early = True
                            break
                        time.sleep(0.05)
                    feed_pending()
                if debug_phases:
                    print(f"[capture][{scenario_id}] step={sorted(step)[0]} ms={int((time.monotonic()-phase_t0)*1000)}", flush=True)
        finally:
            finally_t0 = time.monotonic()
            exit_code = proc.poll()
            if exit_code is None:
                try:
                    os.killpg(proc.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    proc.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    try:
                        proc.wait(timeout=2.0)
                    except subprocess.TimeoutExpired:
                        pass
            # Drain teardown bytes (alt-screen leave etc.).
            drain_deadline = time.monotonic() + 1.5
            while time.monotonic() < drain_deadline:
                feed_pending()
                if proc.poll() is not None and time.monotonic() - last_chunk_at[0] > 0.3:
                    break
                time.sleep(0.05)
            feed_pending()
            try:
                os.close(master)
            except OSError:
                pass
            run.exit_code = proc.returncode if proc.returncode is not None and proc.returncode >= 0 else None
            run.exit_signal = -proc.returncode if proc.returncode is not None and proc.returncode < 0 else None
            run.total_ms = int((time.monotonic() - t_start) * 1000)
            run.raw = b"".join(d for _, d in chunks)
            run.ansi = scan_ansi_events(run.raw, chunks)
            run.notes = "exited_before_kill" if exited_early else "killed_after_script"
            if debug_phases:
                print(f"[capture][{scenario_id}] finally ms={int((time.monotonic()-finally_t0)*1000)}", flush=True)

        return run

    def snapshot_home(self, run: ScenarioRun, tails: list[str]) -> None:
        for path in sorted(self.home.rglob("*")):
            if path.is_file():
                rel = str(path.relative_to(self.home))
                try:
                    size = path.stat().st_size
                except OSError:
                    size = -1
                run.home_files_after[rel] = size
        for rel in tails:
            p = self.home / rel
            if p.is_file() and p.stat().st_size <= 65536:
                try:
                    run.file_tails[rel] = p.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    pass


_ANSI_PATTERNS = {
    "alt_screen_enter": rb"\x1b\[\?1049h",
    "alt_screen_leave": rb"\x1b\[\?1049l",
    "alt_screen_47_enter": rb"\x1b\[\?47h",
    "alt_screen_47_leave": rb"\x1b\[\?47l",
    "sync_begin": rb"\x1b\[\?2026h",
    "sync_end": rb"\x1b\[\?2026l",
    "cursor_hide": rb"\x1b\[\?25l",
    "cursor_show": rb"\x1b\[\?25h",
    "mouse_1000_on": rb"\x1b\[\?1000h",
    "mouse_1000_off": rb"\x1b\[\?1000l",
    "mouse_1002_on": rb"\x1b\[\?1002h",
    "mouse_1003_on": rb"\x1b\[\?1003h",
    "mouse_sgr_1006_on": rb"\x1b\[\?1006h",
    "mouse_1015_on": rb"\x1b\[\?1015h",
    "focus_tracking_on": rb"\x1b\[\?1004h",
    "focus_tracking_off": rb"\x1b\[\?1004l",
    "bracketed_paste_on": rb"\x1b\[\?2004h",
    "bracketed_paste_off": rb"\x1b\[\?2004l",
    "kitty_keyboard_query": rb"\x1b\[>0q",
    "osc_title": rb"\x1b\]0;",
    "osc_cursor_color": rb"\x1b\]12;",
    "osc_conemu_progress": rb"\x1b\]9;4;",
    "clear_screen": rb"\x1b\[2J",
}


def scan_ansi_events(raw: bytes, chunks: list[tuple[float, bytes]]) -> dict:
    out: dict[str, object] = {}
    cumulative = bytearray()
    first_t: dict[str, int] = {}
    for t_s, data in chunks:
        cumulative.extend(data)
        view = bytes(cumulative)
        for name, pat in _ANSI_PATTERNS.items():
            if name not in first_t:
                m = re.search(pat, view)
                if m:
                    first_t[name] = int(t_s * 1000)
    counts: Counter[str] = Counter()
    for name, pat in _ANSI_PATTERNS.items():
        counts[name] = len(re.findall(pat, raw))
    out["counts"] = dict(counts)
    out["first_t_ms"] = first_t
    out["sgr_count"] = len(re.findall(rb"\x1b\[[0-9;]*m", raw))
    out["cup_count"] = len(re.findall(rb"\x1b\[[0-9;]*H", raw))
    out["el_count"] = len(re.findall(rb"\x1b\[[0-9;]*K", raw))
    out["total_bytes"] = len(raw)
    return out


# ---------------------------------------------------------------------------
# Scenario scripts
# ---------------------------------------------------------------------------

B: str  # set at runtime


def interactive_steps_default(settle: float = 3.2) -> list[dict]:
    return [
        {"quiet": settle},
        {"frame": "settle"},
        {"sleep": 1.2},
        {"frame": "animated_second"},
        {"quiet": 0.5},
        {"frame": "pre_exit"},
    ]


def build_scenarios(binary: str, cols: int, rows: int) -> dict[str, dict]:
    """Scenario id -> spec. ``frames_map`` later maps family -> scenario ids."""
    ctrl_q = "\x11"
    esc = "\x1b"
    return {
        # --- interactive -------------------------------------------------
        "S01_startup_default": {
            "kind": "interactive",
            "argv": [binary],
            "steps": interactive_steps_default()
            + [{"send": ctrl_q}, {"wait_exit": True, "timeout": 6.0}],
            "tails": [".grok/config.toml", ".grok/active_sessions.json"],
        },
        "S02_minimal": {
            "kind": "interactive",
            "argv": [binary, "--minimal"],
            "steps": [
                {"quiet": 3.2},
                {"frame": "minimal_settle"},
                {"sleep": 1.0},
                {"frame": "minimal_second"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [".grok/config.toml"],
        },
        "S03_no_alt_screen": {
            "kind": "interactive",
            "argv": [binary, "--no-alt-screen"],
            "steps": [
                {"quiet": 3.2},
                {"frame": "inline_settle"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [],
        },
        "S04_fullscreen_80x24": {
            "kind": "interactive",
            "argv": [binary, "--fullscreen"],
            "cols": 80,
            "rows": 24,
            "steps": [
                {"quiet": 3.2},
                {"frame": "small_settle"},
                {"sleep": 1.0},
                {"frame": "small_second"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [],
        },
        "S05_prompt_typing": {
            "kind": "interactive",
            "argv": [binary],
            "steps": [
                {"quiet": 3.2},
                {"frame": "before_typing"},
                {"send": "hello parity"},
                {"sleep": 0.8},
                {"frame": "typed"},
                {"send": "\x7f\x7f"},
                {"sleep": 0.5},
                {"frame": "after_backspace"},
                {"send": "/"},
                {"sleep": 0.9},
                {"frame": "slash_menu"},
                {"send": esc},
                {"sleep": 0.4},
                {"send": "\x1b[200~pasted text\x1b[201~"},
                {"sleep": 0.7},
                {"frame": "after_paste"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [],
        },
        "S06_key_nav": {
            "kind": "interactive",
            "argv": [binary],
            "steps": [
                {"quiet": 3.2},
                {"frame": "nav_start"},
                {"send": "\t\t"},
                {"sleep": 0.5},
                {"frame": "after_tab"},
                {"send": "\x1b[A\x1b[B"},
                {"sleep": 0.5},
                {"frame": "after_arrows"},
                {"send": "\x10"},
                {"sleep": 0.9},
                {"frame": "after_ctrl_p"},
                {"send": esc},
                {"sleep": 0.4},
                {"frame": "after_esc"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [],
        },
        "S14_dashboard": {
            "kind": "interactive",
            "argv": [binary, "dashboard"],
            "steps": [
                {"quiet": 3.2},
                {"frame": "dashboard_settle"},
                {"sleep": 1.0},
                {"frame": "dashboard_second"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [],
        },
        "S19_welcome_keys": {
            "kind": "interactive",
            "argv": [binary],
            "steps": [
                {"quiet": 3.2},
                {"frame": "welcome_start"},
                {"send": "\x1b[B\x1b[B\x1b[A"},
                {"sleep": 0.5},
                {"frame": "after_menu_keys"},
                {"send": "\r"},
                {"sleep": 1.2},
                {"frame": "after_enter"},
                {"send": ctrl_q},
                {"wait_exit": True, "timeout": 6.0},
            ],
            "tails": [],
        },
        # --- cli ---------------------------------------------------------
        "C01_version": cli_spec([binary, "--version"]),
        "C02_version_sub": cli_spec([binary, "version"]),
        "C03_help": cli_spec([binary, "--help"]),
        "C04_help_sub": cli_spec([binary, "help"]),
        "C05_inspect": cli_spec([binary, "inspect"]),
        "C06_models": cli_spec([binary, "models"]),
        "C07_sessions_help": cli_spec([binary, "sessions"]),
        "C08_sessions_list": cli_spec([binary, "sessions", "list"]),
        "C09_sessions_search": cli_spec([binary, "sessions", "search", "parity"]),
        "C10_mcp_help": cli_spec([binary, "mcp"]),
        "C11_mcp_list": cli_spec([binary, "mcp", "list"]),
        "C12_mcp_doctor": cli_spec([binary, "mcp", "doctor"], timeout=25.0),
        "C13_plugin_help": cli_spec([binary, "plugin"]),
        "C14_plugin_list": cli_spec([binary, "plugin", "list"]),
        "C15_worktree_help": cli_spec([binary, "worktree"]),
        "C16_worktree_list": cli_spec([binary, "worktree", "list"], timeout=15.0),
        "C17_memory_help": cli_spec([binary, "memory"]),
        "C18_agent_help": cli_spec([binary, "agent", "--help"]),
        "C19_export_help": cli_spec([binary, "export", "--help"]),
        "C20_trace_help": cli_spec([binary, "trace", "--help"]),
        "C21_update_help": cli_spec([binary, "update", "--help"]),
        "C22_update_check": cli_spec([binary, "update", "--check", "--json"], timeout=20.0),
        "C23_login_help": cli_spec([binary, "login", "--help"]),
        "C24_logout_help": cli_spec([binary, "logout", "--help"]),
        "C25_completions_bash": cli_spec([binary, "completions", "bash"], rows=6000),
        "C26_dashboard_help": cli_spec([binary, "dashboard", "--help"]),
        "C27_wrap_help": cli_spec([binary, "wrap", "--help"]),
        "C28_setup_help": cli_spec([binary, "setup", "--help"]),
        "E01_bogus_flag": cli_spec([binary, "--bogus-flag"]),
        "E02_resume_missing": cli_spec(
            [binary, "--resume", "00000000-0000-0000-0000-000000000000"], timeout=15.0
        ),
        "E03_single_turn_no_auth": cli_spec([binary, "-p", "hello parity"], timeout=25.0),
        "E04_completions_bad_shell": cli_spec([binary, "completions", "notashell"]),
        "E05_sessions_delete_missing": cli_spec(
            [binary, "sessions", "delete", "00000000-0000-0000-0000-000000000000"], timeout=15.0
        ),
    }


def cli_spec(argv: list[str], timeout: float = 12.0, rows: int = 1000) -> dict:
    # Tall viewport so full CLI output fits one grid without scroll-off.
    return {
        "kind": "cli",
        "argv": argv,
        "cols": 120,
        "rows": rows,
        "steps": [
            {"wait_exit": True, "timeout": timeout},
            {"frame": "cli_final", "styled": False},
        ],
        "tails": [],
    }


# ---------------------------------------------------------------------------
# Family mapping + fixed invariants
# ---------------------------------------------------------------------------

FAMILY_SCENARIOS: dict[str, list[str]] = {
    "terminal-input-decoding": ["S05_prompt_typing", "S06_key_nav", "S01_startup_default"],
    "terminal-lifecycle-writer-cursor": ["S01_startup_default", "S02_minimal", "S03_no_alt_screen"],
    "deterministic-render-surfaces": ["S01_startup_default", "S04_fullscreen_80x24"],
    "scrollback-state": ["S02_minimal", "S03_no_alt_screen"],
    "prompt-editor-completions": ["S05_prompt_typing"],
    "layout-chrome-themes-responsive": ["S01_startup_default", "S04_fullscreen_80x24"],
    "action-effect-focus-overlay": ["S06_key_nav", "S19_welcome_keys"],
    "sessions-persistence-replay": ["C08_sessions_list", "C09_sessions_search", "C07_sessions_help", "S01_startup_default"],
    "prompt-queue-interjection-compaction-memory": ["C17_memory_help", "E03_single_turn_no_auth", "S05_prompt_typing"],
    "workspace-worktrees-trust-vcs-sandbox": ["C05_inspect", "C15_worktree_help", "C16_worktree_list"],
    "tools-permissions-scheduler-teams": ["C05_inspect", "C03_help", "C18_agent_help"],
    "local-hooks-mcp-acp-plugins-codegraph-lsp": ["C10_mcp_help", "C11_mcp_list", "C12_mcp_doctor", "C13_plugin_help", "C14_plugin_list"],
    "public-auth-providers-models-updates-sleepwake": ["S01_startup_default", "C06_models", "C23_login_help", "C24_logout_help", "C22_update_check", "E03_single_turn_no_auth"],
    "cli-config-settings-doctor-support": ["C01_version", "C03_help", "C05_inspect", "E01_bogus_flag", "E02_resume_missing", "S02_minimal"],
    "startup-welcome-trust-firstprompt": ["S01_startup_default", "S19_welcome_keys", "C05_inspect"],
    "shell-lifecycle-status-context-footer": ["S01_startup_default", "S02_minimal"],
    "transcript-blocks-tools-diffs-markdown-media": ["C19_export_help", "C20_trace_help", "C07_sessions_help"],
    "overlays-pickers-settings-permissions-questions": ["S06_key_nav", "S19_welcome_keys"],
    "plan-vim-minimal-inline-fullscreen-modes": ["S02_minimal", "S03_no_alt_screen", "S04_fullscreen_80x24", "C03_help"],
    "dashboard-queue-tasks-todo-subagents": ["S14_dashboard", "C26_dashboard_help", "C18_agent_help"],
    "notifications-tips-appearance-diagnostics": ["S01_startup_default", "C21_update_help", "C22_update_check"],
    "reference-registries": ["C03_help", "C06_models", "C07_sessions_help", "C10_mcp_help", "C13_plugin_help", "C15_worktree_help", "C17_memory_help", "C25_completions_bash", "C05_inspect"],
    "harness-architectural-invariants": ["S01_startup_default", "C07_sessions_help", "C08_sessions_list"],
}

# Observability notes recorded honestly per family.
FAMILY_OBSERVABILITY_NOTES: dict[str, dict] = {
    "prompt-queue-interjection-compaction-memory": {
        "observability": "partial",
        "blocked_surfaces": [
            "mid-turn queue drain and compaction checkpoints require an authenticated live turn (P8 environment-blocked)"
        ],
    },
    "transcript-blocks-tools-diffs-markdown-media": {
        "observability": "partial",
        "blocked_surfaces": [
            "live transcript block/diff/media rendering requires an authenticated provider turn (P8 environment-blocked); export/trace/sessions CLI surfaces are captured"
        ],
    },
    "overlays-pickers-settings-permissions-questions": {
        "observability": "partial",
        "blocked_surfaces": [
            "permission prompts and question overlays fire only mid-turn with an authenticated provider (P8 environment-blocked); focus/navigation key handling is captured"
        ],
    },
    "dashboard-queue-tasks-todo-subagents": {
        "observability": "partial",
        "blocked_surfaces": [
            "task/todo/subagent progress rows require live agent execution (P8 environment-blocked); dashboard startup view and agent registry surfaces are captured"
        ],
    },
    "harness-architectural-invariants": {
        "observability": "partial",
        "blocked_surfaces": [
            "reference-side invariants are observable as session persistence layout and replay CLI surfaces; the invariants *proven* belong to the harness implementation (retain-and-prove), not to reference capture"
        ],
    },
}


# ---------------------------------------------------------------------------
# Contract construction + evaluation
# ---------------------------------------------------------------------------


def family_frame_count(scenario_runs: dict[str, ScenarioRun], scenarios: list[str]) -> int:
    return sum(len(scenario_runs[s].frames) for s in scenarios if s in scenario_runs)


def build_contract(family_id: str, runs: dict[str, ScenarioRun], scenarios: list[str]) -> dict:
    """Derive the sealed contract from actual observations + fixed invariants."""
    scenarios = [s for s in scenarios if s in runs]
    # Scope runs to this family's scenario list: contract checks may only
    # reference scenarios present in the family's sealed observations.
    runs = {s: runs[s] for s in scenarios}
    text_checks: list[dict] = []
    exit_checks: dict[str, int | None] = {}
    ansi_checks: list[dict] = []
    side_effect_checks: list[dict] = []

    def grid_of(run: ScenarioRun, label: str) -> list[str]:
        for fr in run.frames:
            if fr["label"] == label:
                return fr["grid_text"]
        return run.frames[0]["grid_text"] if run.frames else []

    def has_text(rows: list[str], needle: str) -> bool:
        return any(needle in row for row in rows)

    def record_text(scenario: str, label: str, needle: str, required: bool) -> None:
        if scenario in runs:
            text_checks.append(
                {
                    "scenario": scenario,
                    "frame_label": label,
                    "substring": needle,
                    "observed_present": has_text(grid_of(runs[scenario], label), needle),
                    "required": required,
                }
            )

    # ---- fixed invariants per family (grounded in real probes of the binary) ----
    if family_id in ("terminal-lifecycle-writer-cursor", "deterministic-render-surfaces", "notifications-tips-appearance-diagnostics"):
        if "S01_startup_default" in runs:
            r = runs["S01_startup_default"]
            ansi_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "flag": "alt_screen_enter",
                    "min_count": 1,
                    "observed_count": r.ansi.get("counts", {}).get("alt_screen_enter", 0),
                }
            )
            ansi_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "flag": "alt_screen_leave",
                    "min_count": 1,
                    "observed_count": r.ansi.get("counts", {}).get("alt_screen_leave", 0),
                }
            )
            ansi_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "flag": "sync_begin",
                    "min_count": 5,
                    "observed_count": r.ansi.get("counts", {}).get("sync_begin", 0),
                }
            )
            ansi_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "flag": "cursor_hide",
                    "min_count": 1,
                    "observed_count": r.ansi.get("counts", {}).get("cursor_hide", 0),
                }
            )
    if family_id == "terminal-lifecycle-writer-cursor" and "S03_no_alt_screen" in runs:
        r = runs["S03_no_alt_screen"]
        ansi_checks.append(
            {
                "scenario": "S03_no_alt_screen",
                "flag": "alt_screen_enter",
                "max_count": 0,
                "observed_count": r.ansi.get("counts", {}).get("alt_screen_enter", 0),
            }
        )
    if family_id == "terminal-input-decoding" and "S01_startup_default" in runs:
        r = runs["S01_startup_default"]
        for flag in ("mouse_1000_on", "mouse_sgr_1006_on", "focus_tracking_on", "bracketed_paste_on", "kitty_keyboard_query"):
            ansi_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "flag": flag,
                    "min_count": 1,
                    "observed_count": r.ansi.get("counts", {}).get(flag, 0),
                }
            )

    if family_id in ("startup-welcome-trust-firstprompt", "shell-lifecycle-status-context-footer", "notifications-tips-appearance-diagnostics"):
        record_text("S01_startup_default", "settle", "~/work", True)
        for marker in ("Login with Grok", "Quit", "Type a message", "Grok Build", "Connecting..."):
            record_text("S01_startup_default", "settle", marker, False)
    if family_id == "startup-welcome-trust-firstprompt" and "C05_inspect" in runs:
        record_text("C05_inspect", "cli_final", "Project trusted:", True)
    if family_id == "tools-permissions-scheduler-teams":
        record_text("C05_inspect", "cli_final", "Permissions", True)
        record_text("C03_help", "cli_final", "--permission-mode", True)
        record_text("C03_help", "cli_final", "--always-approve", True)
        record_text("C18_agent_help", "cli_final", "agent", False)
    if family_id == "workspace-worktrees-trust-vcs-sandbox":
        record_text("C05_inspect", "cli_final", "Git root:", True)
        record_text("C05_inspect", "cli_final", "Project trusted:", True)
        record_text("C15_worktree_help", "cli_final", "List tracked worktrees", False)
        record_text("C15_worktree_help", "cli_final", "worktree", True)
    if family_id == "local-hooks-mcp-acp-plugins-codegraph-lsp":
        record_text("C10_mcp_help", "cli_final", "Manage MCP server", True)
        record_text("C11_mcp_list", "cli_final", "No MCP servers", False)
        record_text("C13_plugin_help", "cli_final", "marketplace", True)
        record_text("C14_plugin_list", "cli_final", "plugin", False)
    if family_id == "public-auth-providers-models-updates-sleepwake":
        record_text("C06_models", "cli_final", "not authenticated", True)
        record_text("C06_models", "cli_final", "grok-build", True)
        record_text("C23_login_help", "cli_final", "Sign in to Grok", True)
        record_text("C23_login_help", "cli_final", "device", False)
    if family_id == "cli-config-settings-doctor-support":
        record_text("C05_inspect", "cli_final", "Environment", True)
        record_text("E01_bogus_flag", "cli_final", "unexpected argument", True)
        record_text("E02_resume_missing", "cli_final", "Session does not exist", True)
        if "E01_bogus_flag" in runs:
            exit_checks["E01_bogus_flag"] = runs["E01_bogus_flag"].exit_code
        if "E02_resume_missing" in runs:
            exit_checks["E02_resume_missing"] = runs["E02_resume_missing"].exit_code
        if "C01_version" in runs:
            record_text("C01_version", "cli_final", "grok", True)
            exit_checks["C01_version"] = runs["C01_version"].exit_code
    if family_id == "sessions-persistence-replay":
        record_text("C07_sessions_help", "cli_final", "List, search, or restore sessions", True)
        record_text("C08_sessions_list", "cli_final", "No sessions found", False)
        record_text("C09_sessions_search", "cli_final", "session", False)
        if "S01_startup_default" in runs:
            side_effect_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "kind": "home_file_glob",
                    "glob": ".grok/sessions/session_search.sqlite*",
                    "min_matches": 1,
                    "observed_matches": sum(
                        1 for k in runs["S01_startup_default"].home_files_after if k.startswith(".grok/sessions/session_search.sqlite")
                    ),
                }
            )
            side_effect_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "kind": "home_file_exists",
                    "path": ".grok/active_sessions.json",
                    "observed_exists": ".grok/active_sessions.json" in runs["S01_startup_default"].home_files_after,
                }
            )
    if family_id == "harness-architectural-invariants":
        if "S01_startup_default" in runs:
            side_effect_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "kind": "home_file_exists",
                    "path": ".grok/active_sessions.json",
                    "observed_exists": ".grok/active_sessions.json" in runs["S01_startup_default"].home_files_after,
                }
            )
            side_effect_checks.append(
                {
                    "scenario": "S01_startup_default",
                    "kind": "home_file_glob",
                    "glob": ".grok/logs/*.jsonl",
                    "min_matches": 1,
                    "observed_matches": sum(
                        1 for k in runs["S01_startup_default"].home_files_after if k.startswith(".grok/logs/") and k.endswith(".jsonl")
                    ),
                }
            )
        record_text("C07_sessions_help", "cli_final", "List, search, or restore sessions", True)
    if family_id == "reference-registries":
        record_text("C03_help", "cli_final", "Grok Build TUI", True)
        for sub in ("agent", "completions", "dashboard", "export", "inspect", "login", "mcp", "memory", "models", "plugin", "sessions", "worktree"):
            record_text("C03_help", "cli_final", sub, True)
        record_text("C06_models", "cli_final", "Default model:", True)
        record_text("C07_sessions_help", "cli_final", "list", True)
        record_text("C07_sessions_help", "cli_final", "search", True)
        record_text("C07_sessions_help", "cli_final", "delete", True)
        record_text("C10_mcp_help", "cli_final", "doctor", True)
        record_text("C13_plugin_help", "cli_final", "uninstall", True)
        record_text("C15_worktree_help", "cli_final", "gc", True)
        record_text("C25_completions_bash", "cli_final", "_grok()", True)
        record_text("C05_inspect", "cli_final", "Agents", True)
        record_text("C05_inspect", "cli_final", "explore", True)
    if family_id == "plan-vim-minimal-inline-fullscreen-modes":
        record_text("C03_help", "cli_final", "--minimal", True)
        record_text("C03_help", "cli_final", "--fullscreen", True)
        record_text("C03_help", "cli_final", "--no-alt-screen", True)
        record_text("C03_help", "cli_final", "--no-plan", True)
        if "S02_minimal" in runs and ".grok/config.toml" in runs["S02_minimal"].file_tails:
            side_effect_checks.append(
                {
                    "scenario": "S02_minimal",
                    "kind": "file_contains",
                    "path": ".grok/config.toml",
                    "substring": "minimal",
                    "observed_contains": "minimal" in runs["S02_minimal"].file_tails[".grok/config.toml"],
                }
            )
    if family_id == "prompt-editor-completions" and "S05_prompt_typing" in runs:
        typed = grid_of(runs["S05_prompt_typing"], "typed")
        echoed = has_text(typed, "hello parity") or has_text(typed, "hello") or has_text(typed, "parity")
        text_checks.append(
            {
                "scenario": "S05_prompt_typing",
                "frame_label": "typed",
                "substring": "hello parity",
                "observed_present": echoed,
                "required": False,
                "note": "input echo depends on leader connection state; observed value sealed honestly",
            }
        )
    if family_id == "scrollback-state":
        record_text("C03_help", "cli_final", "--minimal", False)
        # minimal mode contract: alt-screen use differs; sealed from counts.
        if "S02_minimal" in runs:
            r = runs["S02_minimal"]
            ansi_checks.append(
                {
                    "scenario": "S02_minimal",
                    "flag": "alt_screen_enter",
                    "note": "minimal/scrollback mode alt-screen policy sealed from observed count",
                    "observed_count": r.ansi.get("counts", {}).get("alt_screen_enter", 0),
                }
            )
    if family_id == "layout-chrome-themes-responsive" and "S04_fullscreen_80x24" in runs:
        r = runs["S04_fullscreen_80x24"]
        dims_ok = bool(r.frames) and r.frames[0]["cols"] == 80 and r.frames[0]["rows"] == 24
        ansi_checks.append(
            {
                "scenario": "S04_fullscreen_80x24",
                "flag": "_frame_dimensions_80x24",
                "frame_dims": [80, 24],
                "min_count": 1 if dims_ok else 0,
                "observed_count": 1 if dims_ok else 0,
            }
        )

    # sealed exit-code facts for every scenario in scope
    for s in scenarios:
        if s in runs and s not in exit_checks:
            run = runs[s]
            if run.exit_code is not None:
                exit_checks[s] = run.exit_code
            elif run.exit_signal is not None:
                exit_checks[s] = -run.exit_signal
            else:
                exit_checks[s] = None

    contract = {
        "schema_version": CONTRACT_SCHEMA,
        "family_id": family_id,
        "reference_epoch": REFERENCE_EPOCH,
        "bound_to_reference_epoch": REFERENCE_EPOCH,
        "scenarios": scenarios,
        "min_scenario_frames": {s: len(runs[s].frames) for s in scenarios if s in runs},
        "expected": {
            "text_substrings": text_checks,
            "ansi_lifecycle": ansi_checks,
            "exit_states": exit_checks,
            "side_effects": side_effect_checks,
        },
        "volatile": {
            "source": "frame-diff between settle and animated_second/pre_exit frames",
            "cells_sealed_in": "observations.json scenario frames + volatile_cells",
        },
        "derivation": "sealed from observations captured by scripts/tui-parity/capture-reference-observations.py; required=false entries record honest observed state for surfaces blocked by missing leader daemon / live provider",
    }
    return contract


def evaluate_contract(observations: dict, contract: dict, expected_epoch: str | None = None) -> list[str]:
    """Return a list of contract failures (empty == pass).

    Semantics are mirrored by scripts/parity_task_qa.py
    validate-reference-observations; keep them in sync. When expected_epoch is
    given, every epoch binding must equal it (catches consistent-but-unbound
    epoch tampering).
    """
    failures: list[str] = []
    if observations.get("reference_epoch") != contract.get("reference_epoch"):
        failures.append("epoch mismatch between observations and contract")
    if observations.get("family_id") != contract.get("family_id"):
        failures.append("family_id mismatch between observations and contract")
    if observations.get("bound_to_reference_epoch") != contract.get("bound_to_reference_epoch"):
        failures.append("bound_to_reference_epoch mismatch")
    if expected_epoch is not None:
        for field in ("reference_epoch", "bound_to_reference_epoch"):
            if observations.get(field) != expected_epoch:
                failures.append(f"observations.{field} not bound to expected epoch")
            if contract.get(field) != expected_epoch:
                failures.append(f"contract.{field} not bound to expected epoch")

    scenario_frames: dict[str, list[dict]] = {
        s["id"]: s.get("frames", []) for s in observations.get("scenarios", [])
    }
    for s, min_frames in (contract.get("min_scenario_frames") or {}).items():
        have = len(scenario_frames.get(s, []))
        if have < min_frames:
            failures.append(f"scenario {s}: expected >= {min_frames} frames, observed {have}")

    # Cell-grid integrity: every sealed frame's hash must match its text. This
    # makes any cell tamper detectable even for families with few text checks.
    for s in observations.get("scenarios", []):
        for fr in s.get("frames", []):
            grid = "\n".join(fr.get("grid_text", []))
            recomputed = hashlib.sha256(grid.encode("utf-8", "replace")).hexdigest()
            if recomputed != fr.get("grid_sha256"):
                failures.append(
                    f"grid hash mismatch: scenario={s['id']} frame={fr.get('label')} sealed={str(fr.get('grid_sha256'))[:12]} recomputed={recomputed[:12]}"
                )

    for check in contract.get("expected", {}).get("text_substrings", []):
        frames = scenario_frames.get(check["scenario"], [])
        target = None
        for fr in frames:
            if fr.get("label") == check.get("frame_label"):
                target = fr
                break
        if target is None and frames:
            target = frames[0]
        grid = " ".join(target.get("grid_text", [])) if target else ""
        present = check["substring"] in grid
        if check.get("required") and not present:
            failures.append(
                f"required text missing: scenario={check['scenario']} label={check.get('frame_label')} substring={check['substring']!r}"
            )
        if "observed_present" in check and present != check["observed_present"]:
            failures.append(
                f"sealed observed_present drift: scenario={check['scenario']} substring={check['substring']!r} sealed={check['observed_present']} actual={present}"
            )

    # ANSI lifecycle checks need the sealed counts from observations.
    ansi_by_scenario = {
        s["id"]: (s.get("ansi", {}) or {}).get("counts", {}) for s in observations.get("scenarios", [])
    }
    for check in contract.get("expected", {}).get("ansi_lifecycle", []):
        flag = check["flag"]
        if flag.startswith("_frame_dimensions_"):
            dims = check.get("frame_dims") or [0, 0]
            frames = scenario_frames.get(check.get("scenario", ""), [])
            dims_ok = bool(frames) and frames[0].get("cols") == dims[0] and frames[0].get("rows") == dims[1]
            observed = 1 if dims_ok else 0
        else:
            counts = ansi_by_scenario.get(check["scenario"], {})
            observed = counts.get(flag, 0)
        if "min_count" in check and observed < check["min_count"]:
            failures.append(
                f"ansi flag {check['flag']} below min in {check['scenario']}: {observed} < {check['min_count']}"
            )
        if "max_count" in check and observed > check["max_count"]:
            failures.append(
                f"ansi flag {check['flag']} above max in {check['scenario']}: {observed} > {check['max_count']}"
            )
        if "observed_count" in check and observed != check["observed_count"]:
            failures.append(
                f"ansi sealed-count drift for {check['flag']} in {check['scenario']}: sealed={check['observed_count']} actual={observed}"
            )

    exit_by_scenario = {s["id"]: s.get("exit", {}).get("code") for s in observations.get("scenarios", [])}
    for scenario, sealed in (contract.get("expected", {}).get("exit_states") or {}).items():
        actual = exit_by_scenario.get(scenario)
        if sealed is not None and actual != sealed:
            failures.append(f"exit state drift for {scenario}: sealed={sealed} actual={actual}")

    home_files_by_scenario: dict[str, dict] = {}
    file_tails_by_scenario: dict[str, dict] = {}
    for s in observations.get("scenarios", []):
        home_files_by_scenario[s["id"]] = s.get("home_files_after", {}) or {}
        file_tails_by_scenario[s["id"]] = s.get("file_tails", {}) or {}
    for check in contract.get("expected", {}).get("side_effects", []):
        kind = check["kind"]
        files = home_files_by_scenario.get(check["scenario"], {})
        if kind == "home_file_exists":
            present = check["path"] in files
            if not present:
                failures.append(f"side-effect file missing: {check['path']} after {check['scenario']}")
            if "observed_exists" in check and present != check["observed_exists"]:
                failures.append(
                    f"side-effect sealed drift for {check['path']}: sealed={check['observed_exists']} actual={present}"
                )
        elif kind == "home_file_glob":
            prefix = check["glob"].split("*")[0]
            count = sum(1 for k in files if k.startswith(prefix))
            if "min_matches" in check and count < check["min_matches"]:
                failures.append(
                    f"side-effect glob {check['glob']} under min after {check['scenario']}: {count} < {check['min_matches']}"
                )
            if "observed_matches" in check and count != check["observed_matches"]:
                failures.append(
                    f"side-effect glob sealed drift {check['glob']}: sealed={check['observed_matches']} actual={count}"
                )
        elif kind == "file_contains":
            content = file_tails_by_scenario.get(check["scenario"], {}).get(check["path"], "")
            present = check["substring"] in content
            if not present:
                failures.append(
                    f"side-effect file {check['path']} missing substring {check['substring']!r} after {check['scenario']}"
                )
            if "observed_contains" in check and present != check["observed_contains"]:
                failures.append(
                    f"side-effect contains sealed drift {check['path']}: sealed={check['observed_contains']} actual={present}"
                )
    return failures


# ---------------------------------------------------------------------------
# Mutations + RED receipt
# ---------------------------------------------------------------------------


def build_mutations(family_id: str, observations: dict, contract: dict) -> dict:
    """Apply each mutant in-memory; require contract failure (RED receipt)."""
    mutants: list[dict] = []

    def run_mutant(mid: str, description: str, mutate) -> dict:
        mutated_obs = copy.deepcopy(observations)
        mutated_contract = copy.deepcopy(contract)
        mutate(mutated_obs, mutated_contract)
        failures = evaluate_contract(mutated_obs, mutated_contract, REFERENCE_EPOCH)
        return {
            "id": mid,
            "description": description,
            "detection": "contract self-check must fail on mutated observations/contract",
            "expected_detection": True,
            "detected": bool(failures),
            "sample_failures": failures[:2],
        }

    def blank_first_frame(obs: dict, _c: dict) -> None:
        scen = obs["scenarios"][0]
        if scen["frames"]:
            fr = scen["frames"][0]
            fr["grid_text"] = [" " * fr["cols"]] * fr["rows"]
            fr["styled_cells"] = {}
            fr["styled_cell_count"] = 0

    def truncate_frames(obs: dict, _c: dict) -> None:
        obs["scenarios"][0]["frames"] = []

    def drop_exit_state(_obs: dict, con: dict) -> None:
        states = con["expected"]["exit_states"]
        if states:
            key = next(iter(states))
            states[key] = 99 if states[key] != 99 else 98

    def unbind_epoch(obs: dict, con: dict) -> None:
        obs["bound_to_reference_epoch"] = "unbound"
        con["bound_to_reference_epoch"] = "unbound"

    def flip_ansi_seal(_obs: dict, con: dict) -> None:
        checks = con["expected"]["ansi_lifecycle"]
        for ch in checks:
            if "observed_count" in ch:
                ch["observed_count"] = ch["observed_count"] + 5
                break
        else:
            con["expected"]["ansi_lifecycle"].append(
                {"scenario": con["scenarios"][0], "flag": "alt_screen_enter", "min_count": 999, "observed_count": 999}
            )

    def drop_side_effect(obs: dict, _c: dict) -> None:
        for scen in obs["scenarios"]:
            scen["home_files_after"] = {}
            scen["file_tails"] = {}

    def tamper_cell_text(obs: dict, _c: dict) -> None:
        # Flip a character without updating grid_sha256: the cell-integrity
        # check in evaluate_contract must catch it for every family.
        for scen in obs["scenarios"]:
            for fr in scen["frames"]:
                rows = fr.get("grid_text")
                if rows:
                    rows[0] = ("X" + rows[0][1:]) if len(rows[0]) > 0 else "X"
                    return

    checks = contract.get("expected", {})
    mutant_defs = [
        ("blank-startup-frame", "erase every cell of the first sealed frame", blank_first_frame),
        ("truncate-frames", "remove all frames from the first scenario", truncate_frames),
        ("exit-state-drift", "seal a different exit code for a scenario", drop_exit_state),
        ("unbound-epoch", "unbind reference_epoch from observations+contract", unbind_epoch),
        ("ansi-seal-tamper", "drift a sealed ANSI lifecycle count", flip_ansi_seal),
        ("grid-text-tamper", "alter frame cells without updating the sealed grid hash", tamper_cell_text),
    ]
    if checks.get("side_effects"):
        mutant_defs.append(("side-effect-tamper", "erase sealed side-effect files", drop_side_effect))

    for mid, description, fn in mutant_defs:
        mutants.append(run_mutant(mid, description, fn))

    all_detected = all(m["detected"] for m in mutants)
    return {
        "schema_version": MUTATION_SCHEMA,
        "family_id": family_id,
        "reference_epoch": REFERENCE_EPOCH,
        "bound_to_reference_epoch": REFERENCE_EPOCH,
        "mutations": mutants,
        "red_receipt": {
            "method": "each mutant applied to an in-memory deepcopy of the sealed observations/contract; evaluate_contract() must return >= 1 failure",
            "evaluator": "scripts/tui-parity/capture-reference-observations.py::evaluate_contract (mirrored by scripts/parity_task_qa.py validate-reference-observations)",
            "all_detected": all_detected,
            "mutant_count": len(mutants),
        },
    }


# ---------------------------------------------------------------------------
# Driver main
# ---------------------------------------------------------------------------


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def build_observations(
    family_id: str,
    scenarios: list[str],
    runs: dict[str, ScenarioRun],
    binary_proof: dict,
    run_meta: dict,
) -> dict:
    import pyte  # noqa: F401  (proves capture-time dependency)

    scen_docs = []
    for s in scenarios:
        if s not in runs:
            continue
        r = runs[s]
        frames = []
        for fr in r.frames:
            frames.append(
                {
                    "index": fr["index"],
                    "label": fr["label"],
                    "cols": fr["cols"],
                    "rows": fr["rows"],
                    "t_ms": fr["t_ms"],
                    "grid_text": fr["grid_text"],
                    "grid_sha256": fr["grid_sha256"],
                    "cursor": fr["cursor"],
                    "styled_cell_count": fr["styled_cell_count"],
                    "styled_cells": fr["styled_cells"],
                }
            )
        # volatile cells between first and last frame
        volatile_cells: list[list[int]] = []
        if len(frames) >= 2:
            a = frames[0]["grid_text"]
            b = frames[-1]["grid_text"]
            for y, (ra, rb) in enumerate(zip(a, b)):
                for x, (ca, cb) in enumerate(zip(ra, rb)):
                    if ca != cb:
                        volatile_cells.append([y, x])
        scen_docs.append(
            {
                "id": s,
                "kind": r.kind,
                "argv": [a if a == str(BINARY) else a for a in r.argv],
                "cols": r.cols,
                "rows": r.rows,
                "exit": {"code": r.exit_code, "signal": r.exit_signal},
                "total_ms": r.total_ms,
                "notes": r.notes,
                "ansi": r.ansi,
                "frames": frames,
                "volatile_cell_count": len(volatile_cells),
                "volatile_cells_sample": volatile_cells[:200],
                "raw_dump": f"_raw/{s}.pty.bin",
                "home_files_after": r.home_files_after,
                "file_tails": r.file_tails,
            }
        )
    notes = FAMILY_OBSERVABILITY_NOTES.get(family_id, {"observability": "full", "blocked_surfaces": []})
    return {
        "schema_version": OBSERVATIONS_SCHEMA,
        "family_id": family_id,
        "reference_epoch": REFERENCE_EPOCH,
        "bound_to_reference_epoch": REFERENCE_EPOCH,
        "captured_at": run_meta["captured_at"],
        "binary": binary_proof,
        "capture_toolchain": run_meta["toolchain"],
        "environment": run_meta["environment"],
        "observability": notes["observability"],
        "blocked_surfaces": notes["blocked_surfaces"],
        "scenarios": scen_docs,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--binary", type=Path, default=BINARY)
    parser.add_argument("--only", default="", help="comma-separated family ids")
    parser.add_argument("--cols", type=int, default=120)
    parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--dry-run", action="store_true", help="print scenario plan and exit")
    args = parser.parse_args()

    faulthandler.enable()
    faulthandler.register(signal.SIGUSR1, all_threads=True)

    _ensure_pyte()  # re-execs when pyte is missing

    binary = args.binary.resolve()
    if not binary.exists():
        print(json.dumps({"verdict": "rejected", "reason": f"reference binary missing: {binary}"}))
        return 1
    binary_sha = sha256_file(binary)
    if binary_sha != EXPECTED_BINARY_SHA256:
        print(
            json.dumps(
                {
                    "verdict": "rejected",
                    "reason": "reference binary sha256 mismatch (frozen digest violated)",
                    "expected": EXPECTED_BINARY_SHA256,
                    "actual": binary_sha,
                }
            )
        )
        return 1

    families = (
        [f.strip() for f in args.only.split(",") if f.strip()] if args.only else list(FAMILY_SCENARIOS)
    )
    needed_scenarios: list[str] = []
    for fam in families:
        for s in FAMILY_SCENARIOS.get(fam, []):
            if s not in needed_scenarios:
                needed_scenarios.append(s)

    if args.dry_run:
        print(json.dumps({"families": families, "scenarios": needed_scenarios}, indent=2))
        return 0

    # version probe (non-PTY, cheap, for provenance)
    version_out = subprocess.run(
        [str(binary), "--version"], capture_output=True, text=True, timeout=30, env={"PATH": "/usr/bin:/bin"}
    ).stdout.strip()

    # Isolated home shared across all scenarios so side effects accumulate.
    home_root = Path(tempfile.mkdtemp(prefix="grok-refcap-home-"))
    (home_root / "tmp").mkdir()
    work = home_root / "work"
    work.mkdir()
    subprocess.run(["git", "init", "-q", str(work)], capture_output=True, check=False)
    subprocess.run(
        ["git", "-C", str(work), "commit", "--allow-empty", "-q", "-m", "refcap fixture", "--author", "refcap <refcap@localhost>"],
        capture_output=True,
        check=False,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
    )

    run_meta = {
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "toolchain": {
            "driver": "scripts/tui-parity/capture-reference-observations.py",
            "driver_sha256": sha256_file(Path(__file__)),
            "python": sys.version.split()[0],
            "vt_parser": "pyte",
        },
        "environment": {
            "cols_default": args.cols,
            "rows_default": args.rows,
            "TERM": "xterm-256color",
            "TZ": "UTC",
            "LANG": "C.UTF-8",
            "home_isolated": str(home_root),
            "work_isolated": str(work),
            "network": "blocked via 127.0.0.1:9 proxy env (hermetic, deterministic offline errors)",
        },
    }
    binary_proof = {
        "path": str(binary.relative_to(REPO_ROOT)),
        "sha256": binary_sha,
        "frozen_sha256_expected": EXPECTED_BINARY_SHA256,
        "version": version_out,
    }

    specs = build_scenarios(str(binary), args.cols, args.rows)
    runner = PtyRunner(home_root, work, args.cols, args.rows)
    runs: dict[str, ScenarioRun] = {}
    raw_dir = args.root / "_raw"
    raw_dir.mkdir(parents=True, exist_ok=True)
    for sid in needed_scenarios:
        spec = specs[sid]
        print(f"[capture] {sid} :: {' '.join(spec['argv'][-2:])}", flush=True)
        run = runner.run(
            sid,
            spec["argv"],
            spec["kind"],
            spec["steps"],
            cols=spec.get("cols"),
            rows=spec.get("rows"),
        )
        runner.snapshot_home(run, spec.get("tails") or [])
        (raw_dir / f"{sid}.pty.bin").write_bytes(run.raw)
        runs[sid] = run
        print(f"[capture] {sid} done :: exit={run.exit_code} frames={len(run.frames)} bytes={len(run.raw)} {run.total_ms:.0f}ms", flush=True)

    # ---- orphan leader sweep (pattern-scoped to the isolated socket) ----
    sock = str(home_root / "leader.sock")
    subprocess.run(["pkill", "-f", sock], capture_output=True, check=False)

    manifest_suggestions: dict[str, dict] = {}
    for fam in families:
        scenarios = FAMILY_SCENARIOS[fam]
        obs = build_observations(fam, scenarios, runs, binary_proof, run_meta)
        contract = build_contract(fam, runs, scenarios)
        failures = evaluate_contract(obs, contract, REFERENCE_EPOCH)
        if failures:
            print(f"[capture] {fam}: CONTRACT SELF-CHECK FAILED -> not sealing: {failures[:3]}", flush=True)
            manifest_suggestions[fam] = {
                "capture_state": "not_captured",
                "status": "incomplete",
                "reason": f"contract self-check failed: {failures[:2]}",
                "artifacts_present": [],
            }
            continue
        mutations = build_mutations(fam, obs, contract)
        if not mutations["red_receipt"]["all_detected"]:
            undetected = [m["id"] for m in mutations["mutations"] if not m["detected"]]
            print(f"[capture] {fam}: RED RECEIPT INCOMPLETE -> not sealing (undetected: {undetected})", flush=True)
            manifest_suggestions[fam] = {
                "capture_state": "not_captured",
                "status": "incomplete",
                "reason": f"red receipt incomplete: undetected mutants {undetected}",
                "artifacts_present": [],
            }
            continue
        fam_dir = args.root / fam
        fam_dir.mkdir(parents=True, exist_ok=True)
        (fam_dir / "observations.json").write_text(json.dumps(obs, indent=1, ensure_ascii=False))
        (fam_dir / "contract.json").write_text(json.dumps(contract, indent=1, ensure_ascii=False))
        (fam_dir / "mutation.json").write_text(json.dumps(mutations, indent=1, ensure_ascii=False))
        manifest_suggestions[fam] = {
            "capture_state": "captured",
            "status": "complete",
            "artifacts_present": ["observations.json", "contract.json", "mutation.json"],
            "observability": obs["observability"],
            "scenario_count": len(obs["scenarios"]),
            "red_receipt_all_detected": True,
        }
        print(f"[capture] {fam}: complete ({len(obs['scenarios'])} scenarios, observability={obs['observability']})", flush=True)

    capture_run = {
        "schema_version": "clean-room-parity-task-8-capture-run/v1",
        "reference_epoch": REFERENCE_EPOCH,
        "binary": binary_proof,
        "run_meta": run_meta,
        "scenarios_executed": sorted(runs),
        "scenario_summary": {
            sid: {
                "exit_code": r.exit_code,
                "exit_signal": r.exit_signal,
                "frames": len(r.frames),
                "bytes": len(r.raw),
                "total_ms": r.total_ms,
            }
            for sid, r in runs.items()
        },
        "manifest_suggestions": manifest_suggestions,
    }
    (args.root / "capture-run.json").write_text(json.dumps(capture_run, indent=1, ensure_ascii=False))
    print(
        json.dumps(
            {
                "verdict": "seal-complete",
                "root": str(args.root),
                "families": {k: v["status"] if "status" in v else "incomplete" for k, v in manifest_suggestions.items()},
            },
            indent=1,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
