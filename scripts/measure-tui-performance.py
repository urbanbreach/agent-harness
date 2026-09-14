#!/usr/bin/env python3
"""Measure release TUI rendering/ANSI encoding in isolated, serial nextest processes."""

import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--frames", type=int, default=120)
    parser.add_argument("--scenario", help="measure only this scenario")
    parser.add_argument("--binaries-metadata", type=Path)
    parser.add_argument("--cargo-metadata", type=Path)
    args = parser.parse_args()
    if args.repetitions < 1 or args.frames < 1:
        parser.error("repetitions and frames must be positive")
    if bool(args.binaries_metadata) != bool(args.cargo_metadata):
        parser.error("both metadata paths are required to reuse a compiled baseline")
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    command = ["cargo", "nextest", "run", "--profile", "perf", "-j", "1", "--success-output", "immediate",
               "-E", "test(perf_interactive_resources)"]
    command += (["--binaries-metadata", str(args.binaries_metadata.resolve()),
                 "--cargo-metadata", str(args.cargo_metadata.resolve())]
                if args.binaries_metadata else ["--release", "-p", "harness-tui", "--lib"])
    scenarios = [("startup", 0), ("static", 100), ("static", 10000),
                 ("scroll", 10000), ("typing", 10000), ("stream", 100),
                 ("stream", 10000), ("code", 100), ("tool", 10000),
                 ("hover", 10000), ("selection", 10000), ("resize", 10000),
                 ("stream-events", 10000)]
    if args.scenario:
        scenarios = [item for item in scenarios if item[0] == args.scenario]
        if not scenarios:
            parser.error("unknown scenario")
    summary = {"commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
               "platform": platform.platform(), "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
               "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
               "repetitions": args.repetitions, "frames": args.frames, "scenarios": {}}
    for scenario, history in scenarios:
        samples = []
        for repetition in range(args.repetitions):
            directory = output / str(repetition + 1)
            directory.mkdir(exist_ok=True)
            env = {**os.environ, "HARNESS_PERF_SCENARIO": scenario,
                   "HARNESS_PERF_HISTORY": str(history), "HARNESS_PERF_FRAMES": str(args.frames),
                   "HARNESS_PERF_ARTIFACT_DIR": str(directory), "CARGO_BUILD_JOBS": "2"}
            log = directory / f"{scenario}-{history}.log"
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=root, env=env, stdout=stream, stderr=subprocess.STDOUT)
            if result.returncode:
                raise SystemExit(f"Measurement failed; see {log}")
            sample = json.loads((directory / f"{scenario}-{history}.json").read_text())
            sample["cpu_ms_per_frame"] = sample["cpu_ticks"] * 1000 / summary["clock_ticks_per_second"] / args.frames
            samples.append(sample)
        fields = ["cold_us", "p50_us", "p95_us", "p99_us", "cpu_ms_per_frame",
                  "rss_before_kib", "rss_after_kib", "peak_rss_kib", "bytes_submitted"]
        medians = {field: statistics.median(sample[field] for sample in samples) for field in fields}
        summary["scenarios"][f"{scenario}-{history}"] = medians
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(f"{scenario:8} {history:5}: p95 {medians['p95_us']:8.0f} us, "
              f"CPU {medians['cpu_ms_per_frame']:.3f} ms/frame, "
              f"RSS {medians['rss_after_kib'] / 1024:.1f} MiB", flush=True)


if __name__ == "__main__":
    main()
