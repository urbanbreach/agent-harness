#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REQUIRED_APPROVAL_FIELDS = frozenset({"schema_version", "verdict", "reviewer", "input_set_digest", "product_epoch", "candidate_sha256", "reference_sha256", "read_only", "findings"})
REQUIRED_REJECTION_FIELDS = frozenset({"root_cause_class", "earliest_task", "write_reservation", "affected_descendant_tasks", "repair_namespace", "reentry_gate"})
DESCRIPTION = "Validate independent visual/runtime review verdicts and provenance"


def reject(reason: str) -> ValueError:
    return ValueError(reason)


def validate(value: dict[str, object]) -> None:
    if not REQUIRED_APPROVAL_FIELDS.issubset(value):
        raise reject("review verdict omits required machine-readable provenance")
    reviewer = value["reviewer"]
    if not isinstance(reviewer, dict) or not all(isinstance(reviewer[field], str) and reviewer[field] for field in ("identity", "tool", "model", "version")):
        raise reject("reviewer identity/tool/model/version is required")
    if value["read_only"] is not True:
        raise reject("independent review must be read-only")
    verdict = value["verdict"]
    findings = value["findings"]
    if not isinstance(findings, list):
        raise reject("findings must be a list")
    if verdict == "unconditional_approval":
        if findings:
            raise reject("approval cannot contain findings")
        return
    if verdict != "rejected":
        raise reject("verdict must be unconditional_approval or rejected")
    for finding in findings:
        if not isinstance(finding, dict) or not REQUIRED_REJECTION_FIELDS.issubset(finding):
            raise reject("rejection finding lacks F1 repair-routing fields")


def main() -> int:
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    _ = parser.add_argument("--verdict", type=Path, required=True)
    args = parser.parse_args()
    if not args.verdict.is_absolute() or not args.verdict.is_file():
        raise reject("verdict must be an existing absolute path")
    value = json.loads(args.verdict.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise reject("verdict root must be an object")
    validate(value)
    print(json.dumps({"verdict": value["verdict"], "validated": True}, sort_keys=True))
    return 0 if value["verdict"] == "unconditional_approval" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(json.dumps({"verdict": "rejected", "reason": str(error)}, sort_keys=True), file=sys.stderr)
        raise SystemExit(1)
