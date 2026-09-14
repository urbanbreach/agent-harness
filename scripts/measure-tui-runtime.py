#!/usr/bin/env python3
"""Measure an offline resource_probe executable through a real PTY (no display emulator)."""
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


def resources(pid):
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    status = Path(f"/proc/{pid}/status").read_text().splitlines()
    rss = int(next(line for line in status if line.startswith("VmRSS:")).split()[1])
    return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK"), rss


def attach_terminal():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


def measure(binary, scenario, seconds, cadence):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 48, 160, 0, 0))
    env = {**os.environ, "TERM": "xterm-256color", "COLORTERM": "truecolor"}
    for name in ("NO_COLOR", "HARNESS_TUI_PRESENTATION_TRACE", "HARNESS_TUI_SCHEDULING_TRACE", "HARNESS_TUI_SCHEDULING_READINESS", "HARNESS_TUI_MIN_DRAW_MS"):
        env.pop(name, None)
    if cadence is not None:
        env["HARNESS_TUI_MIN_DRAW_MS"] = str(cadence)
    with tempfile.TemporaryDirectory(prefix="harness-runtime-perf-") as directory:
        child = subprocess.Popen([str(binary), "burst" if scenario == "slow-burst" else scenario],
                                 stdin=slave, stdout=slave, stderr=slave, cwd=directory, env=env,
                                 preexec_fn=attach_terminal)
        os.close(slave)
        start = time.monotonic()
        measured_at = start + 1.5
        end = measured_at + seconds
        next_input = measured_at
        next_read = start
        before = None
        timestamps = []
        byte_count = 0
        tail = b""
        last_output = b""
        sequence = 0
        try:
            while time.monotonic() < end:
                now = time.monotonic()
                if child.poll() is not None:
                    raise RuntimeError(f"probe exited {child.returncode}: {last_output!r}")
                if before is None and now >= measured_at:
                    before = resources(child.pid)
                if scenario == "typing" and now >= next_input:
                    os.write(master, b"x" if sequence % 128 != 127 else b"\x15")
                    sequence += 1
                    next_input = now + 0.001
                if now >= next_read and select.select([master], [], [], 0.001)[0]:
                    slow = scenario == "slow-burst" and now >= measured_at
                    chunk = os.read(master, 256 if slow else 65536)
                    last_output = (last_output + chunk)[-1000:]
                    framed = tail + chunk
                    if now >= measured_at:
                        timestamps.extend([now] * framed.count(b"\x1b[?2026l"))
                        byte_count += len(chunk)
                    tail = framed[-7:]
                    next_read = now + (0.05 if slow else 0)
                else:
                    time.sleep(0.0005)
            after = resources(child.pid)
            # Stop only this offline fixture; its PTY and workspace are disposable.
            child.terminate()
            child.wait(timeout=2)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            os.close(master)
    intervals = sorted((right - left) * 1000 for left, right in zip(timestamps, timestamps[1:]))
    if scenario in ("typing", "burst", "slow-burst") and not timestamps:
        raise RuntimeError(f"{scenario} produced no visible frames")
    return {"scenario": scenario, "seconds": seconds, "cadence_ms": cadence,
            "cpu_percent_one_core": (after[0] - before[0]) / seconds * 100,
            "rss_before_kib": before[1], "rss_after_kib": after[1],
            "frames": len(timestamps), "frames_per_second": len(timestamps) / seconds,
            "bytes": byte_count, "input_events": sequence,
            "frame_interval_p95_ms": intervals[min(len(intervals) - 1, int(len(intervals) * .95))] if intervals else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--seconds", type=float, default=4)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--cadence", type=int)
    args = parser.parse_args()
    if args.seconds <= 0 or args.repetitions < 1:
        parser.error("seconds and repetitions must be positive")
    samples = []
    for scenario in ("idle", "startup", "typing", "burst", "slow-burst"):
        for _ in range(args.repetitions):
            result = measure(args.binary.resolve(), scenario, args.seconds, args.cadence)
            samples.append(result)
            print(json.dumps(result), flush=True)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(samples, indent=2) + "\n")


if __name__ == "__main__":
    main()
