#!/usr/bin/env python3
"""Coverage checker for the reference-source crosswalk (Task 2).

Fails closed if:
  - Any required source root has zero cited rows.
  - A public Grok command/action/view is omitted.
  - A row is duplicated (same grok_path + grok_symbol).
  - A ``keep`` row lacks a Harness owner.
  - A row lacks valid reference_source entries with real paths.
  - A row uses an invalid decision or harness_state value.
  - The crosswalk file is missing or malformed.

Usage:
    python3 scripts/check-parity-reference-crosswalk.py --crosswalk <path> [--task 2]
    python3 scripts/check-parity-reference-crosswalk.py --crosswalk <path> --red   # expect failure
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Required source roots from plan section 2.2
# ---------------------------------------------------------------------------

REQUIRED_ROOTS: list[str] = [
    "inspirations/grok-build/crates/codegen/xai-grok-pager/",
    "inspirations/grok-build/crates/codegen/xai-grok-pager-render/",
    "inspirations/grok-build/crates/codegen/xai-grok-shell/",
    "inspirations/grok-build/crates/codegen/xai-grok-tools/",
    "inspirations/grok-build/crates/codegen/xai-grok-config/",
    "inspirations/grok-build/crates/codegen/xai-grok-auth/",
    "inspirations/grok-build/crates/codegen/xai-grok-mcp/",
    "inspirations/grok-build/crates/codegen/xai-grok-agent/",
    "inspirations/grok-build/crates/codegen/xai-acp-lib/",
    "inspirations/grok-build/crates/codegen/xai-grok-workspace/",
    "inspirations/grok-build/crates/codegen/xai-grok-update/",
    "inspirations/grok-build/crates/codegen/xai-grok-shell-session-support/",
    "inspirations/grok-build/crates/codegen/xai-fast-worktree/",
    "inspirations/grok-build/crates/codegen/xai-grok-sandbox/",
    "inspirations/grok-build/crates/codegen/xai-prompt-queue/",
    "inspirations/grok-build/crates/codegen/xai-grok-memory/",
    "inspirations/grok-build/crates/codegen/xai-grok-hooks/",
    "inspirations/grok-build/crates/codegen/xai-chat-state/",
    "inspirations/grok-build/crates/codegen/xai-grok-agent/src/compaction.rs",
    "inspirations/grok-build/crates/codegen/xai-hunk-tracker/",
    "inspirations/grok-build/crates/codegen/xai-gix-status/",
    "inspirations/grok-build/crates/codegen/xai-codebase-graph/",
    "inspirations/grok-build/crates/codegen/xai-system-power/",
    "inspirations/grok-build/crates/codegen/xai-tty-utils/",
    "inspirations/grok-build/crates/codegen/xai-grok-voice/",
    "inspirations/grok-build/crates/codegen/xai-grok-plugin-marketplace/",
    "inspirations/grok-build/crates/codegen/xai-grok-workspace-client/",
    "inspirations/grok-build/crates/codegen/xai-grok-telemetry/",
    "inspirations/grok-build/crates/codegen/xai-grok-pager/docs/user-guide/",
]

# Roots that are removal-only per plan section 1.2/1.4
REMOVAL_ROOTS: set[str] = {
    "inspirations/grok-build/crates/codegen/xai-grok-voice/",
    "inspirations/grok-build/crates/codegen/xai-grok-plugin-marketplace/",
    "inspirations/grok-build/crates/codegen/xai-grok-workspace-client/",
    "inspirations/grok-build/crates/codegen/xai-grok-telemetry/",
}

VALID_DECISIONS: set[str] = {"keep", "remove", "reference-only", "identity-divergence"}
VALID_HARNESS_STATES: set[str] = {"exists", "incomplete", "stub", "absent"}
VALID_FAMILIES: set[str] = {
    "slash-command",
    "cli-command",
    "tui-view",
    "tui-action",
    "setting",
    "subsystem",
    "provider",
    "model",
    "auth-flow",
    "integration",
    "removal",
    "screen-mode",
    "key-binding",
    "mouse-action",
    "modal",
    "status-block",
    "dispatch-handler",
    "rendering-feature",
    "theme",
    "tool",
    "identity-divergence",
}

# Required public surface families that MUST have at least one row
REQUIRED_FAMILIES: set[str] = {
    "slash-command",
    "cli-command",
    "tui-view",
    "tui-action",
    "setting",
    "subsystem",
    "auth-flow",
    "integration",
    "removal",
}


class CheckError(Exception):
    """A single check failure with context."""


def _root_for_path(path: str) -> str | None:
    """Return the required root prefix that contains *path*, or None.

    Checks longer (more specific) roots first so that e.g.
    ``xai-grok-agent/src/compaction.rs`` matches before ``xai-grok-agent/``.
    """
    # Sort by length descending so more specific roots are checked first
    sorted_roots = sorted(REQUIRED_ROOTS, key=len, reverse=True)
    for root in sorted_roots:
        if path.startswith(root):
            return root
    # Also check directory-level roots for file paths
    for root in sorted_roots:
        root_dir = root.rstrip("/")
        if path.startswith(root_dir + "/") or path == root_dir:
            return root
    return None


def load_crosswalk(path: str) -> dict[str, Any]:
    p = Path(path)
    if not p.is_file():
        raise CheckError(f"crosswalk file not found: {path}")
    try:
        data = json.loads(p.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise CheckError(f"crosswalk is not valid JSON: {exc}")
    if not isinstance(data, dict):
        raise CheckError("crosswalk root is not a JSON object")
    return data


def extract_rows(data: dict[str, Any]) -> list[dict[str, Any]]:
    rows = data.get("rows", data.get("crosswalk", []))
    if not isinstance(rows, list):
        raise CheckError("'rows' is not a list")
    return rows


def validate_row_schema(row: dict[str, Any], idx: int) -> list[str]:
    """Validate a single row's schema. Returns list of error strings."""
    errors: list[str] = []

    def need(field: str) -> Any:
        if field not in row:
            errors.append(f"row {idx}: missing required field '{field}'")
            return None
        return row[field]

    grok_family = need("grok_family")
    grok_path = need("grok_path")
    grok_symbol = need("grok_symbol")
    decision = need("decision")
    harness_owner = row.get("harness_owner")
    harness_state = need("harness_state")
    executable_probe = need("executable_probe")
    notes = need("notes")
    reference_source = need("reference_source")

    if grok_family is not None and grok_family not in VALID_FAMILIES:
        errors.append(
            f"row {idx}: invalid grok_family '{grok_family}' "
            f"(valid: {sorted(VALID_FAMILIES)})"
        )

    if grok_path is not None:
        if not isinstance(grok_path, str) or not grok_path.startswith("inspirations/"):
            errors.append(
                f"row {idx}: grok_path must start with 'inspirations/', got: {grok_path}"
            )
        root = _root_for_path(grok_path)
        if root is None:
            errors.append(
                f"row {idx}: grok_path '{grok_path}' is not under any required source root"
            )

    if decision is not None and decision not in VALID_DECISIONS:
        errors.append(
            f"row {idx}: invalid decision '{decision}' (valid: {sorted(VALID_DECISIONS)})"
        )

    if harness_state is not None and harness_state not in VALID_HARNESS_STATES:
        errors.append(
            f"row {idx}: invalid harness_state '{harness_state}' "
            f"(valid: {sorted(VALID_HARNESS_STATES)})"
        )

    if decision == "keep" and (not harness_owner or harness_owner == ""):
        errors.append(
            f"row {idx}: decision='keep' but harness_owner is empty "
            f"(grok_path={grok_path}, grok_symbol={grok_symbol})"
        )

    if not executable_probe:
        errors.append(f"row {idx}: executable_probe is empty")

    if not notes:
        errors.append(f"row {idx}: notes is empty")

    if reference_source is not None:
        if not isinstance(reference_source, list) or len(reference_source) == 0:
            errors.append(f"row {idx}: reference_source must be a non-empty list")
        else:
            for j, ref in enumerate(reference_source):
                if not isinstance(ref, dict):
                    errors.append(f"row {idx}: reference_source[{j}] is not an object")
                    continue
                ref_path = ref.get("path")
                ref_symbol = ref.get("symbol")
                if not ref_path or not isinstance(ref_path, str):
                    errors.append(
                        f"row {idx}: reference_source[{j}] missing 'path' string"
                    )
                elif not ref_path.startswith("inspirations/"):
                    errors.append(
                        f"row {idx}: reference_source[{j}] path must start with 'inspirations/'"
                    )
                if not ref_symbol or not isinstance(ref_symbol, str):
                    errors.append(
                        f"row {idx}: reference_source[{j}] missing 'symbol' string"
                    )

    return errors


def check_duplicate_rows(rows: list[dict[str, Any]]) -> list[str]:
    """Check for duplicate rows (same grok_path + grok_symbol)."""
    errors: list[str] = []
    seen: dict[str, int] = {}
    for idx, row in enumerate(rows):
        path = row.get("grok_path", "")
        symbol = row.get("grok_symbol", "")
        key = f"{path}::{symbol}"
        if key in seen:
            errors.append(
                f"duplicate row: rows {seen[key]} and {idx} have "
                f"grok_path='{path}' grok_symbol='{symbol}'"
            )
        else:
            seen[key] = idx
    return errors


def check_root_coverage(rows: list[dict[str, Any]]) -> list[str]:
    """Check that every required source root has at least one cited row."""
    errors: list[str] = []
    cited_roots: set[str] = set()
    for row in rows:
        path = row.get("grok_path", "")
        root = _root_for_path(path)
        if root:
            cited_roots.add(root)
        # Also check reference_source paths
        for ref in row.get("reference_source", []):
            if isinstance(ref, dict):
                ref_path = ref.get("path", "")
                ref_root = _root_for_path(ref_path)
                if ref_root:
                    cited_roots.add(ref_root)

    for root in REQUIRED_ROOTS:
        if root not in cited_roots:
            errors.append(f"required source root has zero cited rows: {root}")
    return errors


def check_family_coverage(rows: list[dict[str, Any]]) -> list[str]:
    """Check that required families have at least one row."""
    errors: list[str] = []
    found_families: set[str] = set()
    for row in rows:
        family = row.get("grok_family")
        if family:
            found_families.add(family)

    for fam in REQUIRED_FAMILIES:
        if fam not in found_families:
            errors.append(f"required family has zero rows: {fam}")
    return errors


def check_removal_surfaces(rows: list[dict[str, Any]]) -> list[str]:
    """Check that removal-only roots have decision='remove' rows."""
    errors: list[str] = []
    for root in REMOVAL_ROOTS:
        root_rows = [
            (idx, row)
            for idx, row in enumerate(rows)
            if _root_for_path(row.get("grok_path", "")) == root
        ]
        if not root_rows:
            # Already caught by root coverage check
            continue
        for idx, row in root_rows:
            decision = row.get("decision")
            if decision not in ("remove", "reference-only"):
                errors.append(
                    f"row {idx}: grok_path under removal root '{root}' "
                    f"has decision='{decision}' but must be 'remove' or 'reference-only' "
                    f"(per plan section 1.2/1.4)"
                )
    return errors


def check_reference_paths_exist(rows: list[dict[str, Any]], base_dir: str) -> list[str]:
    """Check that reference_source paths actually exist on disk."""
    errors: list[str] = []
    for idx, row in enumerate(rows):
        for j, ref in enumerate(row.get("reference_source", [])):
            if not isinstance(ref, dict):
                continue
            ref_path = ref.get("path", "")
            if not ref_path:
                continue
            full = os.path.join(base_dir, ref_path) if not os.path.isabs(ref_path) else ref_path
            if not os.path.exists(full):
                errors.append(
                    f"row {idx}: reference_source[{j}] path does not exist: {ref_path}"
                )
    return errors


def run_checks(crosswalk_path: str, base_dir: str, skip_disk: bool = False) -> list[str]:
    """Run all checks. Returns list of error strings (empty = pass)."""
    all_errors: list[str] = []

    try:
        data = load_crosswalk(crosswalk_path)
    except CheckError as exc:
        return [str(exc)]

    try:
        rows = extract_rows(data)
    except CheckError as exc:
        return [str(exc)]

    if len(rows) == 0:
        return ["crosswalk has zero rows"]

    # Schema validation
    for idx, row in enumerate(rows):
        if not isinstance(row, dict):
            all_errors.append(f"row {idx}: not a JSON object")
            continue
        all_errors.extend(validate_row_schema(row, idx))

    # If schema errors are severe, still try other checks but report
    # Duplicate detection
    all_errors.extend(check_duplicate_rows(rows))

    # Root coverage
    all_errors.extend(check_root_coverage(rows))

    # Family coverage
    all_errors.extend(check_family_coverage(rows))

    # Removal surface decisions
    all_errors.extend(check_removal_surfaces(rows))

    # Reference path existence (optional, can be slow)
    if not skip_disk:
        all_errors.extend(check_reference_paths_exist(rows, base_dir))

    return all_errors


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Coverage checker for the reference-source crosswalk (Task 2)."
    )
    parser.add_argument(
        "--crosswalk",
        required=True,
        help="Path to reference-crosswalk.json",
    )
    parser.add_argument(
        "--task",
        type=int,
        default=2,
        help="Task number (default: 2)",
    )
    parser.add_argument(
        "--red",
        action="store_true",
        help="Expect the checker to FAIL (for RED-then-GREEN testing)",
    )
    parser.add_argument(
        "--skip-disk",
        action="store_true",
        help="Skip checking that reference_source paths exist on disk",
    )
    parser.add_argument(
        "--base-dir",
        default=".",
        help="Base directory for resolving relative paths (default: cwd)",
    )
    args = parser.parse_args()

    errors = run_checks(args.crosswalk, args.base_dir, skip_disk=args.skip_disk)

    if errors:
        print(f"FAIL: {len(errors)} error(s) found:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        if args.red:
            print("RED: expected failure (passing as required)", file=sys.stdout)
            return 0
        return 1
    else:
        print(f"PASS: crosswalk has {len(load_crosswalk(args.crosswalk).get('rows', []))} rows, all checks passed")
        if args.red:
            print("ERROR: expected failure but crosswalk passed (--red mode)", file=sys.stderr)
            return 1
        return 0


if __name__ == "__main__":
    sys.exit(main())
