from pathlib import Path
import datetime, json, os, statistics, subprocess
out = Path(__file__).resolve().parent
root = out / "affinity"
root.mkdir(exist_ok=True)
cpu = min(os.sched_getaffinity(0))
reports = []
for sample in range(1, 4):
    for side in ["candidate", "before"] if sample == 2 else ["before", "candidate"]:
        stem = f"{side}-{sample}"
        command = ["taskset", "-c", str(cpu), "cargo", "nextest", "run", "--binaries-metadata", str(out / side / "binaries.json"), "--cargo-metadata", str(out / "cargo.json"), "--profile", "perf", "-j1", "--success-output", "immediate"]
        env = {**os.environ, "HARNESS_REWRITE_SCENARIO": "typing", "HARNESS_REWRITE_HISTORY": "0", "HARNESS_REWRITE_FRAMES": "500", "HARNESS_REWRITE_PERF_OUT": str(root / f"{stem}.json")}
        started = datetime.datetime.now(datetime.timezone.utc).isoformat()
        with (root / f"{stem}.log").open("w") as log:
            result = subprocess.run(command, env=env, stdout=log, stderr=log)
        reports.append({"side": side, "sample": sample, "started_at": started, "cpu": cpu, "command": command, "exit": result.returncode})
        (root / "run-order.json").write_text(json.dumps(reports, indent=2) + "\n")
        result.check_returncode()
summary = {}
for side in ["before", "candidate"]:
    samples = [json.loads((root / f"{side}-{i}.json").read_text()) for i in range(1,4)]
    summary[side] = {key: statistics.median(s[key] for s in samples) for key in ["p50_us", "p95_us", "p99_us", "bytes"]}
for i in range(1,4):
    a = json.loads((root / f"before-{i}.json").read_text())
    b = json.loads((root / f"candidate-{i}.json").read_text())
    assert all(a[k] == b[k] for k in ["scenario", "history_turns", "history_events", "frames", "bytes", "visible", "oldest"])
summary["scope"] = "Fixed-CPU diagnostic only; does not replace unaffinitized acceptance samples or limits."
summary["cpu"] = cpu
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
