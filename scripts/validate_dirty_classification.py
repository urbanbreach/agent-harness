#!/usr/bin/env python3
"""Canonical dirty-path classification and scope-taxonomy validator.

Todo 8 integrates the Todo 3 scope classification.  This validator fails
closed unless every dirty path from the frozen starting state carries exactly
one valid classification, the classification counts and rules are internally
consistent, every scope family/sub-capability uses a valid disposition,
approved-exclusion families are provably approved by the approved-scope
document, and no row anywhere claims ``pass``.

Usage:
    python3 scripts/validate_dirty_classification.py \
        --starting-state <attemptDir>/task-1-grok-build-clean-room-parity/starting-state.json \
        --classification <attemptDir>/task-3-grok-build-clean-room-parity/path-classification.json \
        --scope <attemptDir>/task-3-grok-build-clean-room-parity/scope-taxonomy.json \
        --approved-scope .omo/drafts/grok-build-clean-room-parity.md
    python3 scripts/validate_dirty_classification.py --self-test
"""
# allow: SIZE_OK - one fail-closed validator for the paired classification and
# scope-taxonomy contracts plus a hermetic self-test.

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from collections import Counter
from pathlib import Path
from typing import Any, Final, NoReturn

VALID_PATH_CLASSES: Final = frozenset({
    "retain-and-prove", "rework", "replace", "retire-approved",
    "unrelated-preserve", "evidence-only", "suspected-contamination",
})
VALID_FAMILY_CLASSES: Final = frozenset({
    "implement", "retain-and-prove", "identity-substitute",
    "approved-exclusion", "external-proof-blocked",
})
PASS_CLAIM: Final = "pass"
DESCRIPTION: Final = "Canonical dirty-path classification and scope-taxonomy validator"
# Phrases (any one sufficient) the approved-scope document must contain to prove
# a plan-canonical approved-exclusion family. Unknown family ids fall back to the
# stricter all-tokens rule so novel exclusions stay unapproved by default.
APPROVAL_MARKERS: Final = {
    "voice-dictation-stt-tts": ("voice", "dictation", "stt", "tts"),
    "hosted-xai-services": ("hosted", "super grok", "billing", "telemetry", "analytics"),
    "enterprise-sso-deployment": ("enterprise", "sso", "oidc"),
    "remote-workspace-control-plane": ("remote workspace", "control plane", "remote hub"),
    "remote-mcp-oauth": ("remote mcp oauth", "mcp oauth", "pkce"),
}


def _normalized_text(text: str) -> str:
    return "".join(ch.lower() if ch.isalnum() else " " for ch in text)


def _exclusion_approved(family_id: str, normalized_document: str) -> bool:
    markers = APPROVAL_MARKERS.get(family_id)
    if markers is not None:
        return any(f" {marker} " in f" {normalized_document} " for marker in markers)
    return all(f" {token} " in f" {normalized_document} " for token in family_id.split("-") if token)


def _fail(message: str) -> NoReturn:
    raise ValueError(message)


def load_json_object(path: Path, label: str) -> dict[str, Any]:
    if not path.is_file():
        _fail(f"{label} must be an existing JSON file: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        _fail(f"{label} root must be a JSON object: {path}")
    return value


def validate_paths(
    starting_state: dict[str, Any],
    classification: dict[str, Any],
    defects: list[str],
) -> tuple[int, int, int]:
    """Return (coverage_percent, overlaps, pass_claims) for path classification."""
    dirty_entries = starting_state.get("dirty_paths")
    if not isinstance(dirty_entries, list) or not dirty_entries:
        # Fall back to legacy product_source_dirty git-status lines.
        dirty_entries = starting_state.get("product_source_dirty")
        if not isinstance(dirty_entries, list) or not dirty_entries:
            _fail("starting-state must contain a non-empty dirty_paths or product_source_dirty list")
        dirty = []
        for line in dirty_entries:
            line = str(line).strip()
            if not line:
                continue
            # Git porcelain v1 lines look like "XY path" or "XY orig -> rename".
            # Skip the first two status columns and take the last path token.
            parts = line.split()
            if len(parts) >= 2 and len(parts[0]) == 2:
                path_part = parts[-1]
            elif len(parts) >= 1:
                path_part = parts[-1]
            else:
                continue
            dirty.append(path_part)
    else:
        dirty = [str(entry["path"]) for entry in dirty_entries if isinstance(entry, dict) and "path" in entry]
    rows = classification.get("paths")
    if not isinstance(rows, list):
        _fail("classification paths must be a list")
    rules = classification.get("classification_rules")
    if not isinstance(rules, dict) or frozenset(rules) != VALID_PATH_CLASSES:
        defects.append("classification_rules must enumerate exactly the canonical classification set")
    seen: dict[str, int] = {}
    tally: Counter[str] = Counter()
    overlaps = 0
    pass_claims = 0
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            defects.append(f"paths[{index}]: not a JSON object")
            continue
        missing = [field for field in ("path", "classification", "reason") if field not in row]
        if missing:
            defects.append(f"paths[{index}]: missing fields {missing}")
            continue
        path = str(row["path"])
        kind = str(row["classification"])
        if path in seen:
            overlaps += 1
            defects.append(f"paths[{index}]: duplicate classification for '{path}' (also at {seen[path]})")
        seen[path] = index
        if kind == PASS_CLAIM:
            pass_claims += 1
        elif kind not in VALID_PATH_CLASSES:
            defects.append(f"paths[{index}]: invalid path classification '{kind}' for '{path}'")
        if not str(row["reason"]).strip():
            defects.append(f"paths[{index}]: empty reason for '{path}'")
        tally[kind] += 1
    classified = frozenset(seen)
    dirty_set = frozenset(dirty)
    unclassified = sorted(dirty_set - classified)
    foreign = sorted(classified - dirty_set)
    if unclassified:
        defects.append(f"unclassified dirty paths: {unclassified}")
    if foreign:
        defects.append(f"classification path is not dirty in starting state: {foreign}")
    counts = classification.get("counts")
    if isinstance(counts, dict) and dict(counts) != dict(tally):
        defects.append(f"counts block drift: recorded={dict(counts)} computed={dict(tally)}")
    total = len(dirty)
    coverage = int(100 * sum(1 for path in dirty if path in classified) / total)
    return coverage, overlaps, pass_claims


def validate_scope(
    scope: dict[str, Any],
    normalized_document: str,
    defects: list[str],
) -> tuple[int, int]:
    """Return (unapproved_exclusions, pass_claims) for the scope taxonomy."""
    families = scope.get("feature_families")
    if not isinstance(families, list) or not families:
        _fail("scope feature_families must be a non-empty list")
    unapproved = 0
    pass_claims = 0
    for index, family in enumerate(families):
        if not isinstance(family, dict):
            defects.append(f"feature_families[{index}]: not a JSON object")
            continue
        family_id = str(family.get("family_id", ""))
        classification_value = str(family.get("classification", ""))
        if not family_id or not str(family.get("description", "")).strip() or not str(family.get("plan_reference", "")).strip():
            defects.append(f"feature_families[{index}]: family_id/description/plan_reference are required")
        if classification_value == PASS_CLAIM:
            pass_claims += 1
        elif classification_value not in VALID_FAMILY_CLASSES:
            defects.append(f"feature_families[{index}]: invalid family classification '{classification_value}'")
        if classification_value == "approved-exclusion" and not _exclusion_approved(family_id, normalized_document):
            unapproved += 1
            defects.append(f"feature_families[{index}]: unapproved exclusion family '{family_id}' missing approval markers")
        for sub_index, sub in enumerate(family.get("sub_capabilities", []) or []):
            if not isinstance(sub, dict) or not str(sub.get("name", "")).strip():
                defects.append(f"feature_families[{index}].sub_capabilities[{sub_index}]: name is required")
                continue
            sub_value = str(sub.get("classification", ""))
            if sub_value == PASS_CLAIM:
                pass_claims += 1
            elif sub_value not in VALID_FAMILY_CLASSES:
                defects.append(f"feature_families[{index}].sub_capabilities[{sub_index}]: invalid classification '{sub_value}'")
            elif sub_value == "approved-exclusion" and classification_value != "approved-exclusion":
                unapproved += 1
                defects.append(
                    f"feature_families[{index}].sub_capabilities[{sub_index}]: "
                    "exclusion sub-capability outside an approved-exclusion family"
                )
    return unapproved, pass_claims


def run_validation(args: argparse.Namespace) -> dict[str, int]:
    starting_state = load_json_object(Path(args.starting_state), "starting-state")
    classification = load_json_object(Path(args.classification), "classification")
    scope = load_json_object(Path(args.scope), "scope-taxonomy")
    approved_path = Path(args.approved_scope)
    if not approved_path.is_file():
        _fail(f"approved-scope document must exist: {approved_path}")
    normalized_document = _normalized_text(approved_path.read_text(encoding="utf-8"))
    defects: list[str] = []
    coverage, overlaps, path_pass = validate_paths(starting_state, classification, defects)
    unapproved, scope_pass = validate_scope(scope, normalized_document, defects)
    metrics = {
        "coverage": coverage,
        "overlaps": overlaps,
        "unapproved_exclusions": unapproved,
        "pass_claims": path_pass + scope_pass,
    }
    if metrics["pass_claims"]:
        defects.append(f"pass claim detected ({metrics['pass_claims']})")
    if metrics["coverage"] < 100:
        defects.append(f"coverage {metrics['coverage']}% below required 100%")
    if metrics["overlaps"]:
        defects.append(f"overlaps {metrics['overlaps']} above required 0")
    if metrics["unapproved_exclusions"]:
        defects.append(f"unapproved exclusions {metrics['unapproved_exclusions']} above required 0")
    if defects:
        _fail("classification invalid: " + "; ".join(defects))
    return metrics


def _metrics_line(metrics: dict[str, int]) -> str:
    return (
        f"coverage={metrics['coverage']}% overlaps={metrics['overlaps']} "
        f"unapproved_exclusions={metrics['unapproved_exclusions']} pass_claims={metrics['pass_claims']}"
    )


# ---------------------------------------------------------------------------
# Hermetic self-test.
# ---------------------------------------------------------------------------

def _write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _build_fixture(root: Path) -> dict[str, Path]:
    starting_state = {
        "schema_version": "self-test-starting-state",
        "dirty_path_count": 3,
        "dirty_paths": [
            {"path": "crates/harness-tui/src/a.rs", "staged": ".M", "unstaged": "N..."},
            {"path": "crates/harness-core/src/b.rs", "staged": ".M", "unstaged": "N..."},
            {"path": "docs/capability-inventory.v1.json", "staged": ".M", "unstaged": "N..."},
        ],
    }
    classification = {
        "schema_version": "self-test-path-classification",
        "classification_rules": {kind: f"rule for {kind}" for kind in sorted(VALID_PATH_CLASSES)},
        "counts": {"evidence-only": 1, "retain-and-prove": 2},
        "paths": [
            {"path": "crates/harness-tui/src/a.rs", "classification": "retain-and-prove", "git_status": ".MN...", "reason": "in-scope product path"},
            {"path": "crates/harness-core/src/b.rs", "classification": "retain-and-prove", "git_status": ".MN...", "reason": "in-scope product path"},
            {"path": "docs/capability-inventory.v1.json", "classification": "evidence-only", "git_status": ".MN...", "reason": "parity manifest; not product code"},
        ],
    }
    scope = {
        "schema_version": "self-test-scope-taxonomy",
        "feature_families": [
            {
                "family_id": "terminal-input-decoding",
                "description": "Terminal input decoding and capability fallbacks",
                "classification": "implement",
                "plan_reference": "Todo 9",
                "sub_capabilities": [{"name": "key decoding", "classification": "implement"}],
            },
            {
                "family_id": "sessions-persistence-replay",
                "description": "Sessions, persistence, and replay",
                "classification": "retain-and-prove",
                "plan_reference": "Todo 17",
                "sub_capabilities": [{"name": "session resume", "classification": "retain-and-prove"}],
            },
            {
                "family_id": "voice-dictation-stt-tts",
                "description": "Voice/dictation/STT/TTS surfaces",
                "classification": "approved-exclusion",
                "plan_reference": "Must NOT have guardrail",
                "sub_capabilities": [{"name": "/voice command", "classification": "approved-exclusion"}],
            },
        ],
    }
    approved = root / "approved-scope.md"
    approved.write_text(
        "# Scope draft\n\nNo voice/dictation/STT/TTS surfaces.\n"
        "Terminal input decoding and sessions/persistence/replay stay in scope.\n",
        encoding="utf-8",
    )
    paths = {"starting_state": root / "starting-state.json", "classification": root / "path-classification.json", "scope": root / "scope-taxonomy.json", "approved": approved}
    _write_json(paths["starting_state"], starting_state)
    _write_json(paths["classification"], classification)
    _write_json(paths["scope"], scope)
    return paths


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="self-test argument shape")
    _ = parser.add_argument("--starting-state")
    _ = parser.add_argument("--classification")
    _ = parser.add_argument("--scope")
    _ = parser.add_argument("--approved-scope")
    return parser


def _argv(fixture: dict[str, Path]) -> list[str]:
    return [
        "--starting-state", str(fixture["starting_state"]),
        "--classification", str(fixture["classification"]),
        "--scope", str(fixture["scope"]),
        "--approved-scope", str(fixture["approved"]),
    ]


def self_test() -> int:
    parser = _parser()
    with tempfile.TemporaryDirectory() as tmp_dir:
        root = Path(tmp_dir)
        fixture = _build_fixture(root)
        base = _argv(fixture)
        metrics = run_validation(parser.parse_args(base))
        if _metrics_line(metrics) != "coverage=100% overlaps=0 unapproved_exclusions=0 pass_claims=0":
            _fail(f"self-test happy path produced unexpected metrics: {metrics}")
        print("PASS: happy classification validates with full coverage")
        cases = 9

        def expect_fail(label: str, argv: list[str], needle: str) -> None:
            try:
                run_validation(parser.parse_args(argv))
            except ValueError as error:
                if needle not in str(error):
                    _fail(f"self-test '{label}' rejected for the wrong reason: {error}")
                print(f"PASS: {label} rejected ({needle})")
                return
            _fail(f"self-test '{label}' unexpectedly accepted")

        def mutated_classification(label: str, needle: str, change: Any) -> None:
            document = json.loads(fixture["classification"].read_text(encoding="utf-8"))
            change(document)
            target = root / f"classification-{label}.json"
            _write_json(target, document)
            argv = base.copy()
            argv[argv.index("--classification") + 1] = str(target)
            expect_fail(label, argv, needle)

        def mutated_scope(label: str, needle: str, change: Any) -> None:
            document = json.loads(fixture["scope"].read_text(encoding="utf-8"))
            change(document)
            target = root / f"scope-{label}.json"
            _write_json(target, document)
            argv = base.copy()
            argv[argv.index("--scope") + 1] = str(target)
            expect_fail(label, argv, needle)

        mutated_classification("unclassified-path", "unclassified dirty paths", lambda d: d["paths"].pop(1))
        mutated_classification("duplicate-classification", "duplicate classification", lambda d: d["paths"].append(dict(d["paths"][0])))
        mutated_classification("invalid-class", "invalid path classification", lambda d: d["paths"].__setitem__(0, {**d["paths"][0], "classification": "promote-pass"}))
        mutated_classification("foreign-path", "not dirty in starting state", lambda d: d["paths"].append({"path": "crates/never-dirty.rs", "classification": "rework", "reason": "foreign"}))
        mutated_classification("counts-drift", "counts block drift", lambda d: d["counts"].__setitem__("retain-and-prove", 1))
        mutated_classification("missing-rule-key", "classification_rules must enumerate", lambda d: d["classification_rules"].pop("rework"))
        mutated_scope("unapproved-exclusion", "unapproved exclusion family", lambda d: d["feature_families"].__setitem__(2, {**d["feature_families"][2], "family_id": "quantum-flux-widgets"}))
        mutated_scope("pass-claim", "pass claim detected", lambda d: d["feature_families"][0].__setitem__("classification", "pass"))

        argv = base.copy()
        argv[argv.index("--approved-scope") + 1] = str(root / "missing-doc.md")
        try:
            run_validation(parser.parse_args(argv))
        except ValueError as error:
            if "approved-scope document must exist" not in str(error):
                _fail(f"self-test 'missing-approved-doc' rejected for the wrong reason: {error}")
            print("PASS: missing-approved-doc rejected (approved-scope document must exist)")
        else:
            _fail("self-test 'missing-approved-doc' unexpectedly accepted")

    print(f"self-test: {cases}/{cases} passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    _ = parser.add_argument("--starting-state", type=Path)
    _ = parser.add_argument("--classification", type=Path)
    _ = parser.add_argument("--scope", type=Path)
    _ = parser.add_argument("--approved-scope", type=Path)
    _ = parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    for flag in ("starting_state", "classification", "scope", "approved_scope"):
        if getattr(args, flag) is None:
            _fail(f"--{flag.replace('_', '-')} is required (or use --self-test)")
    metrics = run_validation(args)
    print(_metrics_line(metrics))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"dirty_classification_valid=false error={error}", file=sys.stderr)
        raise SystemExit(1) from error
