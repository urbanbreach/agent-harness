# Plan 026: Make live and native lanes select their opt-in test binaries

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- scripts/test-lanes.sh crates/harness/tests/test_lanes_script_test.rs docs/testing/testing.md crates/harness-testkit/tests/README.live-proxy.md`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#249](https://github.com/urbanbreach/agent-harness/issues/249)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** plan 024 (`plans/024-preserve-evidence-during-test-lane-dry-runs.md`, [issue #247](https://github.com/urbanbreach/agent-harness/issues/247))
- **Category:** tests
- **Audit finding:** 25 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The default nextest filter excludes live and native binaries. Their explicit lanes do not override that filter, so ignored-test flags alone do not ensure the intended tests are selected. Override the default filter only in these opt-in commands and verify discovery without running live services.

## Current state

`.config/nextest.toml:1` — The default filter intentionally excludes live/native targets.

```toml
# Deterministic test-suite profiles for the harness workspace.
# T1-T3 are parallel by default; T4/T5 run through explicit lanes.

[profile.default]
retries = 0
fail-fast = false
test-threads = "num-cpus"
slow-timeout = { period = "2s", terminate-after = 10 }
default-filter = 'not test(/perf_/) and not binary(/(binary_smoke|pty_e2e|live_proxy_e2e|native_visual_e2e)/)'


[profile.ci]
inherits = "default"
```

`scripts/test-lanes.sh:800` — The opt-in commands retain that default exclusion.

```bash
run_signoff_live() {
  require_live_env signoff-live || return 0
  run_stage signoff-live live_proxy_preflight_requires_live_env "$repo_root" cargo nextest run -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact || true
  run_stage signoff-live live_proxy_prompt_signoff "$repo_root" cargo nextest run -p harness-testkit live_proxy_prompt_signoff -- --ignored --exact || true
  run_stage signoff-live live_proxy_e2e_tui_signoff "$repo_root" cargo nextest run -p harness-testkit live_proxy_e2e_tui_signoff -- --ignored --exact || true
}

run_signoff_native() {
  require_native_env signoff-native || return 0
  run_stage signoff-native native_visual_e2e_ignored "$repo_root" cargo nextest run -p harness-testkit --test native_visual_e2e --test-threads 1 -- --ignored || true
```

## Conventions and exemplar

Keep live/native environment prerequisites and default deterministic exclusions. Plan 024 preserves old evidence in dry runs and must remain intact. A list or dry-run result proves selection only; it is not live, native or visual signoff.

`scripts/test-lanes.sh:549` — The binary signoff lane already uses explicit nextest selection conventions.

```bash
  local binary_smoke_artifacts_dir
  binary_smoke_artifacts_dir="$(stage_dir_for signoff-binary harness_binary_smoke)/artifacts"
  mkdir -p "$binary_smoke_artifacts_dir"
  run_stage signoff-binary harness_binary_smoke "$repo_root" env HARNESS_BINARY_SMOKE=1 HARNESS_BINARY_SMOKE_ARTIFACT_DIR="$binary_smoke_artifacts_dir" cargo nextest run -p harness --test binary_smoke --ignore-default-filter -- --ignored --exact || true
}

run_p0_06_xterm_capture() {
  local mode_name="$1"
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

Nextest distinguishes its default filter from explicit filters: [Running tests](https://nexte.st/docs/running/).

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Native discovery | `cargo nextest list --profile ci --locked --offline -p harness-testkit --test native_visual_e2e --ignore-default-filter --run-ignored only --message-format oneline` | The native_visual_ghostty_smoke case is listed; no tests execute. |
| Shell syntax | `bash -n scripts/test-lanes.sh` | Exit 0. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `scripts/test-lanes.sh`
- `crates/harness/tests/test_lanes_script_test.rs`
- `docs/testing/testing.md`
- `crates/harness-testkit/tests/README.live-proxy.md`

Administrative updates to `plans/026-select-opt-in-test-lanes.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-026-select-opt-in-test-lanes` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Extend existing lane-command coverage

Use the test-lanes dry-run fixture with an isolated artifact directory to inspect each of the three live commands and the native command. Assert explicit target selection, --ignore-default-filter and --run-ignored only while preserving the environment gate. Keep existing no-credential dry-run behavior.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test` → The four generated commands lack the required filter override on the baseline.

### Step 2: Repair explicit lane commands and documented equivalents

In test-lanes.sh use --test live_proxy_e2e with an exact -E test(=name) selection for each named live wrapper, --ignore-default-filter and --run-ignored only. Use --test native_visual_e2e with the same override/ignored mode and existing serialism. Update both documentation files' equivalent commands to current nextest syntax. Do not remove the default profile filter or invent live evidence.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test` → The dry-run target passes and every explicit command selects its intended opt-in entry point.

### Step 3: Verify discovery without running signoff

Run the two nextest list commands below. The live binary must list the preflight, prompt and TUI wrappers, and the native binary must list its visual smoke case. Record this as selection evidence only; an operator's separately authorized environment remains necessary for actual signoff.

**Verify:** `cargo nextest list --profile ci --locked --offline -p harness-testkit --test live_proxy_e2e --ignore-default-filter --run-ignored only --message-format oneline` → The expected ignored cases are listed without contacting a provider or opening a native terminal.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Generated commands explicitly override the default filter and select ignored tests in the intended binary.
- [x] Both discovery commands list their expected cases; the default deterministic profile still excludes them.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- Discovery still lists zero cases; do not claim a signoff lane is fixed from its command string alone.
- The change would bypass prerequisite environment gates or trigger real live/native work during ordinary CI.

## Maintenance notes

When renaming opt-in binaries, update the lane, documentation and discovery expectation together.

## Execution evidence — 2026-09-20

- Reproduced all four generated commands missing the nextest default-filter override.
- Explicit live/native commands now select their binary, override the default filter and select ignored tests; live wrappers use exact names, native remains serial. Matching documentation is updated.
- One behavioral regression checks all four generated commands and missing-environment failure before execution.
- `cargo nextest run --profile ci --locked --offline -p harness --test test_lanes_script_test --ignore-default-filter`: 11 passed, none skipped. This includes the dry-run preservation regression.
- `cargo nextest list --profile ci --locked --offline -p harness-testkit --test live_proxy_e2e --test native_visual_e2e --ignore-default-filter --run-ignored only --message-format oneline`: listed all three live wrappers and `native_visual_ghostty_smoke`; discovery only, no live/native execution.
- `bash -n scripts/test-lanes.sh` and whitespace checks passed. Integrated compile/lint and independent review are recorded in the issue closeout.

## Independent closeout — 2026-09-20

Independent agent `verify_existing_core` verified issue #249: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
