#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf '%s\n' 'Usage: scripts/engine-metrics.sh --output <path> --baseline <git-sha>' >&2
}

output=''
baseline=''
while (($#)); do
    case "$1" in
        --output)
            output=${2:-}
            shift 2
            ;;
        --baseline)
            baseline=${2:-}
            shift 2
            ;;
        *)
            usage
            exit 64
            ;;
    esac
done

if [[ -z "$output" || -z "$baseline" ]]; then
    usage
    exit 64
fi

root=$(git rev-parse --show-toplevel)
baseline_full=$(git -C "$root" rev-parse --verify "${baseline}^{commit}") || {
    printf 'engine-metrics: baseline is not a commit: %s\n' "$baseline" >&2
    exit 65
}

mkdir -p "$(dirname "$output")"
python3 - "$root" "$baseline_full" "$output" <<'PY'
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

root = Path(sys.argv[1])
baseline = sys.argv[2]
output = Path(sys.argv[3])
tokens = ("session", "conversation", "transcript", "projection", "provider_context", "compaction")
crates = ("harness", "harness-core", "harness-providers", "harness-tools", "harness-tui", "harness-testkit")
supplied = {
    "production_loc": 205939,
    "per_crate_production_loc": {"harness-core": 54964, "harness-tui": 100800},
    "buckets": {"session": 14207, "projection": 5944, "compaction": 1585, "model_resolution": 329, "coordinator": 15121},
    "event_variants": 39,
    "active_session_compaction_variants": 2,
    "durable_reducers": 5,
    "size_ok": {"all": 192, "reachable": 185},
}


def command(args, *, cwd=root):
    return subprocess.run(args, cwd=cwd, check=True, text=True, capture_output=True)


def production_path(path):
    parts = path.parts
    name = path.name
    return (
        path.suffix == ".rs"
        and "/src/" in f"/{path.as_posix()}/"
        and "tests" not in parts
        and not name.endswith("_test.rs")
        and name != "tests.rs"
    )


def strip_cfg_test(text):
    kept = []
    skipping = False
    depth = 0
    seen_brace = False
    for line in text.splitlines():
        if re.search(r"#\[cfg\(test\)\]", line):
            skipping = True
            depth = line.count("{") - line.count("}")
            seen_brace = "{" in line
            if seen_brace and depth <= 0:
                skipping = False
            continue
        if skipping:
            depth += line.count("{") - line.count("}")
            seen_brace = seen_brace or "{" in line
            if (not seen_brace and line.rstrip().endswith(";")) or (seen_brace and depth <= 0):
                skipping = False
            continue
        kept.append(line)
    return "\n".join(kept) + ("\n" if text.endswith("\n") else "")


def source_files(ref):
    if ref == "WORKTREE":
        return [
            path.relative_to(root).as_posix()
            for path in root.glob("crates/*/src/**/*.rs")
            if production_path(path.relative_to(root))
        ]
    listing = command(["git", "ls-tree", "-r", "--name-only", ref, "--", "crates"]).stdout
    return [path for path in listing.splitlines() if production_path(Path(path))]


def rust_files(ref):
    if ref == "WORKTREE":
        return [path.relative_to(root).as_posix() for path in root.glob("crates/**/*.rs")]
    listing = command(["git", "ls-tree", "-r", "--name-only", ref, "--", "crates"]).stdout
    return [path for path in listing.splitlines() if path.endswith(".rs")]


def source_text(ref, path):
    if ref == "WORKTREE":
        return (root / path).read_text(encoding="utf-8")
    return command(["git", "show", f"{ref}:{path}"]).stdout


def source_map(ref):
    return {path: strip_cfg_test(source_text(ref, path)) for path in sorted(source_files(ref))}


def loc(text):
    return sum(1 for line in text.splitlines() if line.strip() and not line.lstrip().startswith("//"))


def declared_modules(text):
    return re.findall(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)", text, re.M)


def overlap_files(sources):
    return {
        path: text
        for path, text in sources.items()
        if any(token in path.lower() for token in tokens)
        or any(any(token in module.lower() for token in tokens) for module in declared_modules(text))
    }


def enum_variants(text):
    marker = "pub enum EventV1 {"
    start = text.index(marker) + len(marker)
    depth = 1
    end = start
    for index, char in enumerate(text[start:], start):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index
                break
    return re.findall(r"^\s*([A-Z][A-Za-z0-9_]*)\(", text[start:end], re.M)


def metric_snapshot(ref):
    sources = source_map(ref)
    overlap = overlap_files(sources)
    per_crate = {
        crate: sum(loc(text) for path, text in sources.items() if path.startswith(f"crates/{crate}/"))
        for crate in crates
    }
    buckets = {
        "session": sum(loc(text) for path, text in sources.items() if "session" in path.lower() or "conversation" in path.lower()),
        "projection": sum(loc(text) for path, text in sources.items() if "projection" in path.lower()),
        "compaction": sum(loc(text) for path, text in sources.items() if "compaction" in path.lower()),
        "model_resolution": sum(loc(text) for path, text in sources.items() if "model_resolution" in path.lower()),
        "coordinator": sum(loc(text) for path, text in sources.items() if "/coord" in path),
    }
    event = sources["crates/harness-core/src/event.rs"]
    variants = enum_variants(event)
    compaction = [name for name in variants if "Compaction" in name or name == "BranchSummary"]
    active_compaction = [name for name in ("SessionCompaction", "CompactionFailed") if name in compaction]
    size_ok_paths = sorted(path for path in rust_files(ref) if "SIZE_OK" in source_text(ref, path))
    durable_reducer_paths = [
        "crates/harness-core/src/conversation.rs",
        "crates/harness-core/src/transcript_projection.rs",
        "crates/harness-core/src/proj/resume_projection.rs",
        "crates/harness-core/src/coord/provider_context/restore.rs",
        "crates/harness-tui/src/app/session_projection.rs",
    ]
    files = [
        {"path": path, "sha256": hashlib.sha256(text.encode()).hexdigest(), "production_loc": loc(text)}
        for path, text in sorted(overlap.items())
    ]
    return {
        "production_loc": sum(per_crate.values()),
        "per_crate_production_loc": per_crate,
        "buckets": buckets,
        "frozen_overlap": {
            "file_set_sha256": hashlib.sha256(("\n".join(item["path"] for item in files) + "\n").encode()).hexdigest(),
            "production_loc": sum(item["production_loc"] for item in files),
            "files": files,
        },
        "event_variants": {"count": len(variants), "names": variants},
        "compaction_variants": {"all": compaction, "active_session_compaction": active_compaction},
        "reducer_count": {"count": len(durable_reducer_paths), "paths": durable_reducer_paths},
        "size_ok": {
            "all": len(size_ok_paths),
            "reachable": {
                "status": "unavailable",
                "reason": "no checked-in module-graph reachability oracle covers cfg, target, and generated module selection",
            },
            "paths": size_ok_paths,
        },
    }


def runtime_metrics():
    binary = root / "target/debug/harness"
    if not binary.is_file():
        command(["cargo", "build", "-q", "-p", "harness"])
    with tempfile.TemporaryDirectory(prefix="engine-metrics-") as temp:
        temp_path = Path(temp)
        session_dir = temp_path / "sessions"
        event_log = temp_path / "representative.events.jsonl"
        command([
            str(binary), "--session-dir", str(session_dir), "run", "--scenario", "golden_path",
            "--deterministic", "--out", str(event_log), "--print-run-dir",
        ])
        return {
            "representative_log": {"bytes": event_log.stat().st_size, "events": len(event_log.read_text(encoding="utf-8").splitlines())},
            "representative_corpus": {
                "status": "unavailable",
                "fixture": "crates/harness/tests/perf_sessions_surface_test.rs",
                "declared_corpus": {"sessions": 120, "turns_per_session": 6, "events": 3960},
                "command": "HARNESS_PERF_ARTIFACT_DIR=<dir> cargo nextest run --profile perf -p harness --test perf_sessions_surface_test",
                "reason": "the current fixture fails before writing its artifact because reopen reports resumable=false",
            },
            "list_inspect_latency": {
                "list": {"status": "unavailable", "reason": "no successful representative-corpus artifact"},
                "inspect": {"status": "unavailable", "reason": "the existing corpus fixture measures reopen, not inspect"},
            },
            "long_session_context_build": {
                "status": "unavailable",
                "reason": "no standalone non-network long-session provider-context rebuild benchmark exists; a one-turn golden path is not substituted",
            },
        }


baseline_metrics = metric_snapshot(baseline)
current_metrics = metric_snapshot("WORKTREE")
runtime = runtime_metrics()
observed_baseline = {
    "production_loc": baseline_metrics["production_loc"],
    "per_crate_production_loc": {key: baseline_metrics["per_crate_production_loc"][key] for key in supplied["per_crate_production_loc"]},
    "buckets": baseline_metrics["buckets"],
    "event_variants": baseline_metrics["event_variants"]["count"],
    "active_session_compaction_variants": len(baseline_metrics["compaction_variants"]["active_session_compaction"]),
    "durable_reducers": baseline_metrics["reducer_count"]["count"],
    "size_ok": {"all": baseline_metrics["size_ok"]["all"], "reachable": baseline_metrics["size_ok"]["reachable"]},
}
payload = {
    "schema_version": "engine-metrics-v1",
    "baseline_commit": baseline,
    "measurement_method": "first-party crates/*/src Rust; excludes Rust test paths and #[cfg(test)] modules",
    "baseline_contract": supplied,
    "baseline_measured": observed_baseline,
    "baseline_drift": {key: supplied[key] != observed_baseline[key] for key in supplied},
    "baseline": baseline_metrics,
    "current": current_metrics,
    **runtime,
}
with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=output.parent, delete=False) as handle:
    json.dump(payload, handle, indent=2, sort_keys=True)
    handle.write("\n")
    temporary = Path(handle.name)
os.replace(temporary, output)
PY
