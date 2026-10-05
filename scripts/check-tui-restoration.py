#!/usr/bin/env python3
"""Check real termios restoration after normal exit and terminal initialization failures."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import socket
import struct
import subprocess
import tempfile
import termios
import time


def attach_terminal():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


def attach_reader_terminal():
    attach_terminal()
    # Exercise reader EOF handling, independently of the default SIGHUP action.
    signal.signal(signal.SIGHUP, signal.SIG_IGN)


def thread_switches(pid):
    result = {}
    for task in Path(f"/proc/{pid}/task").iterdir():
        fields = dict(line.split(":", 1) for line in (task / "status").read_text().splitlines())
        result[task.name] = int(fields["voluntary_ctxt_switches"])
    assert result, "thread accounting unavailable"
    return result


def check_reader(binary, scenario):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    before = termios.tcgetattr(slave)
    raw = bytearray()
    report = {"scenario": scenario}
    with tempfile.TemporaryDirectory(prefix="harness-reader-") as root:
        address = str(Path(root) / "control.sock")
        env = {key: os.environ[key] for key in ("PATH", "HOME", "USER", "LANG") if key in os.environ}
        env.update(TERM="xterm-256color", COLORTERM="truecolor", HARNESS_DISABLE_ANIMATIONS="1")
        error_path = Path(root) / "stderr.log"
        with error_path.open("wb") as errors:
            child = subprocess.Popen([str(binary), address], stdin=slave, stdout=slave,
                stderr=errors if scenario == "hangup" else slave,
                cwd=root, env=env, preexec_fn=attach_reader_terminal)
        control = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)

        def drain(seconds):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline and child.poll() is None:
                if select.select([master], [], [], min(.01, max(0, deadline - time.monotonic())))[0]:
                    raw.extend(os.read(master, 65536))

        try:
            deadline = time.monotonic() + 5
            while b"\x1b[?2026l" not in raw and time.monotonic() < deadline:
                assert child.poll() is None, "reader fixture exited during startup"
                drain(.01)
            assert b"\x1b[?2026l" in raw, "reader fixture did not present a frame"
            control.connect(address)
            drain(.2)
            start_switches = thread_switches(child.pid)
            start_bytes = len(raw)
            sample_start = time.monotonic()
            drain(1)
            sample_seconds = time.monotonic() - sample_start
            end_switches = thread_switches(child.pid)
            assert start_switches.keys() == end_switches.keys(), "idle worker set changed"
            report.update(idle_seconds=sample_seconds,
                switches_before=start_switches, switches_after=end_switches,
                idle_switches_per_second={tid: (end_switches[tid] - count) / sample_seconds
                    for tid, count in start_switches.items()},
                idle_output_bytes=len(raw) - start_bytes)
            partial = {"split_utf8": b"\xe2", "unterminated_paste": b"\x1b[200~partial"}.get(scenario)
            if partial:
                os.write(master, partial)
                drain(.1)
            if scenario == "resize":
                frame_count = raw.count(b"\x1b[?2026l")
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
                drain(.5)
                report["resize_presented"] = raw.count(b"\x1b[?2026l") > frame_count
            stop_start = time.monotonic()
            if scenario == "hangup":
                os.close(master)
                master = None
            else:
                control.sendall(b'{"stop":true}\n')
            limit = 2 if scenario == "hangup" else 1
            while child.poll() is None and time.monotonic() - stop_start < limit:
                if master is None:
                    time.sleep(.01)
                else:
                    drain(.01)
            report["shutdown_ms"] = (time.monotonic() - stop_start) * 1000
            report["timed_out"] = child.poll() is None
            report["exit_code"] = child.poll()
            if scenario == "hangup":
                report["stderr"] = error_path.read_text()
            if master is not None:
                while select.select([master], [], [], 0)[0]:
                    raw.extend(os.read(master, 65536))
                report["termios_restored"] = termios.tcgetattr(slave) == before
                report["protocol_exit"] = all(sequence in raw for sequence in
                    (b"\x1b[?1049l", b"\x1b[?2004l", b"\x1b[?1000l"))
        finally:
            control.close()
            if child.poll() is None:
                child.kill()
                child.wait()
            if master is not None:
                os.close(master)
            os.close(slave)
    return report, raw


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
            if scenario == "telemetry_failure":
                blocker.write_text("fixture\n")
            env["HARNESS_TUI_PRESENTATION_TRACE"] = str(blocker / "trace.json")
        with open("/dev/full", "wb", buffering=0) as full:
            child = subprocess.Popen([str(binary), scenario if scenario.startswith("handoff") else "idle"], stdin=slave,
                stdout=full if scenario == "output_failure" else slave, stderr=slave,
                cwd=root, env=env, preexec_fn=attach_terminal)
            start = time.monotonic()
            sent_frames = 0
            exits_sent = 0
            try:
                while child.poll() is None:
                    if time.monotonic() - start > 10:
                        raise RuntimeError(f"{scenario}: child failed to exit")
                    if select.select([master], [], [], .01)[0]:
                        raw.extend(os.read(master, 65536))
                    frames = raw.count(b"\x1b[?2026l")
                    expected_exits = 4 if scenario == "handoff" else 1
                    if (scenario in ("normal", "handoff", "handoff_failure")
                            and exits_sent < expected_exits and frames > sent_frames):
                        os.write(master, b"\x11\x11")
                        sent_frames = frames
                        exits_sent += 1
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
            "exits_sent": exits_sent,
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
    parser.add_argument("--live-binary", type=Path, help="also check Linux reader wakeups and joined shutdown")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    reports = []
    for scenario in ("normal", "telemetry_failure", "output_failure", "handoff_failure", "handoff"):
        result, raw = check(args.binary.resolve(), scenario)
        reports.append(result)
        (args.output / f"{scenario}.ansi").write_bytes(raw)
        print(json.dumps(result), flush=True)
    (args.output / "restoration.json").write_text(json.dumps(reports, indent=2) + "\n")
    reader_reports = []
    if args.live_binary:
        for scenario in ("idle", "split_utf8", "unterminated_paste", "resize", "hangup"):
            result, raw = check_reader(args.live_binary.resolve(), scenario)
            reader_reports.append(result)
            (args.output / f"reader-{scenario}.ansi").write_bytes(raw)
            print(json.dumps(result), flush=True)
        (args.output / "reader.json").write_text(json.dumps(reader_reports, indent=2) + "\n")
    assert reports[0]["exit_code"] == 0 and reports[0]["termios_restored"]
    assert all(reports[0]["protocol_exit"].values())
    if not args.record_reference_defects:
        for result in reader_reports:
            assert max(result["idle_switches_per_second"].values()) <= 2, "periodic idle wakeups"
            assert result["idle_output_bytes"] == 0, "idle redraw"
            assert not result["timed_out"], "reader shutdown timed out"
            if result["scenario"] == "hangup":
                assert result["exit_code"] == 1, "terminal hangup did not return a normal error"
                assert "terminal event poll failed:" in result["stderr"], "missing typed reader failure"
            else:
                assert result["exit_code"] == 0 and result["termios_restored"] and result["protocol_exit"]
            if result["scenario"] == "resize":
                assert result["resize_presented"], "resize without input did not redraw"
        assert all(result["termios_restored"] for result in reports), "terminal restoration failed"
        assert all(result["exit_code"] != 0 for result in reports[1:4]), "failure injection did not fail"
        assert reports[3]["exits_sent"] == 1, "handoff failure must follow a completed session"
        assert reports[4]["exit_code"] == 0 and reports[4]["exits_sent"] == 4, "preserved handoffs failed"
        for result in (reports[1], reports[3]):
            assert all(result["protocol_exit"].values()), "initialization failure left terminal protocols active"


if __name__ == "__main__":
    main()
