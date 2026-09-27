#!/usr/bin/env python3
"""Check real termios restoration after normal exit and terminal initialization failures."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time


def attach_terminal():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


def check(binary, scenario):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    before = termios.tcgetattr(slave)
    raw = bytearray()
    with tempfile.TemporaryDirectory(prefix="harness-restore-") as root:
        env = {key: os.environ[key] for key in ("PATH", "HOME", "USER", "LANG") if key in os.environ}
        env.update(TERM="xterm-256color", COLORTERM="truecolor", HARNESS_DISABLE_ANIMATIONS="1")
        if scenario in ("telemetry_failure", "handoff_failure"):
            blocker = Path(root) / "not-a-directory"
            blocker.write_text("fixture\n")
            key = "HARNESS_RESTORE_TRACE" if scenario == "handoff_failure" else "HARNESS_TUI_PRESENTATION_TRACE"
            env[key] = str(blocker / "trace.json")
        with open("/dev/full", "wb", buffering=0) as full:
            child = subprocess.Popen([str(binary), "handoff_failure" if scenario == "handoff_failure" else "idle"], stdin=slave,
                stdout=full if scenario == "output_failure" else slave, stderr=slave,
                cwd=root, env=env, preexec_fn=attach_terminal)
            start = time.monotonic()
            sent = False
            try:
                while child.poll() is None:
                    if time.monotonic() - start > 10:
                        raise RuntimeError(f"{scenario}: child failed to exit")
                    if select.select([master], [], [], .01)[0]:
                        raw.extend(os.read(master, 65536))
                    if scenario in ("normal", "handoff_failure") and not sent and b"\x1b[?2026l" in raw:
                        os.write(master, b"\x11\x11")
                        sent = True
                while select.select([master], [], [], 0)[0]:
                    raw.extend(os.read(master, 65536))
                after = termios.tcgetattr(slave)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
                os.close(master)
                os.close(slave)
    flags = lambda attrs: [attrs[index] for index in range(6)]
    restored = before == after
    return {"scenario": scenario, "exit_code": child.returncode, "termios_restored": restored,
            "flags_before": flags(before), "flags_after": flags(after),
            "control_chars_restored": before[6] == after[6],
            "protocol_exit": {"alternate_screen_left": b"\x1b[?1049l" in raw,
                              "paste_disabled": b"\x1b[?2004l" in raw,
                              "mouse_disabled": b"\x1b[?1000l" in raw}}, raw


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--record-reference-defects", action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    reports = []
    for scenario in ("normal", "telemetry_failure", "output_failure", "handoff_failure"):
        result, raw = check(args.binary.resolve(), scenario)
        reports.append(result)
        (args.output / f"{scenario}.ansi").write_bytes(raw)
        print(json.dumps(result), flush=True)
    (args.output / "restoration.json").write_text(json.dumps(reports, indent=2) + "\n")
    assert reports[0]["exit_code"] == 0 and reports[0]["termios_restored"]
    assert all(reports[0]["protocol_exit"].values())
    if not args.record_reference_defects:
        assert all(result["termios_restored"] for result in reports), "terminal restoration failed"
        assert all(result["exit_code"] != 0 for result in reports[1:]), "failure injection did not fail"
        for result in (reports[1], reports[3]):
            assert all(result["protocol_exit"].values()), "initialization failure left terminal protocols active"


if __name__ == "__main__":
    main()
