# Plan 027: Write nextest JUnit reports where CI collects them

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- .config/nextest.toml crates/harness/tests/test_lanes_script_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#250](https://github.com/urbanbreach/agent-harness/issues/250)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** tests
- **Audit finding:** 26 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Nextest resolves the configured JUnit filename inside the selected profile's store directory. Repeating target/nextest/<profile> in that filename produces a nested path, while GitLab collects the normal location. Use a relative filename and verify actual fresh XML output.

## Current state

`.config/nextest.toml:14` — Both CI and perf profiles repeat their store-directory prefix.

```toml
failure-output = "immediate-final"
success-output = "never"
status-level = "slow"
final-status-level = "slow"
# GitLab collects this as a JUnit report.
junit = { path = "target/nextest/ci/junit.xml" }

[profile.perf]
inherits = "default"
default-filter = 'test(/perf_/)'
failure-output = "immediate-final"
status-level = "pass"
final-status-level = "none"
junit = { path = "target/nextest/perf/junit.xml" }

# This opt-in resource fixture also measures unoptimized baseline histories.
```

`.gitlab-ci.yml:35` — GitLab already expects the normal profile report locations.

```yaml
  when: always
  expire_in: 1 week
  reports:
    junit:
      - target/nextest/ci/junit.xml
  paths:
    - target/nextest/

.perf_junit_artifacts: &perf_junit_artifacts
  when: always
  expire_in: 1 week
  reports:
    junit:
      - target/nextest/perf/junit.xml
  paths:
    - target/nextest/perf/
```

## Conventions and exemplar

Set junit.path to junit.xml for both profiles. Preserve profile filters, concurrency and retries; CI artifact paths are already correct. Coordinate this test file with plans 024/026, but there is no functional prerequisite.

`crates/harness/tests/test_lanes_script_test.rs:38` — Update the existing profile-contract assertion that currently locks in the wrong path.

```rust

    // assert
    assert!(ci_profile_is_wired);
    assert!(nextest_config.contains("[profile.default]"));
    assert!(nextest_config.contains("[profile.ci]\ninherits = \"default\""));
    assert!(nextest_config.contains("junit = { path = \"target/nextest/ci/junit.xml\" }"));
}
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

Nextest's [configuration reference](https://nexte.st/docs/configuration/reference/) defines report paths relative to the profile store directory.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(test_lanes_ci_profile_is_defined_for_fast_and_integration)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Parse both reports | `python3 -c 'from pathlib import Path; import xml.etree.ElementTree as E; paths=[Path("target/nextest")/p/"junit.xml" for p in ("ci","perf")]; [E.parse(p) for p in paths]; print("Both JUnit reports parse")'` | Both reports parse; separately confirm their fresh run timestamps and selected testcase names. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `.config/nextest.toml`
- `crates/harness/tests/test_lanes_script_test.rs`

Administrative updates to `plans/027-align-nextest-junit-paths.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-027-align-nextest-junit-paths` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Correct both profile filenames and the existing assertion

Set each configured JUnit path to junit.xml and update test_lanes_ci_profile_is_defined_for_fast_and_integration to assert that contract for both ci and perf. Do not add a separate mirror-only test.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(test_lanes_ci_profile_is_defined_for_fast_and_integration)'` → The focused test passes and the CI run reports target/nextest/ci/junit.xml.

### Step 2: Generate an actual report under each profile

Run the focused command under ci, then under perf with --ignore-default-filter to select the same cheap existing case. Before running, note the start time; afterward parse each expected XML file and confirm it is fresh and contains that case. An old nested XML or a stale existing report is not sufficient.

**Verify:** `cargo nextest run --profile perf --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter -E 'test(test_lanes_ci_profile_is_defined_for_fast_and_integration)'` → Fresh XML at target/nextest/ci/junit.xml and target/nextest/perf/junit.xml contains the selected test.

### Step 3: Verify report collection compatibility

Run the full test-lanes script target and compare the generated report paths with the existing GitLab reports.junit entries. Do not relabel the cheap perf-profile invocation as performance benchmark evidence.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter` → All script cases pass; both CI artifact paths match actual outputs.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test -E 'test(test_lanes_ci_profile_is_defined_for_fast_and_integration)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Both profiles configure junit.xml without a repeated target prefix.
- [x] A fresh selected run under each profile produces parseable XML at the existing GitLab collection path.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The installed nextest version uses a different store-directory contract; confirm its actual output before changing CI paths.
- An unrelated test failure prevents a fresh report; record it separately rather than asserting an old file proves success.

## Maintenance notes

Keep profile filenames relative to the store directory when adjusting nextest profiles or CI report collection.

## Execution evidence — 2026-09-20

- Both profiles now use `junit.xml`, and the existing profile test checks each profile section.
- Ran `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter -E 'test(=test_lanes_ci_profile_is_defined_for_fast_and_integration)'`: 1 passed.
- Repeated that selected test under `--profile perf`: 1 passed (report-path verification only, not performance evidence).
- Before each run recorded a start timestamp; parsed the newly written `target/nextest/{ci,perf}/junit.xml`, confirmed modification after start and the selected testcase. Nextest's store remains workspace-relative even with CARGO_TARGET_DIR set.
- The unchanged GitLab report paths match both fresh XML files. Integrated compile/lint and independent review remain in the issue closeout.

## Independent closeout — 2026-09-20

Independent agent `verify_ci_perf` verified issue #250: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
