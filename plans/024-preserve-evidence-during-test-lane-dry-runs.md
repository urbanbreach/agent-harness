# Plan 024: Preserve existing evidence during test-lane dry runs

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- scripts/test-lanes.sh crates/harness/tests/test_lanes_script_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#247](https://github.com/urbanbreach/agent-harness/issues/247)
- **Priority:** P1
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** dx
- **Audit finding:** 23 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Several PTY/signoff stages delete capture directories before run_stage checks dry-run mode. A dry run can therefore erase earlier evidence. Guard those destructive preparation steps while retaining the dry-run metadata the runner intentionally produces.

## Current state

`scripts/test-lanes.sh:298` — The dry-run guard is inside run_stage.

```bash
  local command_path="$stage_dir/command.txt"

  write_command_file "$command_path" "$@"

  if [[ "$dry_run" -eq 1 ]]; then
    printf 'dry-run: ' >"$stdout_path"
    write_quoted_command_line "$stdout_path" "$@"
    printf '[%s] %s command: ' "$mode_name" "$stage_name"
    print_quoted_command_line "$@"
    : >"$stderr_path"
    printf 'command_exit_code=0\ndry_run=true\n' >"$status_path"
    printf 'command_exit_code=0\ndry_run=true\ncommand_not_executed=true\n' >"$verification_path"
    record_stage_result "$mode_name" "$stage_name" DRY-RUN "$status_path" "$verification_path" command_not_executed
    return 0
```

`scripts/test-lanes.sh:557` — Capture cleanup occurs before that guard.

```bash
  local stage_name="$2"
  local cols="$3"
  local rows="$4"
  local destination="$(stage_dir_for "$mode_name" "$stage_name")/artifacts"
  local temporary_template="${TMPDIR:-/tmp}/harness-xterm-${timestamp}-${cols}x${rows}-XXXXXX"
  rm -rf "$destination"
  mkdir -p "$destination"
  run_stage "$mode_name" "$stage_name" "$repo_root" \
    bash -c 'set -euo pipefail
      temporary="$(mktemp -d "$1")"; destination="$2"; cols="$3"; rows="$4"; root="$5"
```

## Conventions and exemplar

Dry runs may continue to create their documented stage metadata/directories. They must preserve pre-existing captures byte-for-byte. Keep actual-run stale-evidence cleanup and fail-closed gates unchanged; do not build a new command wrapper.

`crates/harness/tests/test_lanes_script_test.rs:161` — Extend the existing signoff-PTY dry-run integration fixture.

```rust
fn signoff_pty_dry_run_emits_stage_artifact_and_fail_closed_contract() {
    // arrange
    let root = repo_root();
    let artifact_root = tempfile::tempdir().unwrap_or_abort();
    let script = root.join("scripts/test-lanes.sh");

    // act
    let output = std::process::Command::new("bash")
        .arg(&script)
        .arg("signoff-pty")
        .arg("--dry-run")
        .arg("--artifact-dir")
        .arg(artifact_root.path())
        .current_dir(&root)
        .output()
        .unwrap_or_abort();

    // assert
    assert!(output.status.success(), "lane failed: {output:?}");
    let stages = [
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(signoff_pty_dry_run_emits_stage_artifact_and_fail_closed_contract)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Shell syntax | `bash -n scripts/test-lanes.sh` | Exit 0. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `scripts/test-lanes.sh`
- `crates/harness/tests/test_lanes_script_test.rs`

Administrative updates to `plans/024-preserve-evidence-during-test-lane-dry-runs.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-024-preserve-evidence-during-test-lane-dry-runs` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Seed evidence in the existing dry-run test

Before invoking signoff-pty --dry-run, populate each of the four capture directories currently cleaned near lines 562, 582, 603 and 629 with unique sentinel files and a sibling artifact. Afterward compare their exact bytes while retaining the existing expected stage/receipt assertions.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(signoff_pty_dry_run_emits_stage_artifact_and_fail_closed_contract)'` → The new sentinel assertions fail on the baseline because cleanup removes prior captures.

### Step 2: Guard destructive preparation

At all four cleanup sites in test-lanes.sh, perform rm -rf only when dry_run is false, or place it in the actual stage execution branch. Preserve fresh-run cleanup, quoting, artifact paths and stage ordering. Keep metadata generation unchanged.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(signoff_pty_dry_run_emits_stage_artifact_and_fail_closed_contract)'` → Dry-run sentinels survive and all expected dry-run receipts still appear.

### Step 3: Run the script target and syntax check

Run the existing full test-lanes integration target and bash syntax validation. No PTY/native/live signoff is needed for this change.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test` → All selected checks pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(signoff_pty_dry_run_emits_stage_artifact_and_fail_closed_contract)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] The existing dry-run behavior test compares unchanged bytes in all four capture families.
- [x] Actual execution still performs stale-evidence cleanup; dry-run stage metadata remains valid.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- An intended dry-run operation relies on deleting old evidence; reconcile that contract instead of weakening the preservation assertion.
- The proposed change runs real gated lanes to simulate a dry run.

## Maintenance notes

Preparation outside run_stage must honor dry_run itself. Plans 026–028 share this script/test surface; preserve this regression.

## Execution evidence — 2026-09-20

- Reproduced the baseline loss with binary sentinel files in each of the four capture families.
- Guarded only destructive capture cleanup; actual-run cleanup and dry-run receipts are unchanged.
- Extended the existing signoff dry-run test with byte-for-byte nested-capture and sibling assertions.
- `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test`: 9 passed, 1 perf-named case excluded by the default filter.
- `bash -n scripts/test-lanes.sh` and `git diff --check`: passed.
- An additional direct script run preserved all four binary sentinels and sibling files.
- Cargo used the shared `target/issue-closure` directory with build jobs 2 and dev/test debug info disabled. Integrated checks and independent review are recorded in the issue closeout.

## Independent closeout — 2026-09-20

Independent agent `verify_existing_core` verified issue #247: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
