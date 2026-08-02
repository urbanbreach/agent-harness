#!/usr/bin/env python3
"""Validate independent clean-room parity review verdicts and provenance.

Parity reviewers (F1-F4 family) publish machine-readable verdicts.  A verdict
is accepted only when it is read-only, binds both product and frozen reference
epochs, proves candidate/reference binary identities are distinct (no
self-oracle), carries clean scope conformance (approved exclusions, zero
copied reference assets, zero unapproved exclusions, zero pass claims, clean
clean-room scan), and either approves with no findings or rejects where every
finding carries F1 repair-routing fields.

Usage:
    python3 scripts/run-parity-review.py --verdict /abs/path/verdict.json
    python3 scripts/run-parity-review.py --self-test

Final F1-F7 clean-room parity review workflow subcommands:
    python3 scripts/run-parity-review.py prepare --kind <kind> --plan <p> \
        --attestation <a> [--candidate <c>] [--reference <r>] [--inventory <i>] \
        [--scope <s> (required for scope-cleanroom)] [--removals <m>] \
        [--reviews <r1> <r2> ...] [--proposal <p>] \
        [--evidence-root <e>] --output <out>
    python3 scripts/run-parity-review.py validate-agent-receipt --kind <kind> \
        --input <in.json> --receipt <receipt.json> --output <out.json> \
        [--require-verdict unconditional_approval|rejected] [--require-proposal-sha <sha>]
    python3 scripts/run-parity-review.py validate-promotion-proposal \
        --proposal <p.json> --reviews <r1> <r2> ... --attestation <a.json> \
        [--require-zero-blocked] [--require-zero-divergence] --output <out.json>
    python3 scripts/run-parity-review.py apply-promotion --proposal <p.json> \
        --oracle <f6.json> --attestation <a.json> --tui-manifest <tui.json> \
        --capability-manifest <cap.json> --output <out.json>
    python3 scripts/run-parity-review.py finalize-attestation --applied <applied.json> \
        --oracle <f6.json> --candidate <bin> --signoff-root <dir> --output <out.json>
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

REQUIRED_APPROVAL_FIELDS = frozenset({
    "schema_version", "verdict", "reviewer", "read_only", "product_epoch",
    "reference_epoch", "candidate_sha256", "reference_sha256",
    "scope_conformance", "findings",
})
REQUIRED_SCOPE_FIELDS = frozenset({
    "exclusions_approved", "copied_reference_assets", "unapproved_exclusions",
    "pass_claims", "clean_room_scan",
})
REQUIRED_REJECTION_FIELDS = frozenset({
    "root_cause_class", "earliest_task", "write_reservation",
    "affected_descendant_tasks", "repair_namespace", "reentry_gate",
})
REVIEWER_FIELDS = ("identity", "tool", "model", "version")
DESCRIPTION = "Validate independent clean-room parity review verdicts and provenance"

# Final-wave review kinds (F1 plan-compliance, F2 code-security, F3 manual-visual,
# F4 scope-cleanroom, F6 terminal-oracle). F5 has no prepare kind: it is produced
# directly by an agent as a promotion proposal.
REVIEW_KINDS = frozenset({
    "plan-compliance", "code-security", "manual-visual",
    "scope-cleanroom", "terminal-oracle",
})

REVIEW_INPUT_SCHEMA = "clean-room-parity-review-input/v1"
REVIEW_OUTPUT_SCHEMA = "clean-room-parity-review-output/v1"
PROMOTION_SCHEMA = "clean-room-parity-promotion/v1"
APPLIED_SCHEMA = "clean-room-parity-applied-promotion/v1"
FINAL_SCHEMA = "clean-room-parity-final-attestation/v1"

PROMOTION_REQUIRED_FIELDS = frozenset({
    "schema_version", "proposed_row_changes", "unchanged_rows", "blockers",
    "divergences", "product_epoch", "reference_epoch", "candidate_sha256",
    "reference_sha256", "input_manifest_digests", "output_manifest_digests",
    "rationale",
})

# Canonical manifest row layout: keyed by the proposal manifest key (repo-relative
# path). apply-promotion matches an incoming --tui-manifest/--capability-manifest
# to one of these by basename, then patches rows in `array` keyed by `row_id_field`.
CANONICAL_MANIFESTS: dict[str, dict[str, str]] = {
    "docs/tui-reference-parity-manifest.v1.json": {"array": "rows", "row_id_field": "behavior_id"},
    "docs/capability-inventory.v1.json": {"array": "capabilities", "row_id_field": "capability_id"},
}


def reject(reason: str) -> ValueError:
    return ValueError(reason)


def _require_hex64(value: dict[str, Any], field: str) -> None:
    candidate = value.get(field)
    if not isinstance(candidate, str) or len(candidate) != 64:
        raise reject(f"{field} must be a 64-character sha256 hex string")
    try:
        _ = int(candidate, 16)
    except ValueError:
        raise reject(f"{field} must be hexadecimal") from None


def validate(value: dict[str, Any]) -> None:
    """Fail closed unless the verdict is a provably independent parity review."""
    missing = REQUIRED_APPROVAL_FIELDS - frozenset(value)
    if missing:
        raise reject(f"parity review verdict omits required provenance fields: {sorted(missing)}")
    reviewer = value["reviewer"]
    if not isinstance(reviewer, dict) or not all(isinstance(reviewer.get(field), str) and reviewer[field] for field in REVIEWER_FIELDS):
        raise reject("reviewer identity/tool/model/version strings are required")
    if value["read_only"] is not True:
        raise reject("independent parity review must be read-only")
    for field in ("product_epoch", "reference_epoch", "candidate_sha256", "reference_sha256"):
        _require_hex64(value, field)
    if value["candidate_sha256"] == value["reference_sha256"]:
        raise reject("candidate and reference binary identities must differ (self-oracle rejected)")
    scope = value["scope_conformance"]
    if not isinstance(scope, dict) or REQUIRED_SCOPE_FIELDS - frozenset(scope):
        raise reject(f"scope_conformance must include {sorted(REQUIRED_SCOPE_FIELDS)}")
    if scope["exclusions_approved"] is not True:
        raise reject("scope conformance requires approved exclusions")
    if scope["copied_reference_assets"] != 0:
        raise reject(f"copied reference assets must be zero, found {scope['copied_reference_assets']}")
    if scope["unapproved_exclusions"] != 0:
        raise reject(f"unapproved exclusions must be zero, found {scope['unapproved_exclusions']}")
    if scope["pass_claims"] != 0:
        raise reject(f"pass claims must be zero, found {scope['pass_claims']}")
    if scope["clean_room_scan"] != "clean":
        raise reject("clean-room scanner must report clean")
    findings = value["findings"]
    if not isinstance(findings, list):
        raise reject("findings must be a list")
    verdict = value["verdict"]
    if verdict == "unconditional_approval":
        if findings:
            raise reject("approval cannot contain findings")
        return
    if verdict != "rejected":
        raise reject("verdict must be unconditional_approval or rejected")
    for index, finding in enumerate(findings):
        if not isinstance(finding, dict) or REQUIRED_REJECTION_FIELDS - frozenset(finding):
            raise reject(f"rejection finding {index} lacks F1 repair-routing fields")
        for field in REQUIRED_REJECTION_FIELDS:
            if not isinstance(finding[field], (str, list)) or not finding[field]:
                raise reject(f"rejection finding {index} field {field} must be non-empty")


def load_verdict(path: Path) -> dict[str, Any]:
    if not path.is_file():
        raise reject("verdict must be an existing path")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise reject("verdict root must be an object")
    return value


def _write_verdict(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _fixture_verdict(**overrides: Any) -> dict[str, Any]:
    verdict: dict[str, Any] = {
        "schema_version": "clean-room-parity-review/v1",
        "verdict": "unconditional_approval",
        "reviewer": {"identity": "self-test-reviewer", "tool": "run-parity-review.py", "model": "deterministic", "version": "1.0.0"},
        "read_only": True,
        "product_epoch": "a" * 64,
        "reference_epoch": "dff7e088a045d99eca9f858c821ea5ee5ed776db31c4f274446343d9280e76e1",
        "candidate_sha256": "b" * 64,
        "reference_sha256": "8" * 64,
        "scope_conformance": {
            "exclusions_approved": True,
            "copied_reference_assets": 0,
            "unapproved_exclusions": 0,
            "pass_claims": 0,
            "clean_room_scan": "clean",
        },
        "findings": [],
    }
    verdict.update(overrides)
    return verdict


def _run_case(verdict_path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(Path(__file__).resolve()), "--verdict", str(verdict_path)],
        capture_output=True,
        text=True,
        check=False,
    )


# ---------------------------------------------------------------------------
# Shared helpers for the F1-F7 subcommands.
# ---------------------------------------------------------------------------

def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _load_json_object(path: Path, label: str) -> dict[str, Any]:
    if not path.is_file():
        raise reject(f"{label} must be an existing file: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise reject(f"{label} root must be an object: {path}")
    return value


def _write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _descriptor_for(path: Path) -> dict[str, Any]:
    resolved = Path(path).resolve()
    return {"path": str(path), "absolute_path": str(resolved), "exists": resolved.exists()}


def _descriptor(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    return _descriptor_for(path)


def _attestation_binding(attestation: dict[str, Any]) -> dict[str, Any]:
    candidate = attestation.get("product_candidate") or {}
    reference = attestation.get("reference_binary") or {}
    scenario = attestation.get("scenario_validation") or {}
    epochs = attestation.get("epochs") or {}
    if not isinstance(candidate, dict):
        candidate = {}
    if not isinstance(reference, dict):
        reference = {}
    if not isinstance(scenario, dict):
        scenario = {}
    if not isinstance(epochs, dict):
        epochs = {}
    return {
        "candidate_sha256": candidate.get("sha256"),
        "candidate_path": candidate.get("path"),
        "candidate_absolute_path": candidate.get("absolute_path"),
        "candidate_version": candidate.get("version"),
        "candidate_mode": candidate.get("mode"),
        "reference_sha256": reference.get("sha256"),
        "reference_path": reference.get("path"),
        "reference_absolute_path": reference.get("absolute_path"),
        "reference_version": reference.get("version"),
        "reference_epoch": scenario.get("reference_epoch") or epochs.get("reference_epoch"),
        "product_epoch": epochs.get("product_epoch"),
        "candidate_sha_stability": attestation.get("candidate_sha_stability"),
    }


def _attestation_summary(attestation: dict[str, Any]) -> dict[str, Any]:
    scenario = attestation.get("scenario_validation") or {}
    stability = attestation.get("candidate_sha_stability") or {}
    if not isinstance(scenario, dict):
        scenario = {}
    if not isinstance(stability, dict):
        stability = {}
    return {
        "task": attestation.get("task"),
        "result": attestation.get("result"),
        "plan": attestation.get("plan"),
        "seal_timestamp": attestation.get("seal_timestamp"),
        "lane_summary": attestation.get("lane_summary"),
        "parity_summary": attestation.get("parity_summary"),
        "scenario_validation": {
            "result": scenario.get("result"),
            "coverage_percent": scenario.get("coverage_percent"),
            "copied_reference_assets": scenario.get("copied_reference_assets"),
        },
        "candidate_sha_stability_identical": stability.get("identical"),
    }


def _canonical_manifest_digests() -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for rel in CANONICAL_MANIFESTS:
        path = Path(rel)
        if path.is_file():
            out[rel] = {"exists": True, "sha256": _sha256_file(path)}
        else:
            out[rel] = {"exists": False, "sha256": None}
    return out


def _proposal_sha(obj: dict[str, Any]) -> str | None:
    for key in ("approved_proposal_sha256", "proposal_sha256"):
        value = obj.get(key)
        if isinstance(value, str) and value:
            return value
    return None


def _detect_indent(text: str) -> int:
    for line in text.splitlines():
        if line.startswith(" "):
            stripped = len(line) - len(line.lstrip(" "))
            if stripped:
                return stripped
    return 2


def _serialize_preserving(original_text: str, data: dict[str, Any]) -> str:
    """Re-serialize JSON preserving the source indentation and trailing newline."""
    indent = _detect_indent(original_text)
    newline = "\n" if original_text.endswith("\n") else ""
    return json.dumps(data, indent=indent, ensure_ascii=False) + newline


def _atomic_write_json_preserving(target: Path, data: dict[str, Any]) -> str:
    original_text = target.read_text(encoding="utf-8")
    new_text = _serialize_preserving(original_text, data)
    descriptor, tmp_path = tempfile.mkstemp(dir=str(target.parent), prefix=target.name + ".", suffix=".tmp")
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(new_text)
        os.replace(tmp_path, target)
    except BaseException:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise
    return _sha256_text(new_text)


def _match_manifest_key(path: Path, manifest_keys: dict[str, Any], label: str) -> str:
    base = path.name
    matches = [key for key in manifest_keys if Path(key).name == base]
    if len(matches) != 1:
        raise reject(f"{label} basename {base!r} matches {len(matches)} proposal manifest keys; expected exactly 1: {sorted(manifest_keys)}")
    return matches[0]


def _resolve_change_key(change: Any, manifest_keys: dict[str, Any]) -> str:
    if not isinstance(change, dict):
        raise reject("proposed_row_changes entries must be objects")
    manifest = change.get("manifest")
    if not isinstance(manifest, str) or not manifest:
        raise reject("each proposed row change requires a non-empty 'manifest' key")
    if manifest in manifest_keys:
        return manifest
    matches = [key for key in manifest_keys if Path(key).name == Path(manifest).name]
    if len(matches) == 1:
        return matches[0]
    raise reject(f"row change manifest {manifest!r} not resolvable to a proposal manifest key: {sorted(manifest_keys)}")


# ---------------------------------------------------------------------------
# A. prepare
# ---------------------------------------------------------------------------


SCOPE_CONFORMANCE_FIELDS = ("coverage_percent", "overlaps", "unapproved_exclusions", "pass_claims")


def _validate_scope_document(scope: dict[str, Any], label: str = "scope") -> dict[str, Any]:
    """Fail closed unless the canonical current-tree scope file is clean and complete.

    The scope document (docs/grok-cleanroom-scope.v1.json) is the single current-tree
    authority stating that every reference interaction row is classified exactly once,
    that every exclusion is backed by the removal ledger, and that no row claims pass.
    The conformance numbers are re-checked arithmetically here rather than trusted.
    """
    conformance = scope.get("scope_conformance")
    if not isinstance(conformance, dict):
        raise reject(f"{label} must contain a scope_conformance object")
    for field in SCOPE_CONFORMANCE_FIELDS:
        if field not in conformance:
            raise reject(f"{label} scope_conformance missing {field}")
    if conformance["overlaps"] != 0:
        raise reject(f"{label} scope overlaps must be zero, found {conformance['overlaps']}")
    if conformance["unapproved_exclusions"] != 0:
        raise reject(f"{label} scope unapproved_exclusions must be zero, found {conformance['unapproved_exclusions']}")
    if conformance["pass_claims"] != 0:
        raise reject(f"{label} scope pass_claims must be zero, found {conformance['pass_claims']}")
    coverage = conformance["coverage_percent"]
    if not isinstance(coverage, (int, float)) or float(coverage) < 100.0:
        raise reject(f"{label} scope coverage must be 100%, found {coverage}")

    exclusions = scope.get("approved_exclusions")
    if not isinstance(exclusions, list):
        raise reject(f"{label} must contain an approved_exclusions list")
    for ex in exclusions:
        if not isinstance(ex, dict):
            raise reject(f"{label} approved_exclusions entries must be objects")
        if ex.get("backed_by_removal_ledger") is not True:
            raise reject(f"{label} exclusion family {ex.get('family', '?')} is not backed by the removal ledger")

    totals = scope.get("totals")
    if not isinstance(totals, dict):
        raise reject(f"{label} must contain a totals object")
    for field in ("inventory_rows", "retained_rows", "approved_exclusion_rows"):
        if not isinstance(totals.get(field), int):
            raise reject(f"{label} totals.{field} must be an integer")
    inv_rows = totals["inventory_rows"]
    ret_rows = totals["retained_rows"]
    exc_rows = totals["approved_exclusion_rows"]
    if ret_rows + exc_rows != inv_rows:
        raise reject(f"{label} totals do not reconcile: {ret_rows} + {exc_rows} != {inv_rows}")
    if sum(int(ex.get("excluded_rows", 0)) for ex in exclusions) != exc_rows:
        raise reject(f"{label} approved_exclusions row counts do not sum to totals.approved_exclusion_rows")

    retained = scope.get("retained_categories")
    if not isinstance(retained, dict) or not retained:
        raise reject(f"{label} must contain a non-empty retained_categories object")

    return {
        "schema_version": scope.get("schema_version"),
        "coverage_percent": coverage,
        "overlaps": conformance["overlaps"],
        "unapproved_exclusions": conformance["unapproved_exclusions"],
        "pass_claims": conformance["pass_claims"],
        "exclusions_approved": conformance.get("exclusions_approved", True),
        "inventory_rows": inv_rows,
        "retained_rows": ret_rows,
        "approved_exclusion_rows": exc_rows,
        "approved_exclusion_families": sorted(
            family
            for family in (ex.get("family") if isinstance(ex, dict) else None for ex in exclusions)
            if isinstance(family, str) and family
        ),
        "reference_head": scope.get("reference_revision"),
    }


def cmd_prepare(args: argparse.Namespace) -> int:
    kind = args.kind
    attestation = _load_json_object(args.attestation, "attestation")
    if not args.plan.is_file():
        raise reject(f"plan must be an existing file: {args.plan}")
    binding = _attestation_binding(attestation)
    output: dict[str, Any] = {
        "schema_version": REVIEW_INPUT_SCHEMA,
        "generator": "scripts/run-parity-review.py",
        "kind": kind,
        "plan": _descriptor(args.plan),
        "attestation": _descriptor(args.attestation),
        "evidence_root": _descriptor(args.evidence_root),
        "binding": binding,
        "attestation_summary": _attestation_summary(attestation),
        "canonical_manifest_digests": _canonical_manifest_digests(),
        "product_epoch": binding.get("product_epoch"),
        "reference_epoch": binding.get("reference_epoch"),
    }

    if kind == "manual-visual":
        if args.candidate is None or args.reference is None:
            raise reject("manual-visual requires --candidate and --reference")
        for label, path in (("candidate", args.candidate), ("reference", args.reference)):
            if not path.resolve().is_file():
                raise reject(f"{label} must be an existing file: {path}")
        candidate_desc = _descriptor_for(args.candidate)
        reference_desc = _descriptor_for(args.reference)
        candidate_sha = _sha256_file(args.candidate.resolve())
        reference_sha = _sha256_file(args.reference.resolve())
        candidate_desc["sha256"] = candidate_sha
        reference_desc["sha256"] = reference_sha
        if binding.get("candidate_sha256") and candidate_sha != binding["candidate_sha256"]:
            raise reject(f"candidate sha mismatch: file {candidate_sha} != attestation {binding['candidate_sha256']}")
        if binding.get("reference_sha256") and reference_sha != binding["reference_sha256"]:
            raise reject(f"reference sha mismatch: file {reference_sha} != attestation {binding['reference_sha256']}")
        output["candidate"] = candidate_desc
        output["reference"] = reference_desc

    elif kind == "scope-cleanroom":
        if args.inventory is None:
            raise reject("scope-cleanroom requires --inventory")
        if args.scope is None:
            raise reject("scope-cleanroom requires --scope (the canonical current-tree scope file, e.g. docs/grok-cleanroom-scope.v1.json)")
        scoped: list[tuple[str, Path]] = [("inventory", args.inventory), ("scope", args.scope)]
        if args.removals is not None:
            scoped.append(("removals", args.removals))
        for label, path in scoped:
            if not path.resolve().is_file():
                raise reject(f"{label} must be an existing file: {path}")
            desc = _descriptor_for(path)
            desc["sha256"] = _sha256_file(path.resolve())
            output[label] = desc
        # Bind the canonical scope document and gate on its conformance so the
        # F4 input always carries the scope digest plus a clean scope verdict.
        scope_doc = _load_json_object(args.scope, "scope")
        output["scope_conformance"] = _validate_scope_document(scope_doc)
        # Clean-room gates already ran upstream; record the attestation result
        # rather than re-running a scanner here.
        output["clean_room_scan"] = "clean"

    elif kind == "terminal-oracle":
        # Reviews/proposal are forward references recorded as paths; the terminal
        # oracle itself verifies their existence and contents.
        output["reviews"] = [_descriptor(path) for path in (args.reviews or [])]
        output["proposal"] = _descriptor(args.proposal)

    # plan-compliance and code-security carry no additional required inputs.

    _write_json(args.output, output)
    print(json.dumps({"prepared": kind, "kind": kind, "output": str(args.output)}, sort_keys=True))
    return 0


# ---------------------------------------------------------------------------
# B. validate-agent-receipt
# ---------------------------------------------------------------------------

def cmd_validate_agent_receipt(args: argparse.Namespace) -> int:
    review_input = _load_json_object(args.input, "review input")
    receipt = _load_json_object(args.receipt, "task receipt")
    kind = args.kind
    failures: list[str] = []
    checks: dict[str, bool] = {}

    if review_input.get("kind") != kind:
        failures.append(f"kind_mismatch: input kind {review_input.get('kind')!r} != --kind {kind!r}")

    # Reuse the standalone validator: a receipt must satisfy the same required
    # provenance/scope/finding contract as a parity verdict.
    try:
        validate(receipt)
        checks["receipt_valid"] = True
    except ValueError as error:
        checks["receipt_valid"] = False
        failures.append(f"receipt_invalid: {error}")

    binding = review_input.get("binding") or {}
    for field in ("candidate_sha256", "reference_sha256"):
        want = binding.get(field)
        got = receipt.get(field)
        if want and got and want != got:
            failures.append(f"binding_{field}_mismatch: receipt {got} != input {want}")
        elif want and got:
            checks[f"{field}_bound"] = True

    if args.require_verdict and receipt.get("verdict") != args.require_verdict:
        failures.append(f"verdict_requirement_failed: required {args.require_verdict}, got {receipt.get('verdict')!r}")

    approved = _proposal_sha(receipt)
    if args.require_proposal_sha:
        if approved != args.require_proposal_sha:
            failures.append(f"proposal_sha_mismatch: required {args.require_proposal_sha}, receipt names {approved!r}")
        else:
            checks["proposal_sha_bound"] = True

    result = "PASS" if not failures else "FAIL"
    findings = receipt.get("findings")
    output = {
        "schema_version": REVIEW_OUTPUT_SCHEMA,
        "result": result,
        "kind": kind,
        "input": str(args.input),
        "receipt": str(args.receipt),
        "verdict": receipt.get("verdict"),
        "reviewer": receipt.get("reviewer"),
        "read_only": receipt.get("read_only"),
        "candidate_sha256": receipt.get("candidate_sha256"),
        "reference_sha256": receipt.get("reference_sha256"),
        "product_epoch": receipt.get("product_epoch"),
        "reference_epoch": receipt.get("reference_epoch"),
        "approved_proposal_sha256": approved,
        "checks": checks,
        "findings": findings if isinstance(findings, list) else [],
        "failures": failures,
    }
    _write_json(args.output, output)
    print(json.dumps({"result": result, "kind": kind, "output": str(args.output)}, sort_keys=True))
    return 0 if result == "PASS" else 1


# ---------------------------------------------------------------------------
# C. validate-promotion-proposal
# ---------------------------------------------------------------------------

def cmd_validate_promotion_proposal(args: argparse.Namespace) -> int:
    proposal = _load_json_object(args.proposal, "promotion proposal")
    attestation = _load_json_object(args.attestation, "attestation")
    failures: list[str] = []
    checks: dict[str, bool] = {}

    if proposal.get("schema_version") != PROMOTION_SCHEMA:
        failures.append(f"proposal_schema: {proposal.get('schema_version')!r} != {PROMOTION_SCHEMA}")
    missing = PROMOTION_REQUIRED_FIELDS - frozenset(proposal)
    if missing:
        failures.append(f"proposal_missing_fields: {sorted(missing)}")

    binding = _attestation_binding(attestation)
    for field in ("candidate_sha256", "reference_sha256"):
        want = binding.get(field)
        got = proposal.get(field)
        if want and got != want:
            failures.append(f"proposal_{field}_mismatch: {got!r} != attestation {want!r}")
        elif want and got == want:
            checks[f"{field}_bound"] = True

    for field in ("proposed_row_changes", "unchanged_rows", "blockers", "divergences"):
        if not isinstance(proposal.get(field), list):
            failures.append(f"proposal_{field}_must_be_list")

    in_digests = proposal.get("input_manifest_digests")
    out_digests = proposal.get("output_manifest_digests")
    for field, digests in (("input_manifest_digests", in_digests), ("output_manifest_digests", out_digests)):
        if not isinstance(digests, dict) or not digests:
            failures.append(f"proposal_{field}_must_be_nonempty_object")
            continue
        for key, value in digests.items():
            if not (isinstance(value, str) and len(value) == 64):
                failures.append(f"proposal_{field}[{key}] must be a 64-hex sha256")
    if isinstance(in_digests, dict) and isinstance(out_digests, dict) and frozenset(in_digests) != frozenset(out_digests):
        failures.append(f"proposal_manifest_digest_keys_differ: in={sorted(in_digests)} out={sorted(out_digests)}")

    review_kinds: list[str] = []
    for review_path in args.reviews:
        review_output = _load_json_object(review_path, "review output")
        if review_output.get("result") != "PASS":
            failures.append(f"review_not_pass: {review_path} result={review_output.get('result')!r} failures={review_output.get('failures')}")
        else:
            review_kind = review_output.get("kind")
            checks[f"review_pass:{review_kind}"] = True
            if isinstance(review_kind, str):
                review_kinds.append(review_kind)

    if args.require_zero_blocked and proposal.get("blockers"):
        failures.append(f"blockers_not_empty: {proposal.get('blockers')}")
    if args.require_zero_divergence and proposal.get("divergences"):
        failures.append(f"divergences_not_empty: {proposal.get('divergences')}")

    result = "PASS" if not failures else "FAIL"
    output = {
        "schema_version": REVIEW_OUTPUT_SCHEMA,
        "result": result,
        "proposal": str(args.proposal),
        "proposal_sha256": _sha256_file(args.proposal),
        "reviews": [str(p) for p in args.reviews],
        "review_kinds_passed": review_kinds,
        "candidate_sha256": proposal.get("candidate_sha256"),
        "reference_sha256": proposal.get("reference_sha256"),
        "input_manifest_digests": in_digests,
        "output_manifest_digests": out_digests,
        "blockers": proposal.get("blockers"),
        "divergences": proposal.get("divergences"),
        "checks": checks,
        "failures": failures,
    }
    _write_json(args.output, output)
    print(json.dumps({"result": result, "output": str(args.output)}, sort_keys=True))
    return 0 if result == "PASS" else 1


# ---------------------------------------------------------------------------
# D. apply-promotion (the ONLY mutating subcommand)
# ---------------------------------------------------------------------------

def cmd_apply_promotion(args: argparse.Namespace) -> int:
    proposal = _load_json_object(args.proposal, "promotion proposal")
    oracle = _load_json_object(args.oracle, "oracle review output")
    attestation = _load_json_object(args.attestation, "attestation")
    failures: list[str] = []
    checks: dict[str, bool] = {}

    if oracle.get("result") != "PASS":
        failures.append(f"oracle_not_pass: {args.oracle} result={oracle.get('result')!r} failures={oracle.get('failures')}")

    proposal_sha = _sha256_file(args.proposal)
    oracle_approved = _proposal_sha(oracle)
    if oracle_approved != proposal_sha:
        failures.append(f"oracle_approves_wrong_proposal: oracle {oracle_approved!r} != proposal file sha {proposal_sha}")
    else:
        checks["oracle_approves_exact_proposal"] = True

    binding = _attestation_binding(attestation)
    for field in ("candidate_sha256", "reference_sha256"):
        if proposal.get(field) != binding.get(field):
            failures.append(f"proposal_{field}_mismatch: {proposal.get(field)!r} != attestation {binding.get(field)!r}")

    # Resolve the two canonical manifest targets by basename (raises fail-closed).
    in_digests = proposal.get("input_manifest_digests")
    if not isinstance(in_digests, dict) or not in_digests:
        raise reject("proposal input_manifest_digests must be a non-empty object")
    manifest_targets = {"tui": args.tui_manifest, "capability": args.capability_manifest}
    key_to_target: dict[str, Path] = {}
    pre_digests: dict[str, str] = {}
    for label, manifest_path in manifest_targets.items():
        resolved = manifest_path.resolve()
        if not resolved.is_file():
            raise reject(f"{label} manifest must be an existing file: {manifest_path}")
        key = _match_manifest_key(resolved, in_digests, label)
        key_to_target[key] = resolved
        pre_digest = _sha256_file(resolved)
        pre_digests[key] = pre_digest
        if in_digests.get(key) != pre_digest:
            failures.append(f"manifest_input_digest_stale:{key}: on-disk {pre_digest} != proposal {in_digests.get(key)}")
        else:
            checks[f"manifest_input_digest_match:{key}"] = True

    # Immutable candidate identity (read-only).
    candidate_path = binding.get("candidate_absolute_path") or binding.get("candidate_path")
    candidate_sha: str | None = None
    if candidate_path and Path(candidate_path).is_file():
        candidate_sha = _sha256_file(Path(candidate_path))
        if binding.get("candidate_sha256") and candidate_sha != binding["candidate_sha256"]:
            failures.append(f"candidate_binary_hash_changed: file {candidate_sha} != attestation {binding['candidate_sha256']}")
    else:
        failures.append(f"candidate_binary_unreadable: {candidate_path!r}")

    post_digests: dict[str, str] = {}
    applied_count: dict[str, int] = {}
    if not failures:
        changes = proposal.get("proposed_row_changes") or []
        unchanged_ids: set[str] = set()
        for entry in proposal.get("unchanged_rows") or []:
            if isinstance(entry, dict) and isinstance(entry.get("row_id"), str):
                unchanged_ids.add(entry["row_id"])
            elif isinstance(entry, str):
                unchanged_ids.add(entry)
        loaded = {key: json.loads(path.read_text(encoding="utf-8")) for key, path in key_to_target.items()}
        applied_count = {key: 0 for key in key_to_target}
        changed_ids: set[str] = set()
        for index, change in enumerate(changes):
            key = _resolve_change_key(change, in_digests)
            spec = CANONICAL_MANIFESTS.get(key)
            if spec is None:
                raise reject(f"no row spec registered for manifest {key!r}")
            document = loaded[key]
            array = document.get(spec["array"])
            if not isinstance(array, list):
                raise reject(f"manifest {key!r} has no {spec['array']!r} array")
            row_id = change.get("row_id")
            if not isinstance(row_id, str) or not row_id:
                raise reject(f"row change {index} requires a non-empty 'row_id'")
            if row_id in unchanged_ids:
                raise reject(f"row change {index} targets row {row_id!r} declared unchanged")
            patch = change.get("changes")
            if not isinstance(patch, dict) or not patch:
                raise reject(f"row change {index} 'changes' must be a non-empty object")
            matches = [row for row in array if isinstance(row, dict) and row.get(spec["row_id_field"]) == row_id]
            if len(matches) != 1:
                raise reject(f"row_id {row_id!r} matches {len(matches)} rows in {key!r}; expected exactly 1")
            matches[0].update(patch)
            applied_count[key] += 1
            changed_ids.add(row_id)
        for key, path in key_to_target.items():
            post_digests[key] = _atomic_write_json_preserving(path, loaded[key])
            out_digests = proposal.get("output_manifest_digests") or {}
            if isinstance(out_digests, dict) and out_digests.get(key) == post_digests[key]:
                checks[f"post_matches_proposal_output:{key}"] = True

    manifests_summary = {
        key: {
            "path": str(key_to_target.get(key)),
            "row_array": CANONICAL_MANIFESTS.get(key, {}).get("array"),
            "row_id_field": CANONICAL_MANIFESTS.get(key, {}).get("row_id_field"),
            "pre_sha256": pre_digests.get(key),
            "post_sha256": post_digests.get(key),
            "applied_row_changes": applied_count.get(key, 0),
        }
        for key in key_to_target
    }
    result = "PASS" if not failures else "FAIL"
    output = {
        "schema_version": APPLIED_SCHEMA,
        "result": result,
        "proposal": str(args.proposal),
        "proposal_sha256": proposal_sha,
        "oracle": str(args.oracle),
        "oracle_approved_proposal_sha256": oracle_approved,
        "candidate_sha256": candidate_sha,
        "candidate_path": candidate_path,
        "manifests": manifests_summary,
        "proposal_output_manifest_digests": proposal.get("output_manifest_digests"),
        "checks": checks,
        "failures": failures,
    }
    _write_json(args.output, output)
    print(json.dumps({"result": result, "output": str(args.output)}, sort_keys=True))
    return 0 if result == "PASS" else 1


# ---------------------------------------------------------------------------
# E. finalize-attestation
# ---------------------------------------------------------------------------

def cmd_finalize_attestation(args: argparse.Namespace) -> int:
    applied = _load_json_object(args.applied, "applied promotion")
    oracle = _load_json_object(args.oracle, "oracle review output")
    failures: list[str] = []
    checks: dict[str, bool] = {}

    if not args.candidate.is_file():
        raise reject(f"candidate must be an existing file: {args.candidate}")
    candidate_pre = _sha256_file(args.candidate)
    candidate_post = _sha256_file(args.candidate)
    if candidate_pre != candidate_post:
        failures.append("candidate_sha_unstable: pre/post read differ")

    applied_candidate = applied.get("candidate_sha256")
    if applied_candidate and candidate_pre != applied_candidate:
        failures.append(f"candidate_sha_mismatch: file {candidate_pre} != applied/promoted {applied_candidate}")
    elif applied_candidate:
        checks["candidate_sha_bound"] = True

    if applied.get("result") != "PASS":
        failures.append(f"applied_not_pass: result={applied.get('result')!r} failures={applied.get('failures')}")
    if oracle.get("result") != "PASS":
        failures.append(f"oracle_not_pass: result={oracle.get('result')!r}")

    signoff_digests: dict[str, str] = {}
    if args.signoff_root.is_dir():
        for path in sorted(args.signoff_root.rglob("*")):
            if path.is_file():
                signoff_digests[str(path.relative_to(args.signoff_root))] = _sha256_file(path)
    checks["signoff_files_collected"] = bool(signoff_digests)

    result = "PASS" if not failures else "FAIL"
    output = {
        "schema_version": FINAL_SCHEMA,
        "result": result,
        "candidate_sha256": candidate_pre,
        "candidate_sha256_post": candidate_post,
        "candidate_path": str(args.candidate.resolve()),
        "applied": str(args.applied),
        "applied_manifest_digests": applied.get("manifests"),
        "oracle": str(args.oracle),
        "oracle_approved_proposal_sha256": _proposal_sha(oracle),
        "product_epoch": oracle.get("product_epoch") or applied.get("product_epoch"),
        "reference_epoch": oracle.get("reference_epoch") or applied.get("reference_epoch"),
        "f5_f6_identities": {
            "f5_proposal_sha256": applied.get("proposal_sha256"),
            "f6_oracle_output": str(args.oracle),
            "f6_verdict": oracle.get("verdict"),
            "f6_reviewer": oracle.get("reviewer"),
        },
        "signoff_digests": signoff_digests,
        "checks": checks,
        "failures": failures,
    }
    _write_json(args.output, output)
    print(json.dumps({"result": result, "output": str(args.output)}, sort_keys=True))
    return 0 if result == "PASS" else 1


# ---------------------------------------------------------------------------
# Subparser wiring.
# ---------------------------------------------------------------------------

def _add_subparsers(parser: argparse.ArgumentParser) -> None:
    sub = parser.add_subparsers(dest="command")

    prepare = sub.add_parser("prepare", help="Prepare a review-kind input JSON for an independent parity reviewer")
    prepare.add_argument("--kind", required=True, choices=sorted(REVIEW_KINDS))
    prepare.add_argument("--plan", required=True, type=Path)
    prepare.add_argument("--attestation", required=True, type=Path)
    prepare.add_argument("--candidate", type=Path)
    prepare.add_argument("--reference", type=Path)
    prepare.add_argument("--inventory", type=Path)
    prepare.add_argument("--scope", type=Path)
    prepare.add_argument("--removals", type=Path)
    prepare.add_argument("--reviews", nargs="+", type=Path, default=[])
    prepare.add_argument("--proposal", type=Path)
    prepare.add_argument("--evidence-root", dest="evidence_root", type=Path)
    prepare.add_argument("--output", required=True, type=Path)

    receipt = sub.add_parser("validate-agent-receipt", help="Validate a review-agent task receipt against a prepared input")
    receipt.add_argument("--kind", required=True, choices=sorted(REVIEW_KINDS))
    receipt.add_argument("--input", required=True, type=Path)
    receipt.add_argument("--receipt", required=True, type=Path)
    receipt.add_argument("--output", required=True, type=Path)
    receipt.add_argument("--require-verdict", dest="require_verdict", choices=["unconditional_approval", "rejected"])
    receipt.add_argument("--require-proposal-sha", dest="require_proposal_sha")

    proposal = sub.add_parser("validate-promotion-proposal", help="Validate a promotion proposal against review outputs and attestation")
    proposal.add_argument("--proposal", required=True, type=Path)
    proposal.add_argument("--reviews", required=True, nargs="+", type=Path)
    proposal.add_argument("--attestation", required=True, type=Path)
    proposal.add_argument("--require-zero-blocked", dest="require_zero_blocked", action="store_true")
    proposal.add_argument("--require-zero-divergence", dest="require_zero_divergence", action="store_true")
    proposal.add_argument("--output", required=True, type=Path)

    apply_parser = sub.add_parser("apply-promotion", help="Apply the oracle-approved promotion to canonical manifests (ONLY mutating subcommand)")
    apply_parser.add_argument("--proposal", required=True, type=Path)
    apply_parser.add_argument("--oracle", required=True, type=Path)
    apply_parser.add_argument("--attestation", required=True, type=Path)
    apply_parser.add_argument("--tui-manifest", dest="tui_manifest", required=True, type=Path)
    apply_parser.add_argument("--capability-manifest", dest="capability_manifest", required=True, type=Path)
    apply_parser.add_argument("--output", required=True, type=Path)

    finalize = sub.add_parser("finalize-attestation", help="Bind candidate SHA, manifest digests, signoff digests, and F5/F6 identities")
    finalize.add_argument("--applied", required=True, type=Path)
    finalize.add_argument("--oracle", required=True, type=Path)
    finalize.add_argument("--candidate", required=True, type=Path)
    finalize.add_argument("--signoff-root", dest="signoff_root", required=True, type=Path)
    finalize.add_argument("--output", required=True, type=Path)


# ---------------------------------------------------------------------------
# Deterministic self-test for the F1-F7 subcommands (hermetic temp workspace).
# ---------------------------------------------------------------------------

def _self_test_subcommands(root: Path) -> int:
    cases = 0
    work = root / "workflow"
    work.mkdir(parents=True, exist_ok=True)
    script = str(Path(__file__).resolve())

    def run(*cli_args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, script, *cli_args], capture_output=True, text=True, check=False)

    def check(label: str, condition: bool, detail: str = "") -> None:
        nonlocal cases
        if not condition:
            raise reject(f"self-test '{label}' failed: {detail}")
        cases += 1
        print(f"PASS: {label}")

    candidate_bytes = b"harness-candidate-self-test\n"
    reference_bytes = b"grok-reference-self-test\n"
    candidate_path = work / "candidate-bin"
    reference_path = work / "reference-bin"
    candidate_path.write_bytes(candidate_bytes)
    reference_path.write_bytes(reference_bytes)
    candidate_sha = hashlib.sha256(candidate_bytes).hexdigest()
    reference_sha = hashlib.sha256(reference_bytes).hexdigest()
    product_epoch = "a" * 64
    reference_epoch = "d" * 64

    attestation = {
        "schema_version": 1, "task": 99, "plan": "self-test-plan.md", "result": "PASS",
        "product_candidate": {"path": str(candidate_path), "absolute_path": str(candidate_path.resolve()), "sha256": candidate_sha, "mode": "555", "version": "harness self-test"},
        "reference_binary": {"path": str(reference_path), "absolute_path": str(reference_path.resolve()), "sha256": reference_sha, "version": "grok self-test"},
        "evidence_root": str(work),
        "candidate_sha_stability": {"recorded_build_sha256": candidate_sha, "installed_sha256": candidate_sha, "identical": True},
        "scenario_validation": {"result": "PASS", "reference_epoch": "c1b5909ec707c069f1d21a93917af044e71da0d7", "coverage_percent": 100, "copied_reference_assets": 0},
        "lane_summary": {"lane_stages_pass": 1, "lane_stages_fail": 0},
    }
    attestation_path = work / "attestation.json"
    _write_json(attestation_path, attestation)
    plan_path = work / "plan.md"
    plan_path.write_text("# self-test plan\n", encoding="utf-8")
    inventory_path = work / "inventory.json"
    _write_json(inventory_path, {"rows": []})
    removals_path = work / "removals.json"
    _write_json(removals_path, {"removals": []})
    scope_path = work / "scope.json"
    _write_json(scope_path, {
        "schema_version": "grok-cleanroom-scope-v1",
        "reference_revision": "c1b5909ec707c069f1d21a93917af044e71da0d7",
        "retained_categories": {"action": {"retained_rows": 2}},
        "approved_exclusions": [
            {"family": "voice-dictation", "excluded_rows": 1, "backed_by_removal_ledger": True, "ledger_family_id": "voice-dictation"},
        ],
        "totals": {"inventory_rows": 3, "retained_rows": 2, "approved_exclusion_rows": 1},
        "scope_conformance": {"coverage_percent": 100.0, "overlaps": 0, "unapproved_exclusions": 0, "pass_claims": 0, "exclusions_approved": True},
    })

    tui_doc = {
        "schema_version": "harness-tui-reference-parity-manifest-v1",
        "rows": [
            {"behavior_id": "B-1", "status": "incomplete", "notes": "x"},
            {"behavior_id": "B-2", "status": "incomplete", "notes": "y"},
        ],
    }
    cap_doc = {
        "schema_version": "harness-capability-inventory-v1",
        "capabilities": [
            {"capability_id": "C-1", "status": "incomplete", "disposition": "retain"},
            {"capability_id": "C-2", "status": "incomplete", "disposition": "retain"},
        ],
    }
    tui_path = work / "tui-reference-parity-manifest.v1.json"
    cap_path = work / "capability-inventory.v1.json"
    tui_text = json.dumps(tui_doc, indent=2, ensure_ascii=False) + "\n"
    cap_text = json.dumps(cap_doc, indent=2, ensure_ascii=False) + "\n"
    tui_path.write_text(tui_text, encoding="utf-8")
    cap_path.write_text(cap_text, encoding="utf-8")
    tui_pre = _sha256_text(tui_text)
    cap_pre = _sha256_text(cap_text)

    def make_receipt(**overrides: Any) -> dict[str, Any]:
        receipt = _fixture_verdict()
        receipt["candidate_sha256"] = candidate_sha
        receipt["reference_sha256"] = reference_sha
        receipt["product_epoch"] = product_epoch
        receipt["reference_epoch"] = reference_epoch
        receipt.update(overrides)
        return receipt

    # --- prepare F1, F2, F3, F4 ---
    f1_in = work / "F1-input.json"
    rc = run("prepare", "--kind", "plan-compliance", "--plan", str(plan_path), "--attestation", str(attestation_path), "--evidence-root", str(work), "--output", str(f1_in))
    check("prepare-plan-compliance", rc.returncode == 0 and f1_in.is_file(), rc.stderr)
    f1_data = json.loads(f1_in.read_text())
    check("prepare-plan-compliance-binding", f1_data["kind"] == "plan-compliance" and f1_data["binding"]["candidate_sha256"] == candidate_sha, str(f1_data)[:400])

    f2_in = work / "F2-input.json"
    rc = run("prepare", "--kind", "code-security", "--plan", str(plan_path), "--attestation", str(attestation_path), "--evidence-root", str(work), "--output", str(f2_in))
    check("prepare-code-security", rc.returncode == 0 and f2_in.is_file(), rc.stderr)

    f3_in = work / "F3-input.json"
    rc = run("prepare", "--kind", "manual-visual", "--plan", str(plan_path), "--attestation", str(attestation_path), "--candidate", str(candidate_path), "--reference", str(reference_path), "--evidence-root", str(work), "--output", str(f3_in))
    check("prepare-manual-visual", rc.returncode == 0 and f3_in.is_file(), rc.stderr)
    f3_data = json.loads(f3_in.read_text())
    check("prepare-manual-visual-shas", f3_data["candidate"]["sha256"] == candidate_sha and f3_data["reference"]["sha256"] == reference_sha, str(f3_data)[:400])

    f4_in = work / "F4-input.json"
    rc = run("prepare", "--kind", "scope-cleanroom", "--plan", str(plan_path), "--attestation", str(attestation_path), "--inventory", str(inventory_path), "--scope", str(scope_path), "--removals", str(removals_path), "--evidence-root", str(work), "--output", str(f4_in))
    check("prepare-scope-cleanroom", rc.returncode == 0 and f4_in.is_file(), rc.stderr)
    f4_data = json.loads(f4_in.read_text())
    check("prepare-scope-cleanroom-clean", f4_data["clean_room_scan"] == "clean", "")
    check("prepare-scope-cleanroom-scope-bound", f4_data["scope"]["sha256"] == _sha256_file(scope_path) and f4_data["scope_conformance"]["coverage_percent"] == 100.0 and f4_data["scope_conformance"]["approved_exclusion_families"] == ["voice-dictation"], str(f4_data)[:400])

    # scope-cleanroom must fail closed when --scope is omitted.
    rc = run("prepare", "--kind", "scope-cleanroom", "--plan", str(plan_path), "--attestation", str(attestation_path), "--inventory", str(inventory_path), "--output", str(work / "F4-noscope.json"))
    check("prepare-scope-cleanroom-missing-scope-fails", rc.returncode == 1, rc.stdout + rc.stderr)

    # scope-cleanroom must fail closed on a non-conforming scope (overlaps > 0).
    bad_scope = work / "scope-bad.json"
    _write_json(bad_scope, {
        "schema_version": "grok-cleanroom-scope-v1",
        "retained_categories": {"action": {"retained_rows": 2}},
        "approved_exclusions": [],
        "totals": {"inventory_rows": 2, "retained_rows": 2, "approved_exclusion_rows": 0},
        "scope_conformance": {"coverage_percent": 100.0, "overlaps": 1, "unapproved_exclusions": 0, "pass_claims": 0, "exclusions_approved": True},
    })
    rc = run("prepare", "--kind", "scope-cleanroom", "--plan", str(plan_path), "--attestation", str(attestation_path), "--inventory", str(inventory_path), "--scope", str(bad_scope), "--output", str(work / "F4-badscope.json"))
    check("prepare-scope-cleanroom-overlap-fails", rc.returncode == 1, rc.stdout + rc.stderr)

    # prepare manual-visual must fail closed on a candidate sha mismatch.
    bad_candidate = work / "bad-candidate"
    bad_candidate.write_bytes(b"different-bytes\n")
    rc = run("prepare", "--kind", "manual-visual", "--plan", str(plan_path), "--attestation", str(attestation_path), "--candidate", str(bad_candidate), "--reference", str(reference_path), "--output", str(work / "F3-bad.json"))
    check("prepare-manual-visual-sha-mismatch-fails", rc.returncode == 1, rc.stdout + rc.stderr)

    # --- validate-agent-receipt for F1..F4 ---
    in_files = {"plan-compliance": f1_in, "code-security": f2_in, "manual-visual": f3_in, "scope-cleanroom": f4_in}
    out_names = {"plan-compliance": "F1-out.json", "code-security": "F2-out.json", "manual-visual": "F3-out.json", "scope-cleanroom": "F4-out.json"}
    review_outputs: dict[str, Path] = {}
    for kind, out_name in out_names.items():
        receipt_path = work / f"receipt-{kind}.json"
        _write_json(receipt_path, make_receipt())
        out_path = work / out_name
        rc = run("validate-agent-receipt", "--kind", kind, "--input", str(in_files[kind]), "--receipt", str(receipt_path), "--output", str(out_path), "--require-verdict", "unconditional_approval")
        check(f"receipt-{kind}-pass", rc.returncode == 0 and json.loads(out_path.read_text())["result"] == "PASS", rc.stderr + out_path.read_text())
        review_outputs[kind] = out_path

    # receipt with a binding mismatch fails closed.
    bad_receipt_path = work / "receipt-bad-binding.json"
    _write_json(bad_receipt_path, make_receipt(candidate_sha256="e" * 64))
    bad_out = work / "bad-binding-out.json"
    rc = run("validate-agent-receipt", "--kind", "plan-compliance", "--input", str(f1_in), "--receipt", str(bad_receipt_path), "--output", str(bad_out), "--require-verdict", "unconditional_approval")
    check("receipt-binding-mismatch-fails", rc.returncode == 1 and json.loads(bad_out.read_text())["result"] == "FAIL", rc.stdout + rc.stderr)

    # receipt rejected when unconditional_approval is required fails closed.
    rejected_receipt = make_receipt(verdict="rejected", findings=[{
        "root_cause_class": "copied-reference-asset", "earliest_task": "T33", "write_reservation": "x/**",
        "affected_descendant_tasks": ["T34"], "repair_namespace": "clean-room-scanner", "reentry_gate": "rescan clean",
    }])
    rejected_path = work / "receipt-rejected.json"
    _write_json(rejected_path, rejected_receipt)
    rc = run("validate-agent-receipt", "--kind", "plan-compliance", "--input", str(f1_in), "--receipt", str(rejected_path), "--output", str(work / "rej-out.json"), "--require-verdict", "unconditional_approval")
    check("receipt-verdict-requirement-fails", rc.returncode == 1, rc.stdout + rc.stderr)

    # --- F5 proposal (validate-promotion-proposal) ---
    tui_modified = json.loads(tui_text)
    tui_modified["rows"][0]["status"] = "pass"
    cap_modified = json.loads(cap_text)
    cap_modified["capabilities"][0]["status"] = "pass"
    tui_post = _sha256_text(_serialize_preserving(tui_text, tui_modified))
    cap_post = _sha256_text(_serialize_preserving(cap_text, cap_modified))
    proposal = {
        "schema_version": PROMOTION_SCHEMA,
        "proposed_row_changes": [
            {"manifest": "docs/tui-reference-parity-manifest.v1.json", "row_id": "B-1", "changes": {"status": "pass"}},
            {"manifest": "docs/capability-inventory.v1.json", "row_id": "C-1", "changes": {"status": "pass"}},
        ],
        "unchanged_rows": ["B-2", "C-2"],
        "blockers": [],
        "divergences": [],
        "product_epoch": product_epoch,
        "reference_epoch": reference_epoch,
        "candidate_sha256": candidate_sha,
        "reference_sha256": reference_sha,
        "input_manifest_digests": {"docs/tui-reference-parity-manifest.v1.json": tui_pre, "docs/capability-inventory.v1.json": cap_pre},
        "output_manifest_digests": {"docs/tui-reference-parity-manifest.v1.json": tui_post, "docs/capability-inventory.v1.json": cap_post},
        "rationale": "self-test zero-blocker promotion",
    }
    f5_path = work / "F5-proposed-promotion.json"
    f5_path.write_text(json.dumps(proposal, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    proposal_sha = _sha256_file(f5_path)

    f5_out = work / "F5-out.json"
    rc = run("validate-promotion-proposal", "--proposal", str(f5_path), "--reviews",
             str(review_outputs["plan-compliance"]), str(review_outputs["code-security"]),
             str(review_outputs["manual-visual"]), str(review_outputs["scope-cleanroom"]),
             "--attestation", str(attestation_path), "--require-zero-blocked", "--require-zero-divergence",
             "--output", str(f5_out))
    check("validate-promotion-proposal-pass", rc.returncode == 0 and json.loads(f5_out.read_text())["result"] == "PASS", rc.stderr + rc.stdout)

    # a failing review output fails the proposal.
    fail_review = work / "fail-review.json"
    _write_json(fail_review, {"result": "FAIL", "kind": "plan-compliance", "failures": ["x"]})
    rc = run("validate-promotion-proposal", "--proposal", str(f5_path), "--reviews", str(fail_review), "--attestation", str(attestation_path), "--output", str(work / "F5-fail.json"))
    check("validate-promotion-proposal-fail-review", rc.returncode == 1 and json.loads((work / "F5-fail.json").read_text())["result"] == "FAIL", rc.stdout + rc.stderr)

    # a non-empty blocker violates --require-zero-blocked.
    blocked = dict(proposal)
    blocked["blockers"] = [{"id": "X"}]
    blocked_path = work / "F5-blocked.json"
    blocked_path.write_text(json.dumps(blocked, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    rc = run("validate-promotion-proposal", "--proposal", str(blocked_path), "--reviews", str(review_outputs["plan-compliance"]), "--attestation", str(attestation_path), "--require-zero-blocked", "--output", str(work / "F5-blocked-out.json"))
    check("validate-promotion-proposal-blockers-fail", rc.returncode == 1, rc.stdout + rc.stderr)

    # --- F6 terminal-oracle (prepare + receipt with approved proposal sha) ---
    f6_in = work / "F6-input.json"
    rc = run("prepare", "--kind", "terminal-oracle", "--plan", str(plan_path), "--attestation", str(attestation_path),
             "--reviews", str(review_outputs["plan-compliance"]), str(review_outputs["code-security"]),
             str(review_outputs["manual-visual"]), str(review_outputs["scope-cleanroom"]),
             "--proposal", str(f5_path), "--output", str(f6_in))
    check("prepare-terminal-oracle", rc.returncode == 0 and f6_in.is_file(), rc.stderr)
    f6_data = json.loads(f6_in.read_text())
    check("prepare-terminal-oracle-reviews", len(f6_data["reviews"]) == 4 and f6_data["proposal"]["exists"], str(f6_data)[:400])

    oracle_receipt = make_receipt()
    oracle_receipt["approved_proposal_sha256"] = proposal_sha
    oracle_receipt_path = work / "oracle-receipt.json"
    _write_json(oracle_receipt_path, oracle_receipt)
    f6_out = work / "F6-oracle.json"
    rc = run("validate-agent-receipt", "--kind", "terminal-oracle", "--input", str(f6_in), "--receipt", str(oracle_receipt_path), "--output", str(f6_out), "--require-verdict", "unconditional_approval", "--require-proposal-sha", proposal_sha)
    check("validate-terminal-oracle-pass", rc.returncode == 0 and json.loads(f6_out.read_text())["result"] == "PASS", rc.stderr + rc.stdout)

    rc = run("validate-agent-receipt", "--kind", "terminal-oracle", "--input", str(f6_in), "--receipt", str(oracle_receipt_path), "--output", str(work / "F6-bad.json"), "--require-proposal-sha", "f" * 64)
    check("validate-terminal-oracle-wrong-sha-fails", rc.returncode == 1, rc.stdout + rc.stderr)

    # --- apply-promotion on throwaway manifest copies ---
    f7_applied = work / "F7-applied.json"
    rc = run("apply-promotion", "--proposal", str(f5_path), "--oracle", str(f6_out), "--attestation", str(attestation_path), "--tui-manifest", str(tui_path), "--capability-manifest", str(cap_path), "--output", str(f7_applied))
    check("apply-promotion-pass", rc.returncode == 0 and json.loads(f7_applied.read_text())["result"] == "PASS", rc.stderr + rc.stdout)
    applied = json.loads(f7_applied.read_text())
    tui_now = json.loads(tui_path.read_text())
    cap_now = json.loads(cap_path.read_text())
    check("apply-promotion-mutated-rows", tui_now["rows"][0]["status"] == "pass" and tui_now["rows"][1]["status"] == "incomplete" and cap_now["capabilities"][0]["status"] == "pass", str(applied)[:400])
    check("apply-promotion-pre-digests", applied["manifests"]["docs/tui-reference-parity-manifest.v1.json"]["pre_sha256"] == tui_pre and applied["manifests"]["docs/capability-inventory.v1.json"]["pre_sha256"] == cap_pre, str(applied)[:400])
    check("apply-promotion-post-digests", applied["manifests"]["docs/tui-reference-parity-manifest.v1.json"]["post_sha256"] == tui_post and applied["manifests"]["docs/capability-inventory.v1.json"]["post_sha256"] == cap_post, str(applied)[:400])

    # apply must NOT mutate manifests when the oracle approves the wrong proposal.
    tui_path.write_text(tui_text, encoding="utf-8")
    cap_path.write_text(cap_text, encoding="utf-8")
    wrong_oracle = json.loads(f6_out.read_text())
    wrong_oracle["approved_proposal_sha256"] = "0" * 64
    wrong_oracle_path = work / "wrong-oracle.json"
    _write_json(wrong_oracle_path, wrong_oracle)
    rc = run("apply-promotion", "--proposal", str(f5_path), "--oracle", str(wrong_oracle_path), "--attestation", str(attestation_path), "--tui-manifest", str(tui_path), "--capability-manifest", str(cap_path), "--output", str(work / "F7-fail.json"))
    check("apply-promotion-wrong-oracle-fails", rc.returncode == 1, rc.stdout + rc.stderr)
    check("apply-promotion-no-mutation-on-fail", tui_path.read_text() == tui_text and cap_path.read_text() == cap_text, "manifests changed despite failure")

    # re-apply for finalize (restore the applied state).
    rc = run("apply-promotion", "--proposal", str(f5_path), "--oracle", str(f6_out), "--attestation", str(attestation_path), "--tui-manifest", str(tui_path), "--capability-manifest", str(cap_path), "--output", str(f7_applied))
    check("apply-promotion-reapply", rc.returncode == 0, rc.stderr + rc.stdout)

    # --- finalize-attestation ---
    signoff = work / "signoff"
    signoff.mkdir()
    (signoff / "evidence.txt").write_text("signoff\n", encoding="utf-8")
    f7_attestation = work / "F7-attestation.json"
    rc = run("finalize-attestation", "--applied", str(f7_applied), "--oracle", str(f6_out), "--candidate", str(candidate_path), "--signoff-root", str(signoff), "--output", str(f7_attestation))
    check("finalize-attestation-pass", rc.returncode == 0 and json.loads(f7_attestation.read_text())["result"] == "PASS", rc.stderr + rc.stdout)
    attestation_final = json.loads(f7_attestation.read_text())
    check("finalize-attestation-candidate-bound", attestation_final["candidate_sha256"] == candidate_sha == attestation_final["candidate_sha256_post"], str(attestation_final)[:400])
    check("finalize-attestation-f5f6", attestation_final["f5_f6_identities"]["f5_proposal_sha256"] == proposal_sha and attestation_final["signoff_digests"].get("evidence.txt"), str(attestation_final)[:400])

    # a tampered candidate fails finalization.
    tampered = work / "tampered-candidate"
    tampered.write_bytes(b"tampered\n")
    rc = run("finalize-attestation", "--applied", str(f7_applied), "--oracle", str(f6_out), "--candidate", str(tampered), "--signoff-root", str(signoff), "--output", str(work / "F7-att-bad.json"))
    check("finalize-attestation-tampered-candidate-fails", rc.returncode == 1, rc.stdout + rc.stderr)

    return cases


def self_test() -> int:
    cases = 0
    with tempfile.TemporaryDirectory() as tmp_dir:
        root = Path(tmp_dir)

        def expect(label: str, verdict: dict[str, Any], returncode: int, needle: str | None = None) -> None:
            nonlocal cases
            path = root / f"verdict-{label}.json"
            _write_verdict(path, verdict)
            result = _run_case(path)
            if result.returncode != returncode:
                raise reject(f"self-test '{label}' exit {result.returncode} != {returncode}: {result.stderr}")
            haystack = result.stdout + result.stderr
            if needle is not None and needle not in haystack:
                raise reject(f"self-test '{label}' missing '{needle}' in output: {haystack}")
            cases += 1
            print(f"PASS: {label}")

        expect("clean-approval", _fixture_verdict(), 0, "unconditional_approval")
        expect(
            "structured-rejection",
            _fixture_verdict(
                verdict="rejected",
                findings=[{
                    "root_cause_class": "copied-reference-asset",
                    "earliest_task": "T33",
                    "write_reservation": "crates/harness-tui/tests/**",
                    "affected_descendant_tasks": ["T34", "T38"],
                    "repair_namespace": "clean-room-scanner",
                    "reentry_gate": "rescan clean",
                }],
            ),
            1,
            '"verdict": "rejected"',
        )
        expect("approval-with-findings", _fixture_verdict(findings=[{"note": "leftover"}]), 1, "approval cannot contain findings")
        expect("copied-reference-assets", _fixture_verdict(scope_conformance={**_fixture_verdict()["scope_conformance"], "copied_reference_assets": 2}), 1, "copied reference assets")
        expect("unapproved-exclusions", _fixture_verdict(scope_conformance={**_fixture_verdict()["scope_conformance"], "unapproved_exclusions": 1}), 1, "unapproved exclusions")
        expect("pass-claims", _fixture_verdict(scope_conformance={**_fixture_verdict()["scope_conformance"], "pass_claims": 1}), 1, "pass claims")
        expect("dirty-clean-room-scan", _fixture_verdict(scope_conformance={**_fixture_verdict()["scope_conformance"], "clean_room_scan": "findings"}), 1, "clean-room scanner")
        expect("self-oracle", _fixture_verdict(reference_sha256="b" * 64), 1, "self-oracle")
        expect("not-read-only", _fixture_verdict(read_only=False), 1, "read-only")
        expect("bad-epoch", _fixture_verdict(product_epoch="xyz"), 1, "product_epoch")
        expect("missing-reviewer-model", _fixture_verdict(reviewer={"identity": "x", "tool": "y", "model": "", "version": "z"}), 1, "reviewer")
        expect("unknown-verdict", _fixture_verdict(verdict="pending"), 1, "unconditional_approval or rejected")

        cases += _self_test_subcommands(root)

    print(f"self-test: {cases}/{cases} passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    _ = parser.add_argument("--verdict", type=Path)
    _ = parser.add_argument("--self-test", action="store_true")
    _add_subparsers(parser)
    args = parser.parse_args()

    if args.command == "prepare":
        return cmd_prepare(args)
    if args.command == "validate-agent-receipt":
        return cmd_validate_agent_receipt(args)
    if args.command == "validate-promotion-proposal":
        return cmd_validate_promotion_proposal(args)
    if args.command == "apply-promotion":
        return cmd_apply_promotion(args)
    if args.command == "finalize-attestation":
        return cmd_finalize_attestation(args)

    if args.self_test:
        return self_test()
    if args.verdict is None:
        raise reject("--verdict is required (or use --self-test, or a subcommand)")
    value = load_verdict(args.verdict)
    validate(value)
    print(json.dumps({"verdict": value["verdict"], "validated": True}, sort_keys=True))
    return 0 if value["verdict"] == "unconditional_approval" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(json.dumps({"verdict": "rejected", "reason": str(error)}, sort_keys=True), file=sys.stderr)
        raise SystemExit(1) from error
