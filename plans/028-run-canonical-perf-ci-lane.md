# Plan 028: Run the canonical performance evidence lane in GitLab CI

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- .gitlab-ci.yml docs/testing/testing.md`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — canonical run and independent verification pending
- **Issue:** [#251](https://github.com/urbanbreach/agent-harness/issues/251)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** plan 027 (`plans/027-align-nextest-junit-paths.md`, [issue #250](https://github.com/urbanbreach/agent-harness/issues/250))
- **Category:** tests
- **Audit finding:** 27 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The GitLab perf job invokes nextest directly, bypassing the canonical runner's release build, artifact directory and freshness validator. A green job therefore does not establish the documented performance evidence contract. Route it through the existing perf lane and collect its receipts.

## Current state

`.gitlab-ci.yml:117` — The perf job currently bypasses the lane runner.

```yaml
rust:perf:
  stage: test
  image: rust:latest
  cache: *rust_cache
  before_script: *rust_nextest_before_script
  script:
    - cargo nextest run --profile perf --workspace --all-features
  artifacts: *perf_junit_artifacts
```

`scripts/test-lanes.sh:491` — The canonical lane wires release performance artifacts and the freshness gate.

```bash
run_perf() {
  local perf_artifacts_dir
  perf_artifacts_dir="$(stage_dir_for perf nextest_perf)/artifacts"
  mkdir -p "$perf_artifacts_dir"
  run_stage perf nextest_perf "$repo_root" env HARNESS_PERF_ARTIFACT_DIR="$perf_artifacts_dir" cargo nextest run --profile perf --release --workspace --all-features || true
  run_stage perf perf_artifact_freshness "$repo_root" python3 scripts/check-perf-artifacts.py --artifact-dir "$perf_artifacts_dir" || true
}

```

## Conventions and exemplar

Plan 027 establishes the JUnit location. Reuse scripts/test-lanes.sh perf unchanged; do not duplicate its commands in YAML, weaken budgets or add a second benchmark framework. Ensure python3 is available in the Rust CI image for the existing validator while retaining common Rust/nextest setup.

`docs/testing/testing.md:129` — Preserve the documented large-session artifact and freshness contract.

```markdown
The current budget owners are `crates/harness-core/tests/perf_test.rs`, which asserts the resume-plan
projection stays under its measured wall-clock budget for a fixed large event log, and
`crates/harness/tests/perf_sessions_surface_test.rs`, which writes `large-session-surfaces.json`
under the perf stage artifact directory. The large-session artifact records corpus size,
`sessions list`, `sessions reopen --json`, and `session_search` timings plus provenance.
After nextest, the lane runs `scripts/check-perf-artifacts.py` in a `perf_artifact_freshness`
stage so missing, stale, or provenance-mismatched perf artifacts fail closed.
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(test_lanes_exports_artifact_dir_for_performance_stage) \| test(test_lanes_runs_perf_artifact_freshness_gate)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `.gitlab-ci.yml`
- `docs/testing/testing.md`

Administrative updates to `plans/028-run-canonical-perf-ci-lane.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-028-run-canonical-perf-ci-lane` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Use the existing lane in the perf job

Replace the direct nextest invocation with scripts/test-lanes.sh perf --artifact-dir target/ci-perf/${CI_JOB_ID}. Ensure python3 in this job's setup while preserving the common before_script tasks. Retain JUnit artifacts and add target/ci-perf/ to always-collected artifact paths so failures keep their receipts. Use a job-specific fresh directory, not a reused evidence root.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(test_lanes_exports_artifact_dir_for_performance_stage) | test(test_lanes_runs_perf_artifact_freshness_gate)'` → Existing lane wiring/freshness tests pass; the YAML has one canonical perf invocation.

### Step 2: Align the operator command with the same contract

In testing.md make the canonical perf runner the documented evidence-producing command. If retaining a direct nextest example, identify it as a narrower test invocation that does not perform freshness signoff. Keep budget ownership and expected artifact names accurate.

**Verify:** `git diff --check` → The documentation no longer presents the bypass invocation as equivalent performance evidence.

### Step 3: Validate generated stages and real performance evidence

Dry-run the lane into a new owned temporary directory to inspect release selection, HARNESS_PERF_ARTIFACT_DIR and perf_artifact_freshness stage. Then execute the canonical perf lane into another fresh directory using the command below; require passing receipts and a fresh provenance-matching large-session-surfaces.json. A dry run alone cannot finish this plan. Verify the GitLab job with CI lint if available and its actual job result when run.

**Verify:** `bash -c 'plan_perf_dir=$(mktemp -d /tmp/harness-plan-028-XXXXXX); scripts/test-lanes.sh perf --artifact-dir "$plan_perf_dir"'` → Canonical performance and freshness stages pass; the fresh artifact and JUnit report are collected.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(test_lanes_exports_artifact_dir_for_performance_stage) | test(test_lanes_runs_perf_artifact_freshness_gate)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [ ] The GitLab job invokes the canonical perf lane with a job-specific artifact root and has python3 available.
- [ ] A real canonical run passes the release performance tests and freshness validator.
- [ ] CI always collects lane receipts, large-session evidence and the expected perf JUnit report.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [ ] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The canonical lane fails a real budget or freshness check; report the failure without raising budgets or bypassing the validator.
- The job image cannot supply python3 through its existing setup model; resolve the image requirement before marking CI ready.

## Maintenance notes

Keep CI as a caller of the canonical lane. New perf stages belong in that runner and should automatically be included by this job.

## Implementation evidence — 2026-09-20

- `rust:perf` now invokes the existing canonical lane with a CI_JOB_ID-specific root, retains shared Rust/nextest setup, and installs python3 for the validator.
- Its always-collected artifacts include the existing perf JUnit report and `target/ci-perf/` receipts.
- Operator documentation now names the canonical release/freshness contract.
- Parsed the GitLab YAML and confirmed setup alias resolution, the single canonical invocation, always collection and both artifact destinations.
- Existing performance wiring/freshness cases passed in the 11-case script target run used for plan026.
- A real canonical performance run and independent verification are still required and will be recorded before closure.
