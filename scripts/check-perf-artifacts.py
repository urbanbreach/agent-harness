#!/usr/bin/env python3
"""Check freshness and provenance of perf lane artifacts."""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import Final

SCHEMA: Final[str] = "harness-large-session-perf-v1"
REQUIRED_ARTIFACTS: Final[tuple[str, ...]] = ("large-session-surfaces.json",)
DEFAULT_MAX_AGE_MS: Final[int] = 6 * 60 * 60 * 1000
FUTURE_SKEW_MS: Final[int] = 5 * 60 * 1000
MEASUREMENT_KEYS: Final[tuple[str, ...]] = (
    "sessions_list_ms",
    "sessions_reopen_ms",
    "session_search_ms",
)
PROVENANCE_COMMAND_HINT: Final[str] = "scripts/test-lanes.sh perf"


class ParsedArgs(argparse.Namespace):
    """Typed namespace for parsed CLI arguments."""

    artifact_dir: str
    max_age_ms: int

    def __init__(self) -> None:
        """Initialize with default values."""
        super().__init__()
        self.artifact_dir = ""
        self.max_age_ms = DEFAULT_MAX_AGE_MS


def number(value: object) -> bool:
    """Return True if value is a real number (int or float, not bool)."""
    return isinstance(value, int | float) and not isinstance(value, bool)


def positive_number(value: object | None) -> bool:
    """Return True if value is a positive number (>= 1)."""
    if isinstance(value, bool):
        return False
    if isinstance(value, int | float):
        return value >= 1
    return False


def object_map(value: object | None) -> dict[str, object]:
    """Coerce a value into a dict[str, object], returning empty dict on failure."""
    if isinstance(value, dict):
        return {str(key): val for key, val in value.items()}
    return {}


def _validate_timestamp(
    path: Path, data: dict[str, object], max_age_ms: int
) -> list[str]:
    """Validate the timestamp_unix_ms field."""
    timestamp = data.get("timestamp_unix_ms")
    now_ms = int(time.time() * 1000)
    if not isinstance(timestamp, int):
        return [f"{path.name}: timestamp_unix_ms must be an integer"]
    if timestamp < now_ms - max_age_ms or timestamp > now_ms + FUTURE_SKEW_MS:
        return [f"{path.name}: stale timestamp_unix_ms {timestamp}"]
    return []


def _validate_corpus(path: Path, data: dict[str, object]) -> list[str]:
    """Validate the corpus fields."""
    violations: list[str] = []
    corpus = object_map(data.get("corpus"))
    if not positive_number(corpus.get("session_count")):
        violations.append(f"{path.name}: corpus.session_count must be positive")
    if not positive_number(corpus.get("total_events")):
        violations.append(f"{path.name}: corpus.total_events must be positive")
    return violations


def _validate_measurements(path: Path, data: dict[str, object]) -> list[str]:
    """Validate the measurements fields."""
    violations: list[str] = []
    measurements = object_map(data.get("measurements"))
    for key in MEASUREMENT_KEYS:
        value = measurements.get(key)
        if value is None or not number(value):
            violations.append(f"{path.name}: measurements.{key} must be numeric")
    return violations


def _validate_provenance(
    path: Path, data: dict[str, object], artifact_dir: Path
) -> list[str]:
    """Validate the provenance fields."""
    violations: list[str] = []
    provenance = object_map(data.get("provenance"))
    if provenance.get("command_hint") != PROVENANCE_COMMAND_HINT:
        violations.append(
            f"{path.name}: provenance.command_hint must name {PROVENANCE_COMMAND_HINT}"
        )
    provenance_artifact_dir = Path(
        str(provenance.get("artifact_root_env", ""))
    ).resolve()
    if provenance_artifact_dir != artifact_dir:
        violations.append(
            f"{path.name}: provenance.artifact_root_env must match --artifact-dir"
        )
    return violations


def validate_large_session_artifact(
    path: Path, artifact_dir: Path, max_age_ms: int
) -> list[str]:
    """Validate a large-session-surfaces.json perf artifact."""
    try:
        loaded: object = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        return [f"{path.name}: unreadable perf artifact: {exc}"]

    data = object_map(loaded)
    if not data:
        return [f"{path.name}: perf artifact must be a JSON object"]

    violations: list[str] = []
    if data.get("schema_version") != SCHEMA:
        violations.append(f"{path.name}: schema_version must be {SCHEMA}")

    violations.extend(_validate_timestamp(path, data, max_age_ms))
    violations.extend(_validate_corpus(path, data))
    violations.extend(_validate_measurements(path, data))
    violations.extend(_validate_provenance(path, data, artifact_dir))

    return violations


def main(argv: list[str]) -> int:
    """Entry point: validate perf artifacts in the given directory."""
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--artifact-dir", required=True)
    _ = parser.add_argument(
        "--max-age-ms", type=int, default=DEFAULT_MAX_AGE_MS
    )
    args = parser.parse_args(argv, namespace=ParsedArgs())

    artifact_dir = Path(args.artifact_dir).resolve()
    violations: list[str] = []
    if not artifact_dir.is_dir():
        violations.append(f"artifact directory missing: {artifact_dir}")
    else:
        for name in REQUIRED_ARTIFACTS:
            artifact_path = artifact_dir / name
            if not artifact_path.is_file():
                violations.append(
                    f"required perf artifact missing: {artifact_path}"
                )
                continue
            if name == "large-session-surfaces.json":
                violations.extend(
                    validate_large_session_artifact(
                        artifact_path, artifact_dir, args.max_age_ms
                    )
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
