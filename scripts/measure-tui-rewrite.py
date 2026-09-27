#!/usr/bin/env python3
"""Serial release measurements through public TUI inputs, rendering, and ANSI encoding."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--frames", type=int, default=200)
    parser.add_argument("--history", type=int, default=1000)
    parser.add_argument("--allocations", action="store_true", help="separate glibc memusage run")
    args = parser.parse_args()
    if args.repetitions < 1 or args.frames < 100 or args.history < 1:
        parser.error("need positive repetitions/history and at least 100 frames")
    root, output = args.root.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    target = ["--release", "-p", "harness-tui", "--test", "rewrite_performance_test"]
    # Finish compilation before taking any timing samples.
    metadata = json.loads(subprocess.check_output(
        ["cargo", "nextest", "list", *target, "--message-format", "json"], cwd=root))
    binary = next(iter(metadata["rust-suites"].values()))["binary-path"]
    command = ["cargo", "nextest", "run", *target, "--profile", "perf", "-j", "1", "--success-output", "immediate"]
    summary = {"schema": "tui-rewrite-renderer-v1", "platform": platform.platform(),
               "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
               "binary_sha256": hashlib.file_digest(open(binary, "rb"), "sha256").hexdigest(),
               "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
               "command": command, "scenarios": {}}
    workloads = [("startup", 0), ("idle", args.history), ("typing", args.history),
                 ("stream", args.history), ("scroll", args.history), ("resize", args.history)]
    for scenario, count in workloads:
        samples = []
        for repetition in range(args.repetitions + int(args.allocations)):
            allocation = repetition == args.repetitions
            stem = f"{scenario}-{count}-{'alloc' if allocation else repetition + 1}"
            path = output / f"{stem}.json"
            env = {**os.environ, "HARNESS_REWRITE_SCENARIO": scenario,
                   "HARNESS_REWRITE_HISTORY": str(count), "HARNESS_REWRITE_FRAMES": str(args.frames),
                   "HARNESS_REWRITE_PERF_OUT": str(path)}
            run = (["memusage", "-n", Path(binary).name, "--no-timer", *command]
                   if allocation else command)
            with (output / f"{stem}.log").open("w") as log:
                subprocess.run(run, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            if not allocation:
                samples.append(json.loads(path.read_text()))
        fields = ["construction_us", "cold_us", "p50_us", "p95_us", "p99_us", "bytes"]
        median = {key: statistics.median(sample[key] for sample in samples) for key in fields}
        median["rss_kib"] = statistics.median(sample["after"]["rss_kib"] for sample in samples)
        median["cpu_ms_per_frame"] = statistics.median(
            (sample["after"]["cpu_ticks"] - sample["before"]["cpu_ticks"]) * 1000
            / os.sysconf("SC_CLK_TCK") / args.frames for sample in samples)
        summary["scenarios"][f"{scenario}-{count}"] = median
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(f"{scenario}-{count}: p95={median['p95_us']}us RSS={median['rss_kib']}KiB", flush=True)


if __name__ == "__main__":
    main()
