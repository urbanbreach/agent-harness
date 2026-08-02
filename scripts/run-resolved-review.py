#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

GATES = frozenset({"F1", "F2", "F3", "F4-pre", "F4-final"})
REQUIRED_COMMAND_FIELDS = frozenset({"argv", "cwd", "input_digests", "output_paths", "expected_exit_status"})
DESCRIPTION = "Execute a fully resolved F1-F4 review manifest without prose verdicts"


@dataclass(frozen=True, slots=True)
class ResolvedCommand:
    argv: tuple[str, ...]
    cwd: Path
    input_digests: dict[Path, str]
    expected_exit_status: int


def reject(reason: str) -> ValueError:
    return ValueError(reason)


def load_manifest(path: Path) -> dict[str, object]:
    if not path.is_absolute() or not path.is_file():
        raise reject("manifest must be an existing absolute path")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise reject("manifest root must be an object")
    return value


def validate_gate(manifest: dict[str, object], gate: str) -> ResolvedCommand:
    if gate not in GATES:
        raise reject(f"unsupported gate: {gate}")
    gates = manifest.get("gates")
    if not isinstance(gates, dict) or gate not in gates:
        raise reject(f"manifest has no resolved {gate} gate")
    command = gates[gate]
    if not isinstance(command, dict) or not REQUIRED_COMMAND_FIELDS.issubset(command):
        raise reject(f"{gate} has incomplete resolved command fields")
    argv = command["argv"]
    if not isinstance(argv, list) or not all(isinstance(item, str) for item in argv):
        raise reject(f"{gate} argv must be a string list")
    cwd = Path(str(command["cwd"]))
    if not cwd.is_absolute() or not cwd.is_dir():
        raise reject(f"{gate} cwd must be an existing absolute directory")
    inputs = command["input_digests"]
    if not isinstance(inputs, dict) or not inputs:
        raise reject(f"{gate} must declare non-empty input digests")
    input_digests: dict[Path, str] = {}
    for raw_path, expected in inputs.items():
        source = Path(str(raw_path))
        if not source.is_absolute() or not source.is_file() or not isinstance(expected, str):
            raise reject(f"{gate} input is not an existing absolute file: {source}")
        actual = hashlib.sha256(source.read_bytes()).hexdigest()
        if actual != expected:
            raise reject(f"{gate} input digest mismatch: {source}")
        input_digests[source] = expected
    expected_exit_status = command["expected_exit_status"]
    if not isinstance(expected_exit_status, int):
        raise reject(f"{gate} expected exit status must be an integer")
    return ResolvedCommand(tuple(argv), cwd, input_digests, expected_exit_status)


def execute(command: ResolvedCommand, gate: str) -> dict[str, object]:
    completed = subprocess.run(command.argv, cwd=command.cwd, capture_output=True, text=True, check=False)
    if completed.returncode != command.expected_exit_status:
        raise reject(f"{gate} command exit {completed.returncode}, expected {command.expected_exit_status}")
    return {"gate": gate, "verdict": "pass", "actual_exit_status": completed.returncode, "stdout_sha256": hashlib.sha256(completed.stdout.encode()).hexdigest(), "stderr_sha256": hashlib.sha256(completed.stderr.encode()).hexdigest()}


def main() -> int:
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    _ = parser.add_argument("--manifest", type=Path, required=True)
    _ = parser.add_argument("--gate", choices=sorted(GATES))
    _ = parser.add_argument("--reviewer", choices=("visual", "runtime-security-evidence"))
    args = parser.parse_args()
    if (args.gate is None) == (args.reviewer is None):
        raise reject("provide exactly one of --gate or --reviewer")
    manifest = load_manifest(args.manifest)
    requested = [args.gate] if args.gate else (["F1", "F2", "F3", "F4-pre"] if args.reviewer == "runtime-security-evidence" else ["F3"])
    results = [execute(validate_gate(manifest, gate), gate) for gate in requested]
    print(json.dumps({"verdict": "unconditional_approval", "reviewer": args.reviewer, "results": results}, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(json.dumps({"verdict": "rejected", "reason": str(error)}, sort_keys=True), file=sys.stderr)
        raise SystemExit(1)
