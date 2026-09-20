# Plan 028: Run the canonical performance evidence lane in GitLab CI

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- .gitlab-ci.yml docs/testing/testing.md`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
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
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter -E 'test(test_lanes_exports_artifact_dir_for_performance_stage) \| test(test_lanes_runs_perf_artifact_freshness_gate)'` | Expected results are specified per step; final run passes with nonzero selection. |
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

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter -E 'test(test_lanes_exports_artifact_dir_for_performance_stage) | test(test_lanes_runs_perf_artifact_freshness_gate)'` → Existing lane wiring/freshness tests pass; the YAML has one canonical perf invocation.

### Step 2: Align the operator command with the same contract

In testing.md make the canonical perf runner the documented evidence-producing command. If retaining a direct nextest example, identify it as a narrower test invocation that does not perform freshness signoff. Keep budget ownership and expected artifact names accurate.

**Verify:** `git diff --check` → The documentation no longer presents the bypass invocation as equivalent performance evidence.

### Step 3: Validate generated stages and real performance evidence

Dry-run the lane into a new owned temporary directory to inspect release selection, HARNESS_PERF_ARTIFACT_DIR and perf_artifact_freshness stage. Then execute the canonical perf lane into another fresh directory using the command below; require passing receipts and a fresh provenance-matching large-session-surfaces.json. A dry run alone cannot finish this plan. Verify the GitLab job with CI lint if available and its actual job result when run.

**Verify:** `bash -c 'plan_perf_dir=$(mktemp -d /tmp/harness-plan-028-XXXXXX); scripts/test-lanes.sh perf --artifact-dir "$plan_perf_dir"'` → Canonical performance and freshness stages pass; the fresh artifact and JUnit report are collected.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter -E 'test(test_lanes_exports_artifact_dir_for_performance_stage) | test(test_lanes_runs_perf_artifact_freshness_gate)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] The GitLab job invokes the canonical perf lane with a job-specific artifact root and has python3 available.
- [x] A real canonical run passes the release performance tests and freshness validator.
- [x] CI always collects lane receipts, large-session evidence and the expected perf JUnit report.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

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
- Independent canonical and final integrated performance evidence is recorded below.

### Independent CI configuration follow-up

Read-only GitLab inspection found a one-hour project timeout and no runners accepting
untagged jobs. The previous perf job failed with `stuck_pending_no_matching_runners`.
The perf job now selects the available Docker runner, allows six hours for the measured
cold release build, and uses Python's child wait timeout to emit content-free progress
every minute while preserving the canonical command and exit status. This also avoids
GitLab's one-hour inactivity limit while stage output is captured. Benchmark thresholds,
release settings, the canonical runner, and its freshness validator are unchanged.

References: [job timeout](https://docs.gitlab.com/ci/yaml/#timeout) and
[job inactivity limit](https://docs.gitlab.com/ci/pipelines/settings/#set-a-limit-for-how-long-jobs-can-run).

The final YAML passes the project's `glab ci lint`. A runnable stdlib check at
`/tmp/agent-harness-open-issues/check-perf-ci-wrapper.py` verifies the exact canonical
arguments, zero/nonzero exit propagation, and repeated progress timeouts without
running benchmarks or sleeping. Independent review and release evidence pass as recorded below; no hosted pipeline
execution is claimed.

## Independent closeout — 2026-09-20

Independent agent `verify_ci_perf` verified the implementation and follow-up CI configuration: **PASS**.

- At `abeff5ce0e4761b596afacca2c2e3cc3faeb7efa`, the unchanged canonical `scripts/test-lanes.sh perf --artifact-dir /tmp/harness-verify-251-eight-jobs-58xfmfsq` completed with both stages PASS: release nextest and artifact freshness. Nextest run `d259f5b1-b510-47d9-852c-cd999bbd3c1f` passed all seven selected tests after compiling 202 release test binaries. The final uninterrupted build took 64m12s; earlier interrupted build attempts are not passing evidence.
- At final code revision `76050bfccd5ed0f13ccaad8b3d08ada119831d91`, four targets were freshly prebuilt in a separate owned release target. Native nextest binary-metadata reuse ran all seven selected performance cases: two CLI/script, four TUI and one core. This is final-code coverage, not a second full canonical-lane run.
- Each final JUnit report is fresh and has no selected-case failure, error or skip. The unchanged freshness validator passed on `/tmp/harness-verify-integrated-perf-vu1dpvc2/artifacts`; the timestamp, stage-directory provenance and 120-session/3,960-event corpus were checked. Final list/reopen/search measurements were 48/1/18 ms. No compiler or linker was active when either benchmark execution started.
- Release optimization, fat LTO, codegen units, test budgets and validator requirements are unchanged. The six-hour CI job allowance only permits the build and evidence stages to complete.
- Project GitLab CI lint accepted the exact configuration at `2802f58d`; four independent wrapper checks proved canonical argv, one child invocation, 60-second progress intervals and zero/nonzero exit propagation. The available runner tags match the job.
- The focused script commands explicitly use `--ignore-default-filter`, matching the accepted eleven-case run and including its freshness case.

Detailed receipts are retained in the task artifact directory. The [combined verification record](2026-09-20-issue-closeout.md) maps all issues, commits and reviewers. Local nextest was 0.9.143 while CI pins 0.9.98; a hosted pipeline run is not claimed. Formatting, whitespace, compilation and lint are covered by the integrated checks in that record.
