#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import cast


SCHEMA = "harness-large-session-perf-v1"
REQUIRED_ARTIFACTS = ("large-session-surfaces.json",)
DEFAULT_MAX_AGE_MS = 6 * 60 * 60 * 1000
FUTURE_SKEW_MS = 5 * 60 * 1000


class ParsedArgs(argparse.Namespace):
    artifact_dir: str
    max_age_ms: int

    def __init__(self) -> None:
        super().__init__()
        self.artifact_dir = ""
        self.max_age_ms = DEFAULT_MAX_AGE_MS


def number(value: object) -> bool:
    return isinstance(value, int | float) and not isinstance(value, bool)


def positive_number(value: object | None) -> bool:
    if value is None or not number(value):
        return False
    return cast(int | float, value) >= 1


def object_map(value: object | None) -> dict[str, object]:
    if isinstance(value, dict):
        return cast(dict[str, object], value)
    return {}


def validate_large_session_artifact(path: Path, artifact_dir: Path, max_age_ms: int) -> list[str]:
    violations: list[str] = []
    try:
        loaded = cast(object, json.loads(path.read_text()))
    except (OSError, json.JSONDecodeError) as exc:
        return [f"{path.name}: unreadable perf artifact: {exc}"]

    data = object_map(loaded)
    if not data:
        return [f"{path.name}: perf artifact must be a JSON object"]

    if data.get("schema_version") != SCHEMA:
        violations.append(f"{path.name}: schema_version must be {SCHEMA}")

    timestamp = data.get("timestamp_unix_ms")
    now_ms = int(time.time() * 1000)
    if not isinstance(timestamp, int):
        violations.append(f"{path.name}: timestamp_unix_ms must be an integer")
    elif timestamp < now_ms - max_age_ms or timestamp > now_ms + FUTURE_SKEW_MS:
        violations.append(f"{path.name}: stale timestamp_unix_ms {timestamp}")

    corpus = object_map(data.get("corpus"))
    if not positive_number(corpus.get("session_count")):
        violations.append(f"{path.name}: corpus.session_count must be positive")
    if not positive_number(corpus.get("total_events")):
        violations.append(f"{path.name}: corpus.total_events must be positive")

    measurements = object_map(data.get("measurements"))
    for key in ("sessions_list_ms", "sessions_reopen_ms", "session_search_ms"):
        value = measurements.get(key)
        if value is None or not number(value):
            violations.append(f"{path.name}: measurements.{key} must be numeric")

    provenance = object_map(data.get("provenance"))
    if provenance.get("command_hint") != "scripts/test-lanes.sh perf":
        violations.append(f"{path.name}: provenance.command_hint must name scripts/test-lanes.sh perf")
    provenance_artifact_dir = Path(str(provenance.get("artifact_root_env", ""))).resolve()
    if provenance_artifact_dir != artifact_dir:
        violations.append(f"{path.name}: provenance.artifact_root_env must match --artifact-dir")

    return violations


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    _ = parser.add_argument("--artifact-dir", required=True)
    _ = parser.add_argument("--max-age-ms", type=int, default=DEFAULT_MAX_AGE_MS)
    args = parser.parse_args(argv, namespace=ParsedArgs())

    artifact_dir = Path(args.artifact_dir).resolve()
    violations: list[str] = []
    if not artifact_dir.is_dir():
        violations.append(f"artifact directory missing: {artifact_dir}")
    else:
        for name in REQUIRED_ARTIFACTS:
            artifact_path = artifact_dir / name
            if not artifact_path.is_file():
                violations.append(f"required perf artifact missing: {artifact_path}")
                continue
            if name == "large-session-surfaces.json":
                violations.extend(
                    validate_large_session_artifact(artifact_path, artifact_dir, args.max_age_ms)
                )

    if violations:
        print("perf artifact freshness: FAIL")
        for violation in violations:
            print(f"- {violation}")
        return 1

    print(f"perf artifact freshness: PASS ({artifact_dir})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
