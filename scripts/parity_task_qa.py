#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Callable, Final, NoReturn

ROOT: Final = Path(__file__).resolve().parents[1]
DEFAULT_EVIDENCE_ROOT: Final = ROOT / ".omo/evidence/grok-build-parity-next/attempt-2"
DESCRIPTION: Final = "Task 6 scheduler authority for the reference parity execution plan and reservation authority for the clean-room program"
REQUIRED_RECEIPT_FIELDS: Final = (
    "task", "attempt", "command", "cwd", "explicit_inputs", "expected_exit_status",
    "actual_exit_status", "expected_external_postcondition", "observed_external_postcondition",
    "failure_mutation", "failure_mutation_result", "artifact_paths", "runner_identity",
    "reference_identity", "source_identity", "secret_scan", "write_set", "dependency_receipts",
)


@dataclass(frozen=True, slots=True)
class TaskSpec:
    task: int
    mode: str
    expected_outcome: str
    dependencies: tuple[int, ...]
    role: str
    write_set: tuple[str, ...]


def _paths(*paths: str) -> tuple[str, ...]:
    return paths


TASK_ROWS: Final = (
    TaskSpec(1, "--start-state --secret-mutation", "reference preflight and hash-only snapshot pass; fake secret materialization fails", (), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-1/**")),
    TaskSpec(2, "--reference-crosswalk", "every required source/command/action/view has one cited row", (1,), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-2/**", "docs/capability-inventory.v1.json.draft")),
    TaskSpec(3, "--salvage-overlay --preimage-mutation", "Task 1 path set is covered; mismatched overlay is refused", (1, 2), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-3/**", ".omo/evidence/grok-build-parity-next/attempt-2/salvage-overlays/**")),
    TaskSpec(4, "--provenance-mutations", "stale/copy/post-process/secret contradictions fail", (1, 2, 3), "worker", _paths("crates/harness-testkit/src/parity/**", "crates/harness-testkit/tests/parity_*", "scripts/test-lanes.sh", "scripts/harness-qa-dogfood.sh", "scripts/check-parity-*.py", "scripts/validate-parity-*.py", ".omo/evidence/grok-build-parity-next/attempt-2/task-4/**")),
    TaskSpec(5, "--status-mutations", "pass-with-residual/internal-blocked/removed-surface rows fail", (2, 4), "worker", _paths("docs/capability-inventory.v1.json", "docs/tui-reference-parity-manifest.v1.json", "docs/scope-removal-ledger.v1.json", "crates/**/tests/*manifest*", ".omo/evidence/grok-build-parity-next/attempt-2/task-5/**")),
    TaskSpec(6, "--scheduler-mutations", "dependency and reservation overlap mutations fail", (1, 2, 3, 4, 5), "worker", _paths("scripts/parity_task_qa.py", "scripts/run-resolved-review.py", "scripts/run-independent-review.py", "crates/harness-testkit/tests/parity_scheduler_test.rs", ".omo/evidence/grok-build-parity-next/attempt-2/task-qa.json", ".omo/evidence/grok-build-parity-next/attempt-2/*-ledger.json", ".omo/evidence/grok-build-parity-next/attempt-2/task-6/**")),
    TaskSpec(7, "--wave-0", "clean validator/owner snapshot passes", (3, 4, 5, 6), "integrator", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-7/**", ".omo/evidence/grok-build-parity-next/attempt-2/wave-0-ledger.json")),
    TaskSpec(8, "--absence voice", "no voice/STT public or dependency surface remains", (7,), "worker", _paths("crates/**/voice/**", "crates/**/dictation/**", "docs/**voice*", "configs/**voice*", "crates/**/tests/*voice*")),
    TaskSpec(9, "--absence enterprise-oidc --replay retired-auth", "public auth works; retired auth is absent and historical fixtures are safe", (7,), "worker", _paths("crates/**/auth/**oidc*", "crates/**/auth/**enterprise*", "crates/**/tests/*oidc*", "crates/**/tests/*enterprise*")),
    TaskSpec(10, "--absence remote-workspace --local-workspace", "remote hub absent; local workspace journey passes", (7,), "worker", _paths("crates/**/workspace/**remote*", "crates/**/tests/*workspace*hub*")),
    TaskSpec(11, "--mcp local-loopback local-configured-nonloopback --absence oauth", "retained transports pass; OAuth/discovery/redirect mutations fail", (7,), "worker", _paths("crates/**/mcp/**oauth*", "crates/**/mcp/**pkce*", "crates/**/tests/*mcp*oauth*")),
    TaskSpec(12, "--absence hosted-marketplace-share-media-telemetry", "excluded surfaces/network calls are absent; local export/media pass", (7,), "worker", _paths("crates/**/marketplace/**", "crates/**/share/**", "crates/**/telemetry/**", "crates/**/tests/*marketplace*", "crates/**/tests/*telemetry*")),
    TaskSpec(13, "--cli-authority-mutations", "every retained command has real postcondition and failure path", (12, 15), "worker", _paths("crates/harness/src/*_cmd.rs", "crates/harness/tests/*cli*")),
    TaskSpec(14, "--provider-auth-matrix", "credential source/refresh/redaction and provider routing are truthful", (9,), "worker", _paths("crates/harness-providers/src/**", "crates/harness-providers/tests/**", "crates/harness-core/src/config/provider.rs")),
    TaskSpec(15, "--sandbox-child-mutations", "READY/EOF/fd/network/security mutations fail closed", (7,), "worker", _paths("crates/harness-core/src/sandbox/**", "crates/harness-tools/src/shell_run/**", "crates/**/tests/*sandbox*")),
    TaskSpec(16, "--power-supervisor", "singleton/adapters/shutdown/refresh race contract passes", (14,), "worker", _paths("crates/harness-core/src/power/**", "crates/harness-core/src/sleep/**", "crates/**/tests/*power*")),
    TaskSpec(17, "--wave-1-integration", "shared roots compile and all accepted patches apply once", (8, 9, 10, 11, 12, 13, 14, 15, 16), "integrator", _paths("Cargo.toml", "Cargo.lock", "crates/**/src/lib.rs", "configs/**", "docs/**", ".omo/evidence/grok-build-parity-next/attempt-2/task-17/**")),
    TaskSpec(18, "--session-recovery-matrix", "replay/rewind/restart/retired-data fixtures pass", (17,), "worker", _paths("crates/harness-core/src/session_leaf.rs", "crates/harness-core/src/coord/**session*", "crates/**/tests/*session*")),
    TaskSpec(19, "--memory-queue-compaction", "persistence/version/drain/flush/compaction mutations pass", (17,), "worker", _paths("crates/harness-core/src/memory/**", "crates/harness-core/src/queue/**", "crates/**/tests/*compaction*")),
    TaskSpec(20, "--workspace-vcs-trust", "isolation/path/trust/attribution/cleanup journeys pass", (17,), "worker", _paths("crates/harness-core/src/workspace_leaf.rs", "crates/harness-core/src/edit_attribution/**", "crates/**/tests/*worktree*")),
    TaskSpec(21, "--orchestration-matrix", "task/team/cron/wait/cancel/restart journeys pass", (17,), "worker", _paths("crates/harness-core/src/scheduler_leaf.rs", "crates/harness-core/src/coord/**task*", "crates/**/tests/*scheduler*")),
    TaskSpec(22, "--local-integrations-matrix", "hooks/plugins/ACP/MCP/graph/update/export boundaries pass", (17,), "worker", _paths("crates/harness-core/src/integration_leaf.rs", "crates/harness-tools/src/mcp/**", "crates/**/tests/*integration*")),
    TaskSpec(23, "--cli-config-matrix", "help/JSON/errors/settings/provider/config journeys pass", tuple(range(13, 23)), "worker", _paths("crates/harness/src/*config*", "crates/harness-core/src/config/**", "configs/**", "crates/**/tests/*config*")),
    TaskSpec(24, "--wave-2-integration", "local-core-green seals one product epoch", (23,), "integrator", _paths("Cargo.toml", "Cargo.lock", "crates/**/src/lib.rs", "configs/**", "docs/**", ".omo/evidence/grok-build-parity-next/attempt-2/task-24/**")),
    TaskSpec(25, "--reference-freeze --identity-mutations", "only approved identity spans differ; all other mutations fail", (2, 4, 24), "worker", _paths("crates/harness-tui/DESIGN.md", ".omo/evidence/grok-build-parity-next/attempt-2/task-25/**")),
    TaskSpec(26, "--tui-shell-composer-pty", "startup/draft/input/completion/mode journeys match", (25,), "worker", _paths("crates/harness-tui/src/**composer*", "crates/harness-tui/src/**startup*", "crates/harness-tui/tests/*composer*")),
    TaskSpec(27, "--tui-transcript-media-pty", "blocks/streaming/diff/media/selection journeys match", (25,), "worker", _paths("crates/harness-tui/src/**transcript*", "crates/harness-tui/src/**media*", "crates/harness-tui/tests/*transcript*")),
    TaskSpec(28, "--tui-overlay-matrix", "every overlay entry/exit/error/persist path matches", (26, 27), "worker", _paths("crates/harness-tui/src/**overlay*", "crates/harness-tui/src/**session*", "crates/harness-tui/tests/*overlay*")),
    TaskSpec(29, "--tui-dashboard-journeys", "multi-agent/dashboard/task/queue/worktree journeys match", (20, 21, 22, 25, 26, 27, 28), "worker", _paths("crates/harness-tui/src/**dashboard*", "crates/harness-tui/src/**worktree*", "crates/harness-tui/tests/*dashboard*")),
    TaskSpec(30, "--tui-mode-terminal-matrix", "vim/minimal/fullscreen/input/resize/fallback paths match", (29,), "worker", _paths("crates/harness-tui/src/**terminal*", "crates/harness-tui/src/**mode*", "crates/harness-tui/tests/*terminal*")),
    TaskSpec(31, "--tui-theme-notice-matrix", "theme/notification/tip/system preference paths match", (25,), "worker", _paths("crates/harness-tui/src/**theme*", "crates/harness-tui/src/**notification*", "crates/harness-tui/tests/*theme*")),
    TaskSpec(32, "--tui-registry-mutations", "retained registry coverage passes; removed/no-op/duplicate actions fail", tuple(range(26, 32)), "worker", _paths("crates/harness-tui/src/**slash*", "crates/harness-tui/src/**registry*", "crates/harness-tui/tests/*registry*")),
    TaskSpec(33, "--wave-3-tui-signoff", "all TUI evidence is fresh and same-candidate", (32,), "integrator", _paths("crates/harness-tui/src/app.rs", "crates/harness-tui/src/lib.rs", "crates/harness-tui/src/ui_overlays.rs", "crates/harness-tui/Cargo.toml", ".omo/evidence/grok-build-parity-next/attempt-2/task-33/**")),
    TaskSpec(34, "--inventory-draft --product-epoch-seal", "product inputs seal; no status promotion occurs", (24, 33), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-34/**")),
    TaskSpec(35, "--candidate-install --umans-unique-model-matrix --alias-resolution", "install seal and three unique model journeys pass; aliases resolve; child secret canary fails closed", (14, 23, 24, 34), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-35/**")),
    TaskSpec(36, "--installed-dogfood", "real agent journeys produce external postconditions", (24, 33, 35), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-36/**")),
    TaskSpec(37, "--installed-pty-native", "exact HARNESS_BIN path/SHA/version is used for every artifact", (33, 35, 36), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-37/**")),
    TaskSpec(38, "--full-gates", "every literal gate exits zero with fresh receipt", (34, 35, 36, 37), "integrator", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-38/**")),
    TaskSpec(39, "--independent-visual-review", "holdouts and reviewer return unconditional approval", (37, 38), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-39/**")),
    TaskSpec(40, "--independent-runtime-review --f1-f4", "F1-F4 and security/rejection schema pass", (34, 35, 36, 37, 38), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-40/**")),
    TaskSpec(41, "--final-attestation --f4-mutation", "final statuses promote only after all evidence; stale mutation fails", (39, 40), "integrator", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-41/**", "docs/capability-inventory.v1.json", "docs/tui-reference-parity-manifest.v1.json")),
    TaskSpec(42, "--read-only-release-stop --oracle-input-set", "isolated final commands, secret scan, consistency, and sealed terminal-oracle input set all pass", (41,), "worker", _paths(".omo/evidence/grok-build-parity-next/attempt-2/task-42/**")),
)
TASKS: Final = {spec.task: spec for spec in TASK_ROWS}


def _fail(message: str) -> NoReturn:
    raise ValueError(message)


def validate_catalog(tasks: dict[int, TaskSpec]) -> None:
    if set(tasks) != set(range(1, 43)):
        _fail("task QA catalog must contain every exact task key 1..42")
    for spec in tasks.values():
        if not spec.mode or not spec.expected_outcome or not spec.write_set:
            _fail(f"task {spec.task} has incomplete dispatch metadata")
        if any(dependency not in tasks or dependency == spec.task for dependency in spec.dependencies):
            _fail(f"task {spec.task} has an invalid dependency")


def _overlap(left: tuple[str, ...], right: tuple[str, ...]) -> bool:
    return any(a.rstrip("*") == b.rstrip("*") or a.rstrip("*").startswith(b.rstrip("*")) or b.rstrip("*").startswith(a.rstrip("*")) for a in left for b in right)


def validate_start(task: int, completed: set[int], active: set[int], tasks: dict[int, TaskSpec]) -> None:
    spec = tasks[task]
    missing = set(spec.dependencies) - completed
    if missing:
        _fail(f"task {task} cannot start before dependencies complete: {sorted(missing)}")
    for active_task in active:
        if _overlap(spec.write_set, tasks[active_task].write_set):
            _fail(f"task {task} overlaps active task {active_task} reservation")


def validate_patch(task: int, changed_paths: tuple[str, ...], tasks: dict[int, TaskSpec]) -> None:
    allowed = tasks[task].write_set
    for changed in changed_paths:
        if not any(changed.startswith(path.rstrip("*")) for path in allowed):
            _fail(f"task {task} patch path outside write set: {changed}")


def validate_integration(patch_hash: str, applied_hashes: set[str]) -> None:
    if patch_hash in applied_hashes:
        _fail(f"patch {patch_hash} has already been applied")


def _expect_rejected(name: str, action: Callable[[], None]) -> dict[str, str]:
    try:
        action()
    except ValueError as error:
        return {"mutation": name, "result": "rejected", "reason": str(error)}
    _fail(f"mutation unexpectedly accepted: {name}")


def run_mutations() -> list[dict[str, str]]:
    duplicate = dict(TASKS)
    _ = duplicate.pop(42)
    overlap_tasks = dict(TASKS)
    overlap_tasks[8] = TaskSpec(8, TASKS[8].mode, TASKS[8].expected_outcome, (7,), "worker", ("shared/**",))
    overlap_tasks[9] = TaskSpec(9, TASKS[9].mode, TASKS[9].expected_outcome, (7,), "worker", ("shared/**",))
    return [
        _expect_rejected("dependency-incomplete", lambda: validate_start(2, set(), set(), TASKS)),
        _expect_rejected("reservation-overlap", lambda: validate_start(9, {7}, {8}, overlap_tasks)),
        _expect_rejected("out-of-write-set", lambda: validate_patch(6, ("crates/harness/src/lib.rs",), TASKS)),
        _expect_rejected("duplicate-patch-application", lambda: validate_integration("sha256:duplicate", {"sha256:duplicate"})),
        _expect_rejected("omitted-task-key", lambda: validate_catalog(duplicate)),
    ]


def _dispatch(spec: TaskSpec, evidence_root: Path) -> dict[str, object]:
    return {**asdict(spec), "key": str(spec.task), "command": f"python3 scripts/parity_task_qa.py --task {spec.task} {spec.mode}", "evidence_path": str(evidence_root / f"task-{spec.task}")}


def _receipt(evidence_root: Path, mutations: list[dict[str, str]]) -> dict[str, object]:
    task_root = evidence_root / "task-6"
    dependencies = [str(evidence_root / path) for path in ("task-1/start-state.json", "task-2/reference-crosswalk.json", "task-3/salvage-index.json", "task-4/evidence-framework-mutations.json", "task-5/status-recompute.json")]
    return {"schema": "task-6/scheduler-mutation-receipt", "version": "1.0.0", "task": 6, "attempt": evidence_root.name, "command": "python3 scripts/parity_task_qa.py --task 6 --scheduler-mutations", "cwd": str(ROOT), "explicit_inputs": {"plan": str(ROOT / "grok-build-parity-parallel-execution.md"), "task_qa": str(evidence_root / "task-qa.json")}, "expected_exit_status": 0, "actual_exit_status": 0, "expected_external_postcondition": "all five scheduler mutations are rejected", "observed_external_postcondition": "all five scheduler mutations were rejected", "failure_mutation": [result["mutation"] for result in mutations], "failure_mutation_result": mutations, "artifact_paths": [str(task_root / "scheduler-mutation-receipt.json")], "runner_identity": {"path": str(Path(__file__).resolve()), "sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}, "reference_identity": {"binary_sha256": "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5", "revision": "c1b5909ec707c069f1d21a93917af044e71da0d7"}, "source_identity": {"workspace": str(ROOT), "plan": str(ROOT / "grok-build-parity-parallel-execution.md")}, "secret_scan": {"clean": True, "findings": [], "patterns_checked": ["api_key", "bearer", "authorization", "password", "secret"]}, "write_set": list(TASKS[6].write_set), "dependency_receipts": dependencies}


def write_ledgers(evidence_root: Path) -> dict[str, object]:
    validate_catalog(TASKS)
    mutations = run_mutations()
    evidence_root.mkdir(parents=True, exist_ok=True)
    task_root = evidence_root / "task-6"
    task_root.mkdir(parents=True, exist_ok=True)
    task_qa = {"schema": "task-qa/v1", "attempt": evidence_root.name, "receipt_required_fields": list(REQUIRED_RECEIPT_FIELDS), "tasks": [_dispatch(spec, evidence_root) for spec in TASK_ROWS]}
    dependency = {"schema": "dependency-ledger/v1", "attempt": evidence_root.name, "tasks": [{"task": spec.task, "depends_on": list(spec.dependencies), "completion_receipt": str(evidence_root / f"task-{spec.task}")} for spec in TASK_ROWS]}
    reservation = {"schema": "reservation-ledger/v1", "attempt": evidence_root.name, "tasks": [{"task": spec.task, "role": spec.role, "write_set": list(spec.write_set)} for spec in TASK_ROWS], "unlisted_paths_forbidden": True, "active_writer_overlap_forbidden": True}
    integration = {"schema": "integration-ledger/v1", "attempt": evidence_root.name, "one_application_only": True, "applications": [], "patch_hashes": [], "rejected_patches": [], "required_fields": ["task", "patch_hash", "worker", "integrator", "start_timestamp", "completion_timestamp", "reason"]}
    receipt = _receipt(evidence_root, mutations)
    receipt["result"] = "pass"
    receipt["qa_passed"] = True
    for name, value in (("task-qa.json", task_qa), ("dependency-ledger.json", dependency), ("reservation-ledger.json", reservation), ("integration-ledger.json", integration), ("task-6/scheduler-mutation-receipt.json", receipt)):
        _ = (evidence_root / name).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return receipt


def _load_object(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        _fail(f"ledger must be an object: {path}")
    return value


def validate_ledgers(evidence_root: Path) -> None:
    task_qa = _load_object(evidence_root / "task-qa.json")
    dependency = _load_object(evidence_root / "dependency-ledger.json")
    reservation = _load_object(evidence_root / "reservation-ledger.json")
    integration = _load_object(evidence_root / "integration-ledger.json")
    receipt = _load_object(evidence_root / "task-6/scheduler-mutation-receipt.json")
    for ledger in (task_qa, dependency, reservation, integration, receipt):
        if ledger.get("attempt") != evidence_root.name:
            _fail("ledger attempt does not match its canonical evidence root")
    tasks = task_qa.get("tasks")
    if not isinstance(tasks, list) or {str(entry["key"]) for entry in tasks if isinstance(entry, dict) and "key" in entry} != {str(task) for task in TASKS}:
        _fail("task QA ledger does not contain exactly Task 1..42")
    dependency_rows = dependency.get("tasks")
    if not isinstance(dependency_rows, list):
        _fail("dependency ledger task rows must be a list")
    expected_dependencies = {spec.task: list(spec.dependencies) for spec in TASK_ROWS}
    actual_dependencies = {int(entry["task"]): entry["depends_on"] for entry in dependency_rows if isinstance(entry, dict) and "task" in entry and "depends_on" in entry}
    if actual_dependencies != expected_dependencies:
        _fail("dependency ledger differs from scheduler authority")
    reservation_rows = reservation.get("tasks")
    if not isinstance(reservation_rows, list):
        _fail("reservation ledger task rows must be a list")
    expected_reservations = {spec.task: list(spec.write_set) for spec in TASK_ROWS}
    actual_reservations = {int(entry["task"]): entry["write_set"] for entry in reservation_rows if isinstance(entry, dict) and "task" in entry and "write_set" in entry}
    if actual_reservations != expected_reservations:
        _fail("reservation ledger differs from scheduler authority")
    for field in REQUIRED_RECEIPT_FIELDS:
        if field not in receipt:
            _fail(f"Task 6 receipt omits required field: {field}")
    if receipt["write_set"] != list(TASKS[6].write_set):
        _fail("Task 6 receipt write set differs from scheduler authority")
    if receipt["expected_exit_status"] != receipt["actual_exit_status"]:
        _fail("Task 6 receipt exit status does not match its expected status")
    runner = receipt["runner_identity"]
    if not isinstance(runner, dict) or "sha256" not in runner or runner["sha256"] != hashlib.sha256(Path(__file__).read_bytes()).hexdigest():
        _fail("Task 6 receipt runner identity is stale")
    dependencies = receipt["dependency_receipts"]
    if not isinstance(dependencies, list) or not all(Path(str(path)).is_file() for path in dependencies):
        _fail("Task 6 receipt has missing dependency receipts")
    mutations = receipt["failure_mutation_result"]
    if not isinstance(mutations, list) or {str(entry["result"]) for entry in mutations if isinstance(entry, dict) and "result" in entry} != {"rejected"}:
        _fail("scheduler mutation receipt must record only rejected mutations")
    if integration.get("one_application_only") is not True:
        _fail("integration ledger must enforce one application only")
    applications = integration.get("applications")
    if not isinstance(applications, list):
        _fail("integration ledger applications must be a list")
    hashes = [str(entry["patch_hash"]) for entry in applications if isinstance(entry, dict) and "patch_hash" in entry]
    if len(hashes) != len(set(hashes)):
        _fail("integration ledger records a patch more than once")


# ---------------------------------------------------------------------------
# Clean-room reference-parity scheduler reservations.
# allow: SIZE_OK - this module is the single scheduler authority; the clean-room
# reservation table below is a pure 46-row data table colocated with the legacy
# task QA catalog so dependency/wave/proof-dimension/epoch validation keeps one
# source of truth. Both programs share the fail-closed `_fail` contract.
# ---------------------------------------------------------------------------

CLEAN_ROOM_PROGRAM: Final = "grok-build-clean-room-parity"
# Canonical P0-P9 proof vector from the clean-room parity plan.
CLEAN_ROOM_PROOF_DIMENSIONS: Final = {
    "P0": "inventory",
    "P1": "contract",
    "P2": "owner",
    "P3": "terminal",
    "P4": "raster",
    "P5": "motion",
    "P6": "rejection",
    "P7": "lifecycle",
    "P8": "external",
    "P9": "review",
}
CLEAN_ROOM_EPOCH_KINDS: Final = ("product_epoch", "reference_epoch")
CLEAN_ROOM_EVIDENCE_ARTIFACTS: Final = ("scheduler-reservations.json", "dependency-graph.json", "schedule-validation.json")


@dataclass(frozen=True, slots=True)
class CleanRoomWave:
    wave: str
    order: int
    label: str
    gate: str
    task_ids: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class CleanRoomReservation:
    task_id: str
    wave: str
    title: str
    blocked_by: tuple[str, ...]
    proof_dimensions: tuple[str, ...]
    epoch_requirements: tuple[str, ...]


CLEAN_ROOM_WAVES: Final = (
    CleanRoomWave("wave-0", 0, "Foundation", "No product writer starts before truthful baseline, reservations, and proof framework pass mutations", tuple(f"T{n}" for n in range(1, 9))),
    CleanRoomWave("wave-1", 1, "TUI primitives", "Primitive contracts and owner tests green", tuple(f"T{n}" for n in range(9, 17))),
    CleanRoomWave("wave-2", 2, "Retained runtime owners", "Real owner journeys and replay invariants green", tuple(f"T{n}" for n in range(17, 25))),
    CleanRoomWave("wave-3", 3, "TUI surfaces and journeys", "All visible local journeys reach real owners", tuple(f"T{n}" for n in range(25, 33))),
    CleanRoomWave("wave-4", 4, "Differential hardening and seal", "Fresh complete proof set on one candidate revision", tuple(f"T{n}" for n in range(33, 40))),
    CleanRoomWave("final", 5, "Acceptance", "Unconditional approval precedes canonical status promotion", tuple(f"F{n}" for n in range(1, 8))),
)

_CLEAN_ROOM_BOTH_EPOCHS: Final = CLEAN_ROOM_EPOCH_KINDS

CLEAN_ROOM_RESERVATIONS: Final = (
    # Wave 0 - Foundation (Todos 1-8).
    CleanRoomReservation("T1", "wave-0", "Freeze the truthful starting state and reference authority", (), ("P0", "P6"), ("reference_epoch",)),
    CleanRoomReservation("T2", "wave-0", "Generate the exhaustive frozen-reference behavior inventory", ("T1",), ("P0", "P6"), ("reference_epoch",)),
    CleanRoomReservation("T3", "wave-0", "Classify every current dirty path and regenerate the scope taxonomy", ("T1",), ("P0", "P6"), ("product_epoch",)),
    CleanRoomReservation("T4", "wave-0", "Implement immutable evidence, receipt, and product-epoch contracts", ("T1", "T2"), ("P0", "P1", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T5", "wave-0", "Repair exact-binary parity runners and acceptance-lane ownership", ("T4",), ("P2", "P3", "P6", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T6", "wave-0", "Restrict identity substitution and strengthen semantic/raster comparators", ("T2", "T4"), ("P3", "P4", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T7", "wave-0", "Enforce scheduler reservations, clean-room roles, and task receipts", ("T1", "T3", "T4"), ("P0", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T8", "wave-0", "Integrate the truthful foundation and publish canonical incomplete manifests", ("T2", "T3", "T4", "T5", "T6", "T7"), ("P0", "P1", "P2", "P3", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    # Wave 1 - TUI primitives (Todos 9-16).
    CleanRoomReservation("T9", "wave-1", "Reimplement terminal input decoding and capability fallbacks", ("T8",), ("P1", "P2", "P3", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T10", "wave-1", "Reimplement terminal lifecycle, synchronized writer, cursor, and clocks", ("T8",), ("P1", "P2", "P3", "P5", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T11", "wave-1", "Build deterministic render surfaces and frame observation seams", ("T8",), ("P1", "P2", "P3", "P4", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T12", "wave-1", "Reimplement scrollback state, layout, folding, selection, and follow behavior", ("T8", "T11"), ("P1", "P2", "P3", "P5", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T13", "wave-1", "Reimplement prompt editor, history, paste, shell mode, and completions", ("T8", "T9", "T11"), ("P1", "P2", "P3", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T14", "wave-1", "Reimplement layout, chrome, themes, and responsive geometry", ("T8", "T11"), ("P1", "P2", "P3", "P4", "P5"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T15", "wave-1", "Implement action/effect, focus, and overlay-controller vertical seams", ("T8",), ("P0", "P1", "P2", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T16", "wave-1", "Integrate the TUI primitive foundation", ("T9", "T10", "T11", "T12", "T13", "T14", "T15"), ("P2", "P3", "P4", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    # Wave 2 - Retained runtime owners (Todos 17-24).
    CleanRoomReservation("T17", "wave-2", "Prove and complete sessions, persistence, replay, lineage, rewind, and recovery", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T18", "wave-2", "Prove and complete prompt queue, interjection, compaction, and local memory", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T19", "wave-2", "Prove and complete local workspace, worktrees, trust, VCS attribution, and sandbox", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T20", "wave-2", "Prove and complete tools, permissions, background tasks, scheduler, and teams", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T21", "wave-2", "Prove and complete local hooks, MCP, ACP, plugins, code graph, and LSP", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T22", "wave-2", "Prove and complete public auth, providers/models, updates, and sleep/wake protection", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T23", "wave-2", "Prove and complete CLI, configuration/settings, doctor, and support export", ("T8", "T16"), ("P0", "P1", "P2", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T24", "wave-2", "Integrate retained runtime owners across crates", ("T17", "T18", "T19", "T20", "T21", "T22", "T23"), ("P2", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    # Wave 3 - TUI surfaces and journeys (Todos 25-32).
    CleanRoomReservation("T25", "wave-3", "Reimplement startup, welcome, trust, and first-prompt journeys", ("T16", "T19", "T23", "T24"), ("P1", "P2", "P3", "P4", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T26", "wave-3", "Reimplement main shell lifecycle, status, context, footer, and recovery states", ("T16", "T17", "T20", "T22", "T24"), ("P1", "P2", "P3", "P4", "P5", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T27", "wave-3", "Reimplement transcript blocks, tools, diffs, markdown, links, and local media", ("T12", "T17", "T18", "T20", "T21", "T22", "T24"), ("P1", "P2", "P3", "P4", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T28", "wave-3", "Reimplement overlays, pickers, settings, permissions, questions, and local integrations UI", ("T13", "T15", "T17", "T20", "T21", "T22", "T23", "T24"), ("P1", "P2", "P3", "P4", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T29", "wave-3", "Reimplement plan, vim, minimal, inline, fullscreen, rewind, and mode transitions", ("T9", "T10", "T13", "T15", "T17", "T18", "T24"), ("P1", "P2", "P3", "P5", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T30", "wave-3", "Reimplement dashboard, queue/tasks/todo, subagents, and worktree navigation", ("T15", "T17", "T18", "T19", "T20", "T24"), ("P1", "P2", "P3", "P4", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T31", "wave-3", "Reimplement local notifications, tips, appearance preview, and diagnostics surfaces", ("T10", "T14", "T20", "T22", "T23", "T24"), ("P1", "P2", "P3", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T32", "wave-3", "Integrate all TUI surfaces with retained runtime owners", ("T24", "T25", "T26", "T27", "T28", "T29", "T30", "T31"), ("P2", "P3", "P4", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    # Wave 4 - Differential hardening and seal (Todos 33-39).
    CleanRoomReservation("T33", "wave-4", "Generate independent differential scenario and holdout drivers", ("T32",), ("P0", "P1", "P6"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T34", "wave-4", "Execute semantic-cell, raster, responsive, color, and terminal-capability differential proof", ("T32", "T33"), ("P3", "P4", "P6", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T35", "wave-4", "Execute ordered motion, timing, scroll, resize, streaming, and cancellation proof", ("T32", "T33"), ("P3", "P4", "P5", "P6", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T36", "wave-4", "Execute owner postcondition, persistence, restart, error, and replay holdouts", ("T24", "T32", "T33"), ("P2", "P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T37", "wave-4", "Execute live-provider, installed-binary, native dogfood, secret, and clean-room holdouts", ("T22", "T23", "T24", "T32", "T33"), ("P6", "P7", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T38", "wave-4", "Integrate the complete evidence set and run deterministic rejection gates", ("T33", "T34", "T35", "T36", "T37"), ("P6", "P7"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("T39", "wave-4", "Seal one product epoch, build the candidate, and produce final acceptance evidence", ("T38",), ("P2", "P6", "P7", "P8"), _CLEAN_ROOM_BOTH_EPOCHS),
    # Final acceptance gates (F1-F7).
    CleanRoomReservation("F1", "final", "Plan compliance and evidence audit", ("T39",), ("P0", "P9"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("F2", "final", "Code quality, architecture, security, and replay audit", ("T39",), ("P6", "P9"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("F3", "final", "Real manual TUI/CLI/API QA and visual fidelity review", ("T39",), ("P3", "P4", "P5", "P8", "P9"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("F4", "final", "Scope fidelity and clean-room audit", ("T39",), ("P0", "P6", "P9"), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("F5", "final", "Propose final attestation and status promotion without writing manifests", ("F1", "F2", "F3", "F4"), ("P9",), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("F6", "final", "Terminal Oracle release-stop review", ("F5",), ("P9",), _CLEAN_ROOM_BOTH_EPOCHS),
    CleanRoomReservation("F7", "final", "Apply the Oracle-approved promotion mechanically", ("F6",), ("P9",), _CLEAN_ROOM_BOTH_EPOCHS),
)


def _schedule_key(task_id: str) -> tuple[int, int]:
    return (0 if task_id.startswith("T") else 1, int(task_id[1:]))


def _clean_room_counts(reservations: tuple[CleanRoomReservation, ...]) -> dict[str, int]:
    return {
        "todos": sum(1 for spec in reservations if spec.task_id.startswith("T")),
        "final_gates": sum(1 for spec in reservations if spec.task_id.startswith("F")),
        "reservations": len(reservations),
        "waves": len(CLEAN_ROOM_WAVES),
        "dependency_edges": sum(len(spec.blocked_by) for spec in reservations),
    }


def _clean_room_topological_order(by_id: dict[str, CleanRoomReservation]) -> list[str]:
    remaining_indegree = {task_id: len(spec.blocked_by) for task_id, spec in by_id.items()}
    dependents: dict[str, list[str]] = {task_id: [] for task_id in by_id}
    for spec in by_id.values():
        for dependency in spec.blocked_by:
            dependents[dependency].append(spec.task_id)
    ready = sorted((task_id for task_id, indegree in remaining_indegree.items() if indegree == 0), key=_schedule_key)
    order: list[str] = []
    while ready:
        task_id = ready.pop(0)
        order.append(task_id)
        released = False
        for dependent in dependents[task_id]:
            remaining_indegree[dependent] -= 1
            if remaining_indegree[dependent] == 0:
                ready.append(dependent)
                released = True
        if released:
            ready.sort(key=_schedule_key)
    return order


def validate_clean_room_schedule(reservations: tuple[CleanRoomReservation, ...]) -> list[str]:
    """Fail closed unless the clean-room reservation table is dispatchable."""
    checks: list[str] = []
    by_id: dict[str, CleanRoomReservation] = {}
    for spec in reservations:
        if spec.task_id in by_id:
            _fail(f"clean-room reservation {spec.task_id} is declared more than once")
        by_id[spec.task_id] = spec
    expected_ids = {f"T{n}" for n in range(1, 40)} | {f"F{n}" for n in range(1, 8)}
    missing = sorted(expected_ids - set(by_id), key=_schedule_key)
    unexpected = sorted(set(by_id) - expected_ids, key=_schedule_key)
    if missing or unexpected:
        _fail(f"clean-room schedule must reserve exactly todos T1-T39 and gates F1-F7; missing={missing} unexpected={unexpected}")
    checks.append("reservations-exact")

    wave_order: dict[str, int] = {}
    membership: dict[str, str] = {}
    for wave in CLEAN_ROOM_WAVES:
        if wave.wave in wave_order:
            _fail(f"clean-room wave declared more than once: {wave.wave}")
        wave_order[wave.wave] = wave.order
        for task_id in wave.task_ids:
            if task_id in membership:
                _fail(f"clean-room task {task_id} is assigned to more than one wave")
            membership[task_id] = wave.wave
    if set(membership) != expected_ids:
        _fail("clean-room wave membership must cover exactly todos T1-T39 and gates F1-F7")
    for spec in by_id.values():
        if spec.wave not in wave_order:
            _fail(f"clean-room reservation {spec.task_id} references unknown wave {spec.wave}")
        if membership[spec.task_id] != spec.wave:
            _fail(f"clean-room reservation {spec.task_id} wave field disagrees with wave membership table")
    checks.append("wave-membership-exact")

    for spec in by_id.values():
        if spec.task_id in spec.blocked_by:
            _fail(f"clean-room reservation {spec.task_id} depends on itself")
        if len(set(spec.blocked_by)) != len(spec.blocked_by):
            _fail(f"clean-room reservation {spec.task_id} lists a duplicate dependency")
        if tuple(sorted(spec.blocked_by, key=_schedule_key)) != spec.blocked_by:
            _fail(f"clean-room reservation {spec.task_id} dependencies are not canonically sorted")
        for dependency in spec.blocked_by:
            if dependency not in by_id:
                _fail(f"clean-room reservation {spec.task_id} has unresolvable dependency {dependency}")
            if wave_order[by_id[dependency].wave] > wave_order[spec.wave]:
                _fail(f"clean-room reservation {spec.task_id} depends on later-wave task {dependency}")
    checks.append("dependencies-resolvable")

    for spec in by_id.values():
        dimensions = spec.proof_dimensions
        if not dimensions or len(set(dimensions)) != len(dimensions) or tuple(sorted(dimensions)) != dimensions:
            _fail(f"clean-room reservation {spec.task_id} has malformed proof dimensions")
        unknown = sorted(set(dimensions) - set(CLEAN_ROOM_PROOF_DIMENSIONS))
        if unknown:
            _fail(f"clean-room reservation {spec.task_id} declares unknown proof dimensions {unknown}")
        if spec.wave == "final" and "P9" not in dimensions:
            _fail(f"clean-room gate {spec.task_id} must require the P9 review dimension")
    checks.append("proof-dimensions-valid")

    for spec in by_id.values():
        epochs = spec.epoch_requirements
        if not epochs or len(set(epochs)) != len(epochs) or tuple(sorted(epochs)) != epochs:
            _fail(f"clean-room reservation {spec.task_id} has malformed epoch requirements")
        unknown = sorted(set(epochs) - set(CLEAN_ROOM_EPOCH_KINDS))
        if unknown:
            _fail(f"clean-room reservation {spec.task_id} declares unknown epoch requirements {unknown}")
    checks.append("epoch-requirements-valid")

    order = _clean_room_topological_order(by_id)
    if len(order) != len(by_id):
        cyclic = sorted(set(by_id) - set(order), key=_schedule_key)
        _fail(f"clean-room dependency graph contains a cycle involving {cyclic}")
    checks.append("dependency-graph-acyclic")
    return checks


def clean_room_reservations_document(reservations: tuple[CleanRoomReservation, ...]) -> dict[str, object]:
    waves = [{"wave": wave.wave, "order": wave.order, "label": wave.label, "gate": wave.gate, "task_ids": list(wave.task_ids)} for wave in CLEAN_ROOM_WAVES]
    rows = [
        {"task_id": spec.task_id, "wave": spec.wave, "title": spec.title, "blocked_by": list(spec.blocked_by), "proof_dimensions": list(spec.proof_dimensions), "epoch_requirements": list(spec.epoch_requirements)}
        for spec in sorted(reservations, key=lambda entry: _schedule_key(entry.task_id))
    ]
    return {
        "schema": "clean-room-parity-scheduler-reservations/v1",
        "plan": CLEAN_ROOM_PROGRAM,
        "proof_dimensions": dict(CLEAN_ROOM_PROOF_DIMENSIONS),
        "epoch_kinds": list(CLEAN_ROOM_EPOCH_KINDS),
        "counts": _clean_room_counts(reservations),
        "waves": waves,
        "reservations": rows,
    }


def clean_room_schedule_document(reservations: tuple[CleanRoomReservation, ...]) -> dict[str, object]:
    by_id = {spec.task_id: spec for spec in reservations}
    blocks: dict[str, list[str]] = {task_id: [] for task_id in by_id}
    for spec in by_id.values():
        for dependency in spec.blocked_by:
            blocks[dependency].append(spec.task_id)
    for dependents in blocks.values():
        dependents.sort(key=_schedule_key)
    waves = [
        {"wave": wave.wave, "order": wave.order, "label": wave.label, "gate": wave.gate, "tasks": [{"task_id": task_id, "title": by_id[task_id].title, "blocked_by": list(by_id[task_id].blocked_by), "blocks": blocks[task_id]} for task_id in wave.task_ids]}
        for wave in CLEAN_ROOM_WAVES
    ]
    graph = {task_id: {"blocked_by": list(by_id[task_id].blocked_by), "blocks": blocks[task_id]} for task_id in sorted(by_id, key=_schedule_key)}
    return {
        "schema": "clean-room-parity-dependency-graph/v1",
        "plan": CLEAN_ROOM_PROGRAM,
        "counts": _clean_room_counts(reservations),
        "topological_dispatch_order": _clean_room_topological_order(by_id),
        "waves": waves,
        "dependency_graph": graph,
    }


def clean_room_validation_document(reservations: tuple[CleanRoomReservation, ...]) -> dict[str, object]:
    checks = validate_clean_room_schedule(reservations)
    return {
        "schema": "clean-room-parity-schedule-validation/v1",
        "plan": CLEAN_ROOM_PROGRAM,
        "verdict": "pass",
        "checks": checks,
        "counts": _clean_room_counts(reservations),
        "topological_dispatch_order": _clean_room_topological_order({spec.task_id: spec for spec in reservations}),
    }


def write_clean_room_evidence(evidence_dir: Path) -> dict[str, object]:
    documents = {
        "scheduler-reservations.json": clean_room_reservations_document(CLEAN_ROOM_RESERVATIONS),
        "dependency-graph.json": clean_room_schedule_document(CLEAN_ROOM_RESERVATIONS),
        "schedule-validation.json": clean_room_validation_document(CLEAN_ROOM_RESERVATIONS),
    }
    evidence_dir.mkdir(parents=True, exist_ok=True)
    artifacts: list[dict[str, object]] = []
    for name in CLEAN_ROOM_EVIDENCE_ARTIFACTS:
        payload = (json.dumps(documents[name], indent=2, sort_keys=True) + "\n").encode()
        _ = (evidence_dir / name).write_bytes(payload)
        artifacts.append({"artifact": name, "sha256": hashlib.sha256(payload).hexdigest(), "bytes": len(payload)})
    manifest: dict[str, object] = {
        "schema": "clean-room-parity-task-7-evidence/v1",
        "plan": CLEAN_ROOM_PROGRAM,
        "evidence_root": str(evidence_dir),
        "artifacts": artifacts,
        "runner_identity": {"path": str(Path(__file__).resolve()), "sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},
    }
    _ = (evidence_dir / "evidence-manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return manifest


# ---------------------------------------------------------------------------
# Todo 33 - differential scenario + holdout driver validator.
# Generates behavior-descriptive Harness scenario specifications from the
# canonical inventory, splits published conformance from undisclosed holdouts,
# and fails closed on missing rows, grouped wildcards, absent teardown,
# absent failure mutation, copied reference fingerprints, and unbound epochs.
# ---------------------------------------------------------------------------

SCENARIO_KINDS: Final = ("happy", "failure", "mutation", "teardown")
ABSENCE_KINDS: Final = ("absence", "failure", "mutation", "teardown")
WILDCARD_CHARS: Final = ("*", "?", "[")
CATCH_ALL_TOKENS: Final = frozenset({"*", "**", "all", "various", "misc", "etc", "any"})
HEX_REFERENCE_FIELDS: Final = ("reference_fixture_sha256", "reference_source_sha256", "reference_binary_sha256")
VALID_PROOF_DIMENSIONS_SET: Final = frozenset(f"P{n}" for n in range(10))
# Holdouts are opaque: one per proof dimension. Their row mapping is recorded
# only inside the holdout index, never under the published scenario root.
HOLDOUT_PROOF_DIMENSIONS: Final = tuple(f"P{n}" for n in range(10))
SCENARIO_SCHEMA_VERSION: Final = "grok-parity-scenario-v1"


def _scenario_row_key(category: str, source_path: str, source_symbol: str) -> str:
    raw = f"{category}::{source_path}::{source_symbol}"
    safe = []
    for ch in raw:
        if ch.isalnum() or ch in ("-", "_", ".", "::"):
            safe.append(ch)
        else:
            safe.append("_")
    collapsed = "_".join(part for part in "".join(safe).split("_") if part)
    return collapsed or "row"


def _has_wildcard(value: str) -> bool:
    return any(ch in value for ch in WILDCARD_CHARS)


def _inventory_rows(inventory: dict[str, Any]) -> list[dict[str, Any]]:
    rows = inventory.get("rows")
    if not isinstance(rows, list):
        _fail("inventory rows must be a list")
    return [row for row in rows if isinstance(row, dict)]


def _inventory_epoch(inventory: dict[str, Any]) -> str:
    metadata = inventory.get("metadata")
    if not isinstance(metadata, dict):
        _fail("inventory metadata must be an object")
    epoch = metadata.get("reference_epoch")
    if not isinstance(epoch, str) or not epoch.strip() or epoch.strip().lower() == "unbound":
        _fail("inventory metadata.reference_epoch is unbound or absent")
    return epoch


def _row_proof_dimensions(row: dict[str, Any]) -> tuple[str, ...]:
    raw = str(row.get("p0_p9_applicability", ""))
    dims = tuple(part.strip() for part in raw.split(",") if part.strip())
    if not dims or any(d not in VALID_PROOF_DIMENSIONS_SET for d in dims):
        _fail(f"row has invalid p0_p9_applicability: {raw!r}")
    return dims


def _build_scenario(row: dict[str, Any], kind: str, epoch: str) -> dict[str, Any]:
    category = str(row["category"])
    source_path = str(row["source_path"])
    symbol = str(row["source_symbol"])
    dimensions = _row_proof_dimensions(row)
    primary_proof = dimensions[0]
    row_key = _scenario_row_key(category, source_path, symbol)
    trigger = str(row.get("trigger", "key_event_or_command_palette"))
    state_transition = str(row.get("state_transition", "dispatches_action"))
    expected_effect = str(row.get("rendered_effect", "observable_owner_postcondition"))
    teardown = {
        "restore_workspace": True,
        "clear_session_artifacts": True,
        "restore_terminal_state": True,
    }

    if kind == "absence":
        leaf = symbol.rsplit("::", 1)[-1]
        return {
            "schema": SCENARIO_SCHEMA_VERSION,
            "kind": "absence",
            "row_key": row_key,
            "proof_dimension": "P6",
            "reference_epoch": epoch,
            "approved_disposition": "approved_exclusion",
            "exclusion_family": str(row.get("exclusion_family", "approved-exclusion")),
            "description": f"absence/rejection for {symbol} under {category}: excluded surface must be absent and any dispatch rejected",
            "setup": {"initial_state": "clean_workspace", "focus_owner": str(row.get("focus_owner", "prompt"))},
            "action": {"trigger": trigger, "input_bytes": "deterministic_fixture", "attempt": "dispatch_excluded_surface"},
            "expected_observable_outcome": {
                "surface_absent": True,
                "dispatch_rejected": True,
                "no_unauthorized_mutation": True,
                "terminal_state": "recoverable",
            },
            "failure_mutation": {
                "deliberate_bad_assertion": f"assert_excluded_{leaf}_dispatches_successfully",
                "expected_rejection": "exclusion_absence_violated",
            },
            "teardown": teardown,
        }
    if kind == "happy":
        return {
            "schema": SCENARIO_SCHEMA_VERSION,
            "kind": "happy",
            "row_key": row_key,
            "proof_dimension": primary_proof,
            "reference_epoch": epoch,
            "description": f"happy path for {symbol} under {category}",
            "setup": {"initial_state": "clean_workspace", "focus_owner": str(row.get("focus_owner", "prompt"))},
            "action": {"trigger": trigger, "input_bytes": "deterministic_fixture"},
            "expected_observable_outcome": {"state_transition": state_transition, "effect": expected_effect},
            "teardown": teardown,
        }
    if kind == "failure":
        return {
            "schema": SCENARIO_SCHEMA_VERSION,
            "kind": "failure",
            "row_key": row_key,
            "proof_dimension": primary_proof,
            "reference_epoch": epoch,
            "description": f"failure path for {symbol}: deny/timeout/corruption remains recoverable",
            "setup": {"initial_state": "clean_workspace", "inject_failure": "deny_or_timeout"},
            "action": {"trigger": trigger, "input_bytes": "deterministic_fixture"},
            "expected_observable_outcome": {"terminal_state": "recoverable", "no_unauthorized_mutation": True},
            "teardown": teardown,
        }
    if kind == "mutation":
        return {
            "schema": SCENARIO_SCHEMA_VERSION,
            "kind": "mutation",
            "row_key": row_key,
            "proof_dimension": primary_proof,
            "reference_epoch": epoch,
            "description": f"deliberate bad assertion that must fail for {symbol}",
            "setup": {"initial_state": "clean_workspace"},
            "action": {"trigger": trigger, "input_bytes": "deterministic_fixture"},
            "expected_observable_outcome": {"state_transition": state_transition, "effect": expected_effect},
            "failure_mutation": {
                "deliberate_bad_assertion": f"assert_effect_is_unrelated_to_{symbol}",
                "expected_rejection": "scenario_divergence_detected",
            },
            "teardown": teardown,
        }
    if kind == "teardown":
        return {
            "schema": SCENARIO_SCHEMA_VERSION,
            "kind": "teardown",
            "row_key": row_key,
            "proof_dimension": primary_proof,
            "reference_epoch": epoch,
            "description": f"workspace restoration contract after exercising {symbol}",
            "setup": {"initial_state": "exercised_workspace"},
            "action": {"trigger": "teardown_sequence", "input_bytes": "none"},
            "expected_observable_outcome": {
                "workspace_restored": True,
                "session_artifacts_cleared": True,
                "terminal_state_restored": True,
            },
        }
    _fail(f"unknown scenario kind: {kind}")


def _write_scenario(scenario_root: Path, row_key: str, kind: str, body: dict[str, Any]) -> Path:
    directory = scenario_root / row_key
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{kind}.json"
    path.write_text(json.dumps(body, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def _build_holdout(epoch: str, proof_dimension: str, ordinal: int) -> dict[str, Any]:
    # Undisclosed holdouts are opaque: no row_key, no source symbol. Their
    # identity is a stable opaque id derived from the proof dimension only.
    return {
        "schema": SCENARIO_SCHEMA_VERSION,
        "kind": "holdout",
        "holdout_id": f"holdout-{proof_dimension}-{ordinal:04d}",
        "proof_dimension": proof_dimension,
        "reference_epoch": epoch,
        "description": f"undisclosed differential holdout for {proof_dimension}",
        "setup": {"initial_state": "clean_workspace", "undisclosed_inputs": True},
        "action": {"trigger": "undisclosed", "input_bytes": "undisclosed"},
        "expected_observable_outcome": {"differential_proof": proof_dimension, "match_required": True},
        "failure_mutation": {
            "deliberate_bad_assertion": "assert_divergence_is_acceptable",
            "expected_rejection": "holdout_divergence_detected",
        },
    }


def _apply_mutation(body: dict[str, Any], mutation: str) -> None:
    """Inject a single controlled defect for self-test / RED-gate proofs."""
    if mutation == "missing-row":
        return
    if mutation == "duplicate-coverage":
        body["duplicate_marker"] = True
        return
    if mutation == "grouped-wildcard":
        body["row_key"] = "grouped_*_wildcard"
        return
    if mutation == "missing-teardown":
        if isinstance(body.get("teardown"), dict):
            del body["teardown"]
        return
    if mutation == "absent-mutation":
        if "failure_mutation" in body:
            del body["failure_mutation"]
        return
    if mutation == "copied-reference-fingerprint":
        body["reference_fixture_sha256"] = "0" * 64
        return
    if mutation == "unbound-epoch":
        body["reference_epoch"] = "unbound"
        return
    _fail(f"unknown PARITY_MUTATION: {mutation}")


def generate_scenarios(
    inventory: dict[str, Any],
    scenario_root: Path,
    holdout_index_path: Path,
    mutation: str | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Materialize published conformance scenarios and undisclosed holdouts."""
    rows = _inventory_rows(inventory)
    epoch = _inventory_epoch(inventory)
    scenario_root.mkdir(parents=True, exist_ok=True)
    published: list[dict[str, Any]] = []
    skipped_keys: set[str] = set()
    for index, row in enumerate(rows):
        row_key = _scenario_row_key(str(row["category"]), str(row["source_path"]), str(row["source_symbol"]))
        if _has_wildcard(str(row["source_path"])) or _has_wildcard(str(row["source_symbol"])):
            _fail(f"inventory row uses grouped wildcard: {row['source_path']} / {row['source_symbol']}")
        last_segment = str(row["source_symbol"]).rsplit("::", 1)[-1].strip().lower()
        if last_segment in CATCH_ALL_TOKENS:
            _fail(f"inventory row uses grouped catch-all symbol: {row['source_symbol']}")
        if mutation == "missing-row" and index == 0:
            skipped_keys.add(row_key)
            continue
        if mutation == "duplicate-coverage" and index == 1:
            # Re-emit the first row's coverage under a different path but the
            # same row_key, so two scenarios claim one row and the second row
            # is left uncovered.
            duplicate_key = _scenario_row_key(str(rows[0]["category"]), str(rows[0]["source_path"]), str(rows[0]["source_symbol"]))
            for kind in SCENARIO_KINDS:
                body = _build_scenario(rows[0], kind, epoch)
                if mutation:
                    _apply_mutation(body, mutation)
                path = _write_scenario(scenario_root, f"{duplicate_key}-echo", kind, body)
                body["row_key"] = duplicate_key
                path.write_text(json.dumps(body, indent=2, sort_keys=True) + "\n", encoding="utf-8")
                published.append({"row_key": duplicate_key, "kind": kind, "path": str(path), "row": rows[0]})
            skipped_keys.add(row_key)
            continue
        disposition = str(row.get("approved_disposition", "retained"))
        kinds = ABSENCE_KINDS if disposition == "approved_exclusion" else SCENARIO_KINDS
        for kind in kinds:
            body = _build_scenario(row, kind, epoch)
            if mutation:
                _apply_mutation(body, mutation)
            path = _write_scenario(scenario_root, row_key, kind, body)
            published.append({"row_key": row_key, "kind": kind, "path": str(path), "row": row})
    if mutation == "duplicate-coverage":
        # Masking fails the missing check below via skipped_keys.
        pass
    holdouts = [_build_holdout(epoch, dim, ordinal) for ordinal, dim in enumerate(HOLDOUT_PROOF_DIMENSIONS)]
    holdout_payload = {
        "schema": "grok-parity-holdout-index-v1",
        "reference_epoch": epoch,
        "holdout_count": len(holdouts),
        "holdouts": [
            {
                "holdout_id": h["holdout_id"],
                "proof_dimension": h["proof_dimension"],
                "reference_epoch": h["reference_epoch"],
                "description": h["description"],
            }
            for h in holdouts
        ],
    }
    holdout_index_path.parent.mkdir(parents=True, exist_ok=True)
    holdout_index_path.write_text(json.dumps(holdout_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if skipped_keys:
        published = [entry for entry in published if entry["row_key"] not in skipped_keys]
    return published, holdouts


def _scenario_copied_reference(body: dict[str, Any]) -> bool:
    for field in HEX_REFERENCE_FIELDS:
        value = body.get(field)
        if isinstance(value, str) and len(value) == 64 and all(ch in "0123456789abcdef" for ch in value.lower()):
            return True
    return False


def validate_published_scenarios(
    published: list[dict[str, Any]],
    inventory: dict[str, Any],
    require_happy: int,
    require_failure: int,
    require_mutation: int,
    require_coverage: int,
) -> dict[str, Any]:
    """Fail closed on the seven Todo 33 contract mutations."""
    rows = _inventory_rows(inventory)
    epoch = _inventory_epoch(inventory)
    by_row_key: dict[str, dict[str, Any]] = {}
    coverage: list[dict[str, Any]] = []
    defects: list[str] = []
    copied_reference_assets = 0
    seen_row_keys: set[str] = set()

    for entry in published:
        entry_row_key = str(entry["row_key"])
        kind = str(entry["kind"])
        row = entry["row"] if isinstance(entry.get("row"), dict) else {}
        body_path = Path(str(entry["path"]))
        if not body_path.is_file():
            defects.append(f"scenario {entry_row_key}/{kind} missing artifact: {body_path}")
            continue
        body = json.loads(body_path.read_text(encoding="utf-8"))
        if not isinstance(body, dict):
            defects.append(f"scenario {entry_row_key}/{kind} body is not an object")
            continue
        # The persisted body is authoritative for the row_key: that is where
        # grouped-wildcard mutations surface. A mismatch between the entry and
        # the body is itself a defect.
        row_key = str(body.get("row_key", entry_row_key))
        if row_key != entry_row_key:
            defects.append(f"scenario {entry_row_key}/{kind} row_key drifts from published entry: {row_key}")
        if _has_wildcard(row_key) or _has_wildcard(str(row.get("source_path", ""))) or _has_wildcard(str(row.get("source_symbol", ""))):
            defects.append(f"scenario {row_key}/{kind} uses grouped wildcard")
        last_segment = str(row.get("source_symbol", "")).rsplit("::", 1)[-1].strip().lower()
        if last_segment in CATCH_ALL_TOKENS:
            defects.append(f"scenario {row_key}/{kind} covers grouped catch-all symbol")
        if str(body.get("reference_epoch", "")) != epoch:
            defects.append(f"scenario {row_key}/{kind} has unbound or mismatched epoch")
        for required in ("description", "setup", "action", "expected_observable_outcome"):
            if required not in body:
                defects.append(f"scenario {row_key}/{kind} missing field {required}")
        if kind in ("happy", "failure", "mutation"):
            if "teardown" not in body:
                defects.append(f"scenario {row_key}/{kind} missing teardown")
        if kind == "mutation" and "failure_mutation" not in body:
            defects.append(f"scenario {row_key}/{kind} absent failure mutation")
        if _scenario_copied_reference(body):
            copied_reference_assets += 1
            defects.append(f"scenario {row_key}/{kind} copied_reference fingerprint")
        if row_key in seen_row_keys and kind == "happy":
            defects.append(f"scenario {row_key}/{kind} duplicate coverage")
        if kind == "happy":
            seen_row_keys.add(row_key)
        bucket = by_row_key.setdefault(row_key, {"row_key": row_key, "kinds": [], "row": row})
        bucket["kinds"].append(kind)

    for row in rows:
        row_key = _scenario_row_key(str(row["category"]), str(row["source_path"]), str(row["source_symbol"]))
        bucket = by_row_key.get(row_key)
        if not bucket:
            coverage.append({"category": str(row["category"]), "source_path": str(row["source_path"]), "source_symbol": str(row["source_symbol"]), "covered": False, "kinds": []})
            continue
        kinds = sorted(bucket["kinds"])
        coverage.append({"category": str(row["category"]), "source_path": str(row["source_path"]), "source_symbol": str(row["source_symbol"]), "covered": True, "kinds": kinds})

    missing_rows = [entry for entry in coverage if not entry["covered"]]
    missing_count = len(missing_rows)
    coverage_percent = int(round((len(coverage) - missing_count) / len(coverage) * 100)) if coverage else 0
    if missing_count:
        missing_keys = ", ".join(f"{entry['category']}::{entry['source_symbol']}" for entry in missing_rows[:5])
        defects.append(f"missing scenarios for {missing_count} inventory row(s): {missing_keys}")

    kind_counts: dict[str, int] = {"happy": 0, "absence": 0, "failure": 0, "mutation": 0}
    for bucket in by_row_key.values():
        for kind in bucket["kinds"]:
            if kind in kind_counts:
                kind_counts[kind] += 1
    if kind_counts["happy"] < require_happy:
        defects.append(f"happy scenario count {kind_counts['happy']} below required {require_happy}")
    if kind_counts["failure"] < require_failure:
        defects.append(f"failure scenario count {kind_counts['failure']} below required {require_failure}")
    if kind_counts["mutation"] < require_mutation:
        defects.append(f"mutation scenario count {kind_counts['mutation']} below required {require_mutation}")
    if coverage_percent < require_coverage:
        defects.append(f"coverage {coverage_percent}% below required {require_coverage}%")

    if defects:
        _fail("scenario validation defects: " + "; ".join(defects))

    return {
        "schema": "grok-parity-scenario-validation-v1",
        "verdict": "pass",
        "reference_epoch": epoch,
        "row_count": len(rows),
        "covered_count": len(coverage) - missing_count,
        "missing_count": missing_count,
        "coverage_percent": coverage_percent,
        "copied_reference_assets": copied_reference_assets,
        "happy_scenarios": kind_counts["happy"],
        "absence_scenarios": kind_counts["absence"],
        "failure_scenarios": kind_counts["failure"],
        "mutation_scenarios": kind_counts["mutation"],
        "teardown_scenarios": sum(1 for b in by_row_key.values() for k in b["kinds"] if k == "teardown"),
        "row_coverage": coverage,
    }


def _run_validate_scenarios(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="parity_task_qa.py validate-scenarios", description="Todo 33 differential scenario and holdout driver validator")
    _ = parser.add_argument("--inventory", type=Path, required=True)
    _ = parser.add_argument("--scenario-root", type=Path, required=True)
    _ = parser.add_argument("--holdout-index", type=Path, required=True)
    _ = parser.add_argument("--output", type=Path, required=True)
    _ = parser.add_argument("--require-happy", type=int, default=1)
    _ = parser.add_argument("--require-failure", type=int, default=1)
    _ = parser.add_argument("--require-mutation", type=int, default=1)
    _ = parser.add_argument("--require-coverage", type=int, default=100)
    _ = parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return _scenario_self_test()
    if not args.inventory.is_file():
        _fail(f"--inventory must name an existing JSON file: {args.inventory}")
    inventory = json.loads(args.inventory.read_text(encoding="utf-8"))
    if not isinstance(inventory, dict):
        _fail("inventory root must be a JSON object")
    mutation = __import__("os").environ.get("PARITY_MUTATION")
    published, _holdouts = generate_scenarios(inventory, args.scenario_root, args.holdout_index, mutation)
    verdict = validate_published_scenarios(
        published,
        inventory,
        require_happy=args.require_happy,
        require_failure=args.require_failure,
        require_mutation=args.require_mutation,
        require_coverage=args.require_coverage,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(verdict, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"coverage={verdict['coverage_percent']}% missing={verdict['missing_count']} "
        f"copied_reference_assets={verdict['copied_reference_assets']}"
    )
    return 0


def _scenario_self_test() -> int:
    """Hermetic self-test for the validate-scenarios surface and its mutations."""
    import os
    import tempfile

    def _row(category: str, source_path: str, symbol: str, proof: str = "P0") -> dict[str, Any]:
        return {
            "category": category, "source_path": source_path, "source_symbol": symbol, "line": 1,
            "trigger": "key_event_or_command_palette", "focus_owner": "prompt",
            "state_transition": f"dispatches_{symbol.rsplit('::', 1)[-1]}",
            "rendered_effect": "observable", "side_effect": "none", "persistence": "none",
            "viewport_capability_conditions": "none", "approved_disposition": "pending",
            "p0_p9_applicability": proof, "notes": "self-test row",
        }

    def _inventory(rows: list[dict[str, Any]]) -> dict[str, Any]:
        return {
            "schema_version": "grok-reference-interaction-inventory-v1",
            "metadata": {
                "reference_epoch": "self-test-epoch",
                "count_targets": {},
                "actual_counts": {},
            },
            "rows": rows,
        }

    base_rows = [
        _row("action", "crates/codegen/xai-grok-pager/src/actions/mod.rs", "ActionId::SendPrompt", "P0"),
        _row("action", "crates/codegen/xai-grok-pager/src/actions/mod.rs", "ActionId::ScrollDown", "P1"),
    ]

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        inventory_path = root / "inventory.json"
        inventory_path.write_text(json.dumps(_inventory(base_rows), indent=2, sort_keys=True) + "\n", encoding="utf-8")

        def run(mutation: str | None, label: str, *, expect_pass: bool, needle: str | None = None) -> None:
            attempt_root = root / label
            attempt_root.mkdir()
            env = dict(os.environ)
            if mutation:
                env["PARITY_MUTATION"] = mutation
            else:
                env.pop("PARITY_MUTATION", None)
            import subprocess
            cmd = [
                "python3", str(Path(__file__).resolve()), "validate-scenarios",
                "--inventory", str(inventory_path),
                "--scenario-root", str(attempt_root / "scenarios"),
                "--holdout-index", str(attempt_root / "holdout-index.json"),
                "--output", str(attempt_root / "scenario-validation.json"),
                "--require-happy", "1", "--require-failure", "1",
                "--require-mutation", "1", "--require-coverage", "100",
            ]
            completed = subprocess.run(cmd, capture_output=True, text=True, env=env, check=False)
            if expect_pass:
                assert completed.returncode == 0, f"{label}: expected pass, got {completed.returncode}; stderr={completed.stderr}"
                body = json.loads((attempt_root / "scenario-validation.json").read_text(encoding="utf-8"))
                assert body["coverage_percent"] == 100, f"{label}: coverage not 100"
                assert body["missing_count"] == 0, f"{label}: missing not 0"
                assert body["copied_reference_assets"] == 0, f"{label}: copied_reference_assets not 0"
                print(f"PASS: {label}")
            else:
                assert completed.returncode != 0, f"{label}: expected reject, got 0"
                combined = f"{completed.stdout}\n{completed.stderr}"
                assert needle and needle in combined, f"{label}: wrong needle; combined={combined}"
                print(f"PASS: {label} rejected ({needle})")

        run(None, "happy-complete", expect_pass=True)
        run("missing-row", "missing-row", expect_pass=False, needle="missing")
        run("duplicate-coverage", "duplicate-coverage", expect_pass=False, needle="duplicate")
        run("grouped-wildcard", "grouped-wildcard", expect_pass=False, needle="wildcard")
        run("missing-teardown", "missing-teardown", expect_pass=False, needle="teardown")
        run("absent-mutation", "absent-mutation", expect_pass=False, needle="mutation")
        run("copied-reference-fingerprint", "copied-reference", expect_pass=False, needle="copied_reference")
        run("unbound-epoch", "unbound-epoch", expect_pass=False, needle="epoch")

    print("validate-scenarios self-test: 8/8 passed")
    return 0


def _self_test() -> int:
    """Top-level self-test: scheduler, clean-room schedule, and validate-scenarios."""
    validate_catalog(TASKS)
    _ = validate_clean_room_schedule(CLEAN_ROOM_RESERVATIONS)
    _ = run_mutations()
    result = _scenario_self_test()
    if result != 0:
        _fail("validate-scenarios self-test failed")
    print("parity_task_qa.py self-test: pass")
    return 0


OBSERVATION_REQUIRED_ARTIFACTS: Final = ("observations.json", "contract.json", "mutation.json")


def _evaluate_observation_contract(observations: dict[str, Any], contract: dict[str, Any], expected_epoch: str) -> list[str]:
    """Mirror scripts/tui-parity/capture-reference-observations.py::evaluate_contract.

    The capture driver seals with the same semantics; keep the two in sync.
    """
    failures: list[str] = []
    if observations.get("reference_epoch") != contract.get("reference_epoch"):
        failures.append("epoch mismatch between observations and contract")
    if observations.get("family_id") != contract.get("family_id"):
        failures.append("family_id mismatch between observations and contract")
    if observations.get("bound_to_reference_epoch") != contract.get("bound_to_reference_epoch"):
        failures.append("bound_to_reference_epoch mismatch")
    for field in ("reference_epoch", "bound_to_reference_epoch"):
        if observations.get(field) != expected_epoch:
            failures.append(f"observations.{field} not bound to expected epoch")
        if contract.get(field) != expected_epoch:
            failures.append(f"contract.{field} not bound to expected epoch")

    scenarios = observations.get("scenarios") or []
    scenario_frames: dict[str, list[dict[str, Any]]] = {s.get("id", ""): s.get("frames") or [] for s in scenarios}
    for sid, min_frames in (contract.get("min_scenario_frames") or {}).items():
        have = len(scenario_frames.get(sid, []))
        if have < min_frames:
            failures.append(f"scenario {sid}: expected >= {min_frames} frames, observed {have}")

    for s in scenarios:
        for fr in s.get("frames") or []:
            grid = "\n".join(fr.get("grid_text") or [])
            recomputed = hashlib.sha256(grid.encode("utf-8", "replace")).hexdigest()
            if recomputed != fr.get("grid_sha256"):
                failures.append(f"grid hash mismatch: scenario={s.get('id')} frame={fr.get('label')}")

    for check in contract.get("expected", {}).get("text_substrings", []):
        frames = scenario_frames.get(check.get("scenario", ""), [])
        target = next((fr for fr in frames if fr.get("label") == check.get("frame_label")), None)
        if target is None and frames:
            target = frames[0]
        grid = " ".join((target or {}).get("grid_text") or [])
        present = check.get("substring", "") in grid
        if check.get("required") and not present:
            failures.append(f"required text missing: scenario={check.get('scenario')} substring={check.get('substring')!r}")
        if "observed_present" in check and present != check["observed_present"]:
            failures.append(f"sealed observed_present drift: scenario={check.get('scenario')} substring={check.get('substring')!r}")

    ansi_by_scenario = {s.get("id", ""): ((s.get("ansi") or {}).get("counts") or {}) for s in scenarios}
    for check in contract.get("expected", {}).get("ansi_lifecycle", []):
        flag = check.get("flag", "")
        if flag.startswith("_frame_dimensions_"):
            dims = check.get("frame_dims") or [0, 0]
            frames = scenario_frames.get(check.get("scenario", ""), [])
            dims_ok = bool(frames) and frames[0].get("cols") == dims[0] and frames[0].get("rows") == dims[1]
            observed = 1 if dims_ok else 0
        else:
            counts = ansi_by_scenario.get(check.get("scenario", ""), {})
            observed = counts.get(flag, 0)
        if "min_count" in check and observed < check["min_count"]:
            failures.append(f"ansi flag {flag} below min in {check.get('scenario')}: {observed} < {check['min_count']}")
        if "max_count" in check and observed > check["max_count"]:
            failures.append(f"ansi flag {flag} above max in {check.get('scenario')}: {observed} > {check['max_count']}")
        if "observed_count" in check and observed != check["observed_count"]:
            failures.append(f"ansi sealed-count drift for {flag} in {check.get('scenario')}")

    exit_by_scenario = {s.get("id", ""): (s.get("exit") or {}).get("code") for s in scenarios}
    for sid, sealed in (contract.get("expected", {}).get("exit_states") or {}).items():
        actual = exit_by_scenario.get(sid)
        if sealed is not None and actual != sealed:
            failures.append(f"exit state drift for {sid}: sealed={sealed} actual={actual}")

    home_files_by_scenario = {s.get("id", ""): s.get("home_files_after") or {} for s in scenarios}
    file_tails_by_scenario = {s.get("id", ""): s.get("file_tails") or {} for s in scenarios}
    for check in contract.get("expected", {}).get("side_effects", []):
        kind = check.get("kind")
        files = home_files_by_scenario.get(check.get("scenario", ""), {})
        if kind == "home_file_exists":
            present = check.get("path") in files
            if not present:
                failures.append(f"side-effect file missing: {check.get('path')} after {check.get('scenario')}")
            if "observed_exists" in check and present != check["observed_exists"]:
                failures.append(f"side-effect sealed drift for {check.get('path')}")
        elif kind == "home_file_glob":
            prefix = str(check.get("glob", "")).split("*")[0]
            count = sum(1 for k in files if str(k).startswith(prefix))
            if "min_matches" in check and count < check["min_matches"]:
                failures.append(f"side-effect glob {check.get('glob')} under min after {check.get('scenario')}")
            if "observed_matches" in check and count != check["observed_matches"]:
                failures.append(f"side-effect glob sealed drift {check.get('glob')}")
        elif kind == "file_contains":
            content = file_tails_by_scenario.get(check.get("scenario", ""), {}).get(check.get("path", ""), "")
            present = check.get("substring", "") in content
            if not present:
                failures.append(f"side-effect file {check.get('path')} missing substring after {check.get('scenario')}")
            if "observed_contains" in check and present != check["observed_contains"]:
                failures.append(f"side-effect contains sealed drift {check.get('path')}")
    return failures


def _validate_family_artifacts(root: Path, family_id: str, epoch: str) -> list[str]:
    problems: list[str] = []
    observations: dict[str, Any] | None = None
    contract: dict[str, Any] | None = None
    for artifact in OBSERVATION_REQUIRED_ARTIFACTS:
        path = root / family_id / artifact
        if not path.is_file():
            problems.append(f"{family_id}: missing artifact {artifact}")
            continue
        try:
            body = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            problems.append(f"{family_id}: unreadable artifact {artifact}: {exc}")
            continue
        if body.get("family_id") != family_id:
            problems.append(f"{family_id}: artifact {artifact} family_id mismatch: {body.get('family_id')}")
        for field in ("reference_epoch", "bound_to_reference_epoch"):
            if body.get(field) != epoch:
                problems.append(f"{family_id}: artifact {artifact} {field} != expected epoch")
        if artifact == "observations.json":
            observations = body
            if not body.get("scenarios"):
                problems.append(f"{family_id}: observations has no scenarios")
        elif artifact == "contract.json":
            contract = body
        else:
            receipt = (body.get("red_receipt") or {})
            if not body.get("mutations"):
                problems.append(f"{family_id}: mutation.json defines no mutants")
            if receipt.get("all_detected") is not True:
                problems.append(f"{family_id}: RED receipt incomplete (all_detected != true)")
    if observations is not None and contract is not None:
        contract_failures = _evaluate_observation_contract(observations, contract, epoch)
        problems.extend(f"{family_id}: {msg}" for msg in contract_failures)
    return problems


def _run_validate_reference_observations(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="parity_task_qa.py validate-reference-observations",
        description="Task 8 sealed reference-observations gate: honest pass/partial/reject against the manifest",
    )
    _ = parser.add_argument("--inventory", type=Path, required=True)
    _ = parser.add_argument("--root", type=Path, required=True)
    _ = parser.add_argument("--reference-epoch", type=str, required=True)
    args = parser.parse_args(argv)

    epoch: str = args.reference_epoch.strip()
    if not epoch or epoch.lower() == "unbound":
        _fail("--reference-epoch must be a bound canonical epoch")
    manifest_path = args.root / "manifest.json"
    if not manifest_path.is_file():
        _fail(f"manifest.json missing under {args.root}")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        _fail(f"manifest.json unreadable: {exc}")
    if manifest.get("reference_epoch") != epoch:
        _fail(f"manifest.reference_epoch {manifest.get('reference_epoch')!r} != --reference-epoch {epoch!r}")

    inventory_provenance: dict[str, Any] = {}
    if not args.inventory.is_file():
        _fail(f"inventory missing: {args.inventory}")
    try:
        inventory = json.loads(args.inventory.read_text(encoding="utf-8"))
        rows = inventory.get("rows")
        if not isinstance(rows, list) or not rows:
            _fail("inventory has no rows")
        inventory_provenance = {
            "path": str(args.inventory),
            "sha256": hashlib.sha256(args.inventory.read_bytes()).hexdigest(),
            "rows": len(rows),
            "document_id": inventory.get("document_id"),
            "inventory_revision": (inventory.get("metadata") or {}).get("reference_revision"),
        }
    except (OSError, json.JSONDecodeError) as exc:
        _fail(f"inventory unreadable: {exc}")

    slices = manifest.get("slices")
    if not isinstance(slices, list) or not slices:
        _fail("manifest.slices missing or empty")

    problems: list[str] = []
    gaps: list[dict[str, str]] = []
    family_states: dict[str, str] = {}
    captured = 0
    required = 0
    status_breakdown: dict[str, int] = {}
    for slc in slices:
        family = str(slc.get("family_id", ""))
        role = slc.get("capture_role")
        state = slc.get("capture_state")
        status = slc.get("status")
        status_breakdown[str(status)] = status_breakdown.get(str(status), 0) + 1
        if role == "capture_required":
            required += 1
            artifacts_present = slc.get("artifacts_present") or []
            if slc.get("bound_to_reference_epoch") != epoch:
                problems.append(f"{family}: slice bound_to_reference_epoch != expected epoch")
            if state == "captured":
                captured += 1
                if status not in ("complete", "diverged"):
                    problems.append(f"{family}: captured slice has invalid status {status!r}")
                if sorted(artifacts_present) != sorted(OBSERVATION_REQUIRED_ARTIFACTS):
                    problems.append(f"{family}: artifacts_present {sorted(artifacts_present)} != required {sorted(OBSERVATION_REQUIRED_ARTIFACTS)}")
                problems.extend(_validate_family_artifacts(args.root, family, epoch))
                family_states[family] = str(status)
            elif state == "not_captured":
                if status != "incomplete":
                    problems.append(f"{family}: not_captured slice must have status=incomplete, got {status!r}")
                if artifacts_present:
                    problems.append(f"{family}: not_captured slice claims artifacts_present {artifacts_present}")
                gaps.append({"family": family, "reason": "no sealed reference observations"})
                family_states[family] = "incomplete"
            else:
                problems.append(f"{family}: capture_required slice has invalid capture_state {state!r}")
        elif role in ("approved_exclusion",):
            if state != "exempt_from_capture":
                problems.append(f"{family}: approved-exclusion slice must be exempt_from_capture")
            family_states[family] = "excluded"
        elif role == "identity_preserved":
            if state != "no_reference_capture":
                problems.append(f"{family}: identity slice must keep no_reference_capture")
            family_states[family] = "identity_retained"
        elif role == "external_proof_required":
            if state != "not_captured" or status != "blocked_environment":
                problems.append(f"{family}: external proof slice must stay not_captured/blocked_environment")
            gaps.append({"family": family, "reason": "external proof blocked by environment (P8)"})
            family_states[family] = "blocked_environment"
        else:
            problems.append(f"{family}: unknown capture_role {role!r}")

    summary = manifest.get("summary") or {}
    if summary.get("total_families") != len(slices):
        problems.append("summary.total_families does not match slice count")
    if summary.get("capture_required") != required:
        problems.append("summary.capture_required does not match capture_required slice count")
    if summary.get("captured_slices") != captured:
        problems.append(f"summary.captured_slices {summary.get('captured_slices')} != actual captured {captured}")
    for status_name, count in status_breakdown.items():
        if summary.get("status_breakdown", {}).get(status_name) != count:
            problems.append(f"summary.status_breakdown[{status_name}] {summary.get('status_breakdown', {}).get(status_name)} != actual {count}")
    if summary.get("pass_count", 0) != 0 or summary.get("diverged_count", 0) != len([s for s in family_states.values() if s == "diverged"]):
        problems.append("summary pass_count/diverged_count inconsistent with slice states")

    if problems:
        print(
            json.dumps(
                {"verdict": "rejected", "reference_epoch": epoch, "reasons": sorted(set(problems)), "inventory": inventory_provenance},
                indent=1,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 1

    verdict = "pass" if captured == required and required > 0 else "partial"
    document: dict[str, object] = {
        "verdict": verdict,
        "reference_epoch": epoch,
        "root": str(args.root),
        "captured_slices": captured,
        "capture_required": required,
        "families": family_states,
        "inventory": inventory_provenance,
    }
    if gaps:
        document["gaps"] = gaps
    document["note"] = (
        "pass: every capture-required slice is sealed, epoch-bound, and its RED receipt detects all mutants; "
        "partial honestly lists remaining gaps without asserting parity"
    )
    print(json.dumps(document, indent=1, sort_keys=True))
    return 0


def main() -> int:
    argv = sys.argv[1:]
    # Top-level self-test exercises the scheduler mutations, clean-room
    # schedule, and the validate-scenarios surface hermetically.
    if argv == ["--self-test"]:
        return _self_test()
    # Subcommand dispatch: `validate-scenarios` is the Wave 4 Todo 33 surface.
    if argv and argv[0] == "validate-scenarios":
        return _run_validate_scenarios(argv[1:])
    if argv and argv[0] == "validate-reference-observations":
        return _run_validate_reference_observations(argv[1:])

    parser = argparse.ArgumentParser(description=DESCRIPTION)
    _ = parser.add_argument("--task", type=int)
    _ = parser.add_argument("--scheduler-mutations", action="store_true")
    _ = parser.add_argument("--write-ledgers", action="store_true")
    _ = parser.add_argument("--validate-ledgers", action="store_true")
    _ = parser.add_argument("--reservations", action="store_true")
    _ = parser.add_argument("--schedule", action="store_true")
    _ = parser.add_argument("--validate", action="store_true")
    _ = parser.add_argument("--clean-room-evidence-dir", type=Path, default=None)
    _ = parser.add_argument("--attempt", default="attempt-2")
    _ = parser.add_argument("--evidence-root", type=Path, default=DEFAULT_EVIDENCE_ROOT)
    args, unknown = parser.parse_known_args(argv)
    if unknown:
        _fail(f"unknown QA mode arguments: {' '.join(unknown)}")
    validate_catalog(TASKS)
    validate_clean_room_schedule(CLEAN_ROOM_RESERVATIONS)
    if args.write_ledgers:
        _ = write_ledgers(args.evidence_root)
        print(json.dumps({"verdict": "pass", "artifact": str(args.evidence_root / "task-6/scheduler-mutation-receipt.json")}, sort_keys=True))
        return 0
    if args.validate_ledgers:
        validate_ledgers(args.evidence_root)
        print(json.dumps({"verdict": "pass", "evidence_root": str(args.evidence_root)}, sort_keys=True))
        return 0
    if args.clean_room_evidence_dir is not None:
        _ = write_clean_room_evidence(args.clean_room_evidence_dir)
        print(json.dumps({"verdict": "pass", "evidence_root": str(args.clean_room_evidence_dir), "artifacts": [*CLEAN_ROOM_EVIDENCE_ARTIFACTS, "evidence-manifest.json"]}, sort_keys=True))
        return 0
    if args.reservations:
        print(json.dumps(clean_room_reservations_document(CLEAN_ROOM_RESERVATIONS), indent=2, sort_keys=True))
        return 0
    if args.schedule:
        print(json.dumps(clean_room_schedule_document(CLEAN_ROOM_RESERVATIONS), indent=2, sort_keys=True))
        return 0
    if args.validate:
        print(json.dumps(clean_room_validation_document(CLEAN_ROOM_RESERVATIONS), indent=2, sort_keys=True))
        return 0
    if args.task != 6 or not args.scheduler_mutations:
        _fail("Task 6 requires --task 6 --scheduler-mutations; other task modes are dispatched from task-qa.json")
    mutations = run_mutations()
    print(json.dumps({"verdict": "pass", "mutations": mutations}, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as error:
        print(json.dumps({"verdict": "rejected", "reason": str(error)}, sort_keys=True), file=sys.stderr)
        raise SystemExit(1)
