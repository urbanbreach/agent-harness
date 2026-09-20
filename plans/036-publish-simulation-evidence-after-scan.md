# Plan 036: Publish simulation evidence only after validation and secret scanning

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-testkit/src/bin/simulation_evidence.rs crates/harness-testkit/tests/simulation_evidence_recorded.rs docs/testing/testing.md`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** Implemented; awaiting integrated checks and independent verification
- **Issue:** [#259](https://github.com/urbanbreach/agent-harness/issues/259)
- **Priority:** P1
- **Effort:** M
- **Risk:** MED
- **Depends on:** none
- **Category:** security
- **Audit finding:** 35 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The simulation evidence command copies raw inputs into the final artifact root and writes placeholder success metadata before scanning. A rejected run can leave sensitive or success-shaped evidence at its published destination. Build the complete bundle in an owned sibling staging directory and publish it only after every gate passes.

## Current state

`crates/harness-testkit/src/bin/simulation_evidence.rs:28` — The final artifact root is created and populated before validation finishes.

```rust
    fs::create_dir_all(&args.artifact_root).map_err(|err| {
        format!(
            "failed to create artifact root {}: {err}",
            args.artifact_root.display()
        )
    })?;

    copy_file(
        &args.matrix,
        &args.artifact_root.join("simulation-matrix.json"),
    )?;
```

`crates/harness-testkit/src/bin/simulation_evidence.rs:195` — The final scan and schema gates happen only after output has been written.

```rust
    let final_redaction_summary = scan_simulation_artifact_root(&args.artifact_root)
        .map_err(|failure| failure.to_string())?;
    if final_redaction_summary.secret_finding_count != 0 {
        return Err(format!(
            "secret-scan failed: rejected_artifacts={:?}",
            final_redaction_summary.rejected_artifacts
        ));
    }

    validate_simulation_events_file(&matrix, &args.artifact_root.join("simulation-events.jsonl"))
        .map_err(format_failures)?;
    validate_artifact_index_file(
        &matrix,
        &args.artifact_root,
        &args.artifact_root.join("artifact-index.jsonl"),
    )
    .map_err(format_failures)?;
    validate_report_file(&matrix, &args.artifact_root.join("simulation-report.json"))
        .map_err(format_failures)?;
```

## Conventions and exemplar

Use tempfile, already a production dependency of harness-testkit. Stage on the same filesystem as the destination for rename publication. Preserve artifact names, relative references and all current secret/schema/determinism/invariant gates. Never overwrite a nonempty existing evidence directory; preserve prior evidence on failure. No general transaction library or backup tree.

`crates/harness-testkit/tests/support/simulation_validator.rs:9` — Reuse the existing valid matrix fixture and schema constants.

```rust
pub fn valid_matrix() -> Value {
    json!({
        "schema_version": MATRIX_SCHEMA_VERSION,
        "invariants": [
            {"invariant_id": "INV-001", "description": "event vocabulary", "behavioral": true},
            {"invariant_id": "INV-002", "description": "tool lifecycle", "behavioral": true},
            {"invariant_id": "INV-003", "description": "replay projection", "behavioral": true},
            {"invariant_id": "INV-004", "description": "redaction and stability", "behavioral": true}
        ],
        "scenarios": [{
            "scenario_id": "golden_path",
            "description": "valid matrix fixture",
            "determinism_class": "offline-deterministic",
            "invariant_ids": ["INV-001", "INV-002", "INV-003", "INV-004"],
            "owner_tests_or_lanes": ["scripts/test-lanes.sh simulation"],
            "replay_command": "harness replay --session <simulation-run-dir> --json",
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-testkit --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-testkit --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-testkit/src/bin/simulation_evidence.rs`
- `crates/harness-testkit/tests/simulation_evidence_recorded.rs` (create)
- `docs/testing/testing.md`

Administrative updates to `plans/036-publish-simulation-evidence-after-scan.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-036-publish-simulation-evidence-after-scan` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add one command-boundary rejection fixture

Create simulation_evidence_recorded.rs using existing valid_matrix/support fixtures and the built simulation_evidence binary. Supply synthetic secret-bearing input and assert a nonzero result with no published artifact directory and no full synthetic value in diagnostics. Repeat with a pre-existing sentinel destination: it must remain unchanged. Include a valid successful bundle control.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test` → Rejected-input rows expose published files on the baseline; fixtures require no real credentials or live provider.

### Step 2: Generate and validate inside an owned sibling TempDir

Refactor run() to direct all copies, generated summaries and interim status files into a TempDir beside the intended artifact root. Keep bundle paths relative so staging names never enter published references. Run the existing first/final scans, schema validators, same-seed comparison and invariants against staging; any failure drops only this owned directory.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test` → A failed gate leaves no new final evidence and cleans the staging directory without touching prior output.

### Step 3: Publish the complete validated bundle once

After all gates pass, install staging by same-filesystem directory rename into an absent destination, or an existing empty directory if the platform supports atomic replacement. Refuse a nonempty destination before generation and recheck safely at publication; never remove prior evidence to make rename succeed. Print PASS only after publication succeeds. Document this destination policy where operators invoke the lane.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test` → The valid control produces the complete validated bundle; failure and pre-existing sentinel cases preserve destination bytes.

### Step 4: Run validator compatibility and the deterministic lane

Run the new command-boundary target plus existing simulation validator and secret scanner tests. Then run the canonical simulation lane with a fresh artifact root and verify all receipts and relative artifact references. If its caller passes a pre-populated subdirectory, preserve the caller's contract by staging only the bundle-owned destination; do not delete caller receipts.

**Verify:** `bash -c 'plan_sim_dir=$(mktemp -d /tmp/harness-plan-036-XXXXXX); scripts/test-lanes.sh simulation --artifact-dir "$plan_sim_dir"'` → All selected tests and the canonical offline lane pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Secret/schema/invariant failures publish no new bundle and preserve all pre-existing destination files.
- [x] Successful output becomes visible only as a fully validated bundle; PASS appears only after installation.
- [ ] The existing deterministic simulation lane still produces valid receipts and relative artifact references.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The canonical caller relies on merging files into a nonempty bundle destination; report that exact layout before changing publication semantics.
- Staging and destination cannot share a filesystem or the proposed implementation deletes prior evidence to emulate atomic rename.
- A test requires a real secret or republishes synthetic secret contents in diagnostics.

## Maintenance notes

All future artifacts and validators must target staging. Keep publication as the final filesystem operation and never treat placeholder scan summaries as publishable evidence.

## Execution evidence — issue #259

- Baseline drift check: no changes to the scoped implementation files between the planning baseline and `3d8e3d4f`.
- The canonical caller uses a dedicated `simulation/stages/simulation_evidence/artifacts` bundle directory; lane command/status receipts live in its parent. Staging only this bundle preserves the caller's layout.
- Implemented private sibling `TempDir` generation, destination checks before generation and publication, and final same-filesystem directory rename. Existing nonempty destinations and symlinks are refused; an empty directory is replaced only where the platform's rename supports it. No pre-existing output is removed.
- All existing scan, schema, determinism, and invariant gates still execute against staged artifacts. The staged matrix is scanned before matrix validation because its validation diagnostics may include input values.
- Added one command-boundary table covering secret-bearing raw events, secret-bearing invalid matrix metadata (literal and JSON-escaped), malformed JSON, schema failures, invariant failures, same-seed mismatch, and valid inputs across absent, empty, and populated destinations. It verifies cleanup, prior bytes, safe diagnostics, the complete artifact set, and relative index references.
- Baseline focused nextest command: 28 existing tests passed; the new regression failed as intended with `secret published rejected data`. A separate local binary reproducer confirmed matrix validation echoed a runtime-generated synthetic secret (reported only as a boolean).
- Initial focused nextest run including all 21 original table cases and the matrix-secret rejection: 29 tests passed, 0 failed, 0 skipped (run ID `a4aca91a-6761-49c9-97ef-1c1029bc24e7`). The earlier staging-only run also passed 29 tests.
- `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0.
- Additional `python3 scripts/check-test-suite-gates.py`: two pre-existing file-focus failures (`crates/harness-core/tests/coord/17_tool_task_lifecycle_events_preserve_owner_test.rs`, 1355 > 800 lines; `crates/harness-tui/tests/tool_order_capture_test.rs`, 823 > 800). No added-file gate failure.
- Cargo environment for focused runs: `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.
- Exact focused command: `cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test`.
- The coordinating agent owns integrated `cargo check`, Clippy, and the canonical `scripts/test-lanes.sh simulation` run, plus plan-index consolidation and independent verification. Those gates are pending and are not claimed as passed here; this avoids redundant broad builds queued on the shared Cargo target.


### Independent-review correction: decoded validation diagnostics

- The independent reviewer reproduced a credential leak when invalid matrix metadata encoded its first marker character as a JSON Unicode escape. Raw scanning did not recognize the encoded marker; matrix validation decoded it and the shared failure formatter printed the value.
- The binary's shared `format_failures` boundary now emits only the validation-failure count. Matrix, event, artifact-index, report, and same-seed comparison failures retain their rejection behavior without printing decoded paths, expected/observed values, or identifiers from input.
- Extended the existing table with escaped invalid matrix metadata across all three destination states (24 table cases total). Assertions report only booleans when checking synthetic values.
- Fresh private build baseline: `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/fix-evidence-private CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo nextest run --profile ci --locked --offline -p harness-testkit --test simulation_evidence_recorded --test simulation_validator_test --test secretscan_test` failed as intended: 28 passed, 1 failed with `stderr exposed a secret` (run `9b392254-fa92-43b4-9c0b-4df2daface94`). No shared workspace artifacts were reused.
- The same private-target command after the correction passed: 29 tests, 0 failed, 0 skipped (run `48a44648-57b4-4830-9f5f-40aee761a44e`).
- `cargo fmt --all -- --check` and `git diff --check`: exit 0.
- Follow-up independent review and integration remain owned by the coordinating agent; no issue was closed or pushed from this executor checkout.
