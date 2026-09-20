# Plan 018: Keep secret values out of QA scan diagnostics

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- scripts/harness-qa-live-smoke.sh scripts/harness-qa-dogfood.sh scripts/qa/security-hardening.test.mjs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** Implemented; awaiting independent verification
- **Issue:** [#241](https://github.com/urbanbreach/agent-harness/issues/241)
- **Priority:** P1
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** security
- **Audit finding:** 17 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

QA scripts copy complete matching lines into stderr and secret-scan receipts, creating additional copies of the sensitive values they detect. Some live failure paths print raw captured output before it has been scanned. Report only safe match metadata and fail closed on scanner errors.

## Current state

`scripts/harness-qa-live-smoke.sh:365` — Full grep matches are written to the scan receipt and stderr.

```bash
secret_hits="$(
  # shellcheck disable=SC2016
  grep -RInE 'sk-|Bearer |BEGIN PRIVATE KEY' "${evidence_dir}" 2>/dev/null || true
)"
if [[ -n "${secret_hits}" ]]; then
  printf 'FAIL\n%s\n' "${secret_hits}" >"${secret_scan_path}"
  printf 'Secret scan failed (fail-closed):\n%s\n' "${secret_hits}" >&2
  exit 1
fi
printf 'PASS\npatterns=sk-|Bearer |BEGIN PRIVATE KEY\n' >"${secret_scan_path}"

printf 'harness-qa live-smoke OK\nevidence_dir=%s\n' "${evidence_dir}"
```

`scripts/harness-qa-dogfood.sh:334` — The dogfood failure diagnostic includes the matched line.

```bash
    printf 'containers=0\n'
    printf 'temp_paths=0\n'
    printf 'qa_env=0\n'
  } >"${evidence_dir}/cleanup.txt"

  local secret_hits
  secret_hits="$(grep -RInE 'sk-|Bearer |BEGIN PRIVATE KEY' "${evidence_dir}" 2>/dev/null || true)"
  if [[ -n "${secret_hits}" ]]; then
    printf 'Secret scan failed (fail-closed):\n%s\n' "${secret_hits}" >&2
    return 1
  fi

  printf 'harness-qa dogfood OK\nevidence_dir=%s\n' "${evidence_dir}"
}

# Isolation: session-dir must stay under evidence or /tmp; never $HOME/.config/harness.
```

## Conventions and exemplar

Keep the existing fixed marker rules and fail-closed exit behavior. Matching only those fixed markers is enough; do not introduce a new scanner dependency. The scan's own receipt must not become input on a rerun. Raw diagnostic output from an unsuccessful command must never be copied to stderr before safe handling.

`scripts/qa/security-hardening.test.mjs:23` — Use the existing temporary workspace test pattern and Node's built-in test runner.

```javascript
test("validateEvidenceDir rejects destructive and escaping paths", async () => {
  // Given: a repository boundary and destructive or escaping path inputs.
  const repoRoot = await mkdtemp(join(tmpdir(), "harness-security-repo-"));
  await mkdir(join(repoRoot, ".omo", "evidence"), { recursive: true });
  const invalid = ["/", homedir(), repoRoot, ".", join(repoRoot, ".omo", "evidence"),
    join(repoRoot, ".omo", "evidence", "..", "outside"), join(tmpdir(), "ordinary-evidence")];
  try {
    // When/Then: every protected, traversal, or non-dedicated path is rejected.
    for (const path of invalid) {
      await assert.rejects(validateEvidenceDir(path, repoRoot), /unsafe evidence directory/);
    }
  } finally {
    await rm(repoRoot, { recursive: true, force: true });
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `node --test scripts/qa/security-hardening.test.mjs` | Expected results are specified per step; final run passes with nonzero selection. |
| Shell syntax | `bash -n scripts/harness-qa-live-smoke.sh scripts/harness-qa-dogfood.sh` | Exit 0. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `scripts/harness-qa-live-smoke.sh`
- `scripts/harness-qa-dogfood.sh`
- `scripts/qa/security-hardening.test.mjs`

Administrative updates to `plans/018-keep-qa-secret-scans-secret-free.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-018-keep-qa-secret-scans-secret-free` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add one table-driven diagnostic regression

Extend security-hardening.test.mjs using temporary evidence and fake local command fixtures. Generate synthetic marker-plus-suffix values at runtime, execute both script scan/failure paths, and assert failure with no complete synthetic value in stdout, stderr or scan receipts. Include a clean rerun with an existing receipt and a scanner error.

**Verify:** `node --test scripts/qa/security-hardening.test.mjs` → The leaking-output rows fail on the baseline; tests use no live credentials or network.

### Step 2: Emit safe scan metadata in both scripts

Change all full-line grep captures to fixed match-only markers with filename/line metadata or a count. Preserve grep status distinctions: 0 means findings, 1 means clean, and greater than 1 means scan failure. Remove the blanket || true handling that hides scanner errors. Exclude the owned receipt from scanned evidence. Update both normal scans and live-command failure handling so unscanned raw run_output is not printed.

**Verify:** `node --test scripts/qa/security-hardening.test.mjs` → Synthetic values never appear in derived diagnostics; scanner errors return nonzero and no PASS receipt.

### Step 3: Verify syntax and failure receipts

Run the Node regression and bash syntax checks for both scripts. Keep the original raw evidence policy unchanged; this issue eliminates republishing values into scan output, rather than claiming all original command captures are redacted.

**Verify:** `node --test scripts/qa/security-hardening.test.mjs` → All checks pass, including the clean rerun and scanner-error rows.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `node --test scripts/qa/security-hardening.test.mjs` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] The diagnostic regression finds no complete synthetic secret in stdout, stderr or scan receipts.
- [x] Findings and scanner failures both fail closed; a clean rerun is not contaminated by its old receipt.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row (index consolidation is owned by the coordinating agent). Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A marker rule can match an entire credential rather than a fixed prefix; emit its rule identifier instead of that match.
- The fix requires real credentials or a live provider run to verify.

## Maintenance notes

Every new QA error path must avoid printing captured output before scanning. Keep secret-finding and scanner-failure statuses distinct.

## Execution evidence — issue #241

- Baseline drift check: no differences in the three scoped implementation files from the planning baseline to `3d8e3d4f`.
- Added one table-driven command-boundary regression with nine isolated cases: both scripts' marker findings, command failures, scanner errors, clean reruns with a prior receipt, and the live optional-tool failure path. All commands are local fakes; no provider or credentials are used.
- Baseline `node --test scripts/qa/security-hardening.test.mjs`: failed as intended in all nine new cases (12 pre-existing checks passed). Diagnostics from the regression do not reproduce the synthetic values.
- Final `node --test scripts/qa/security-hardening.test.mjs`: 22 checks passed, 0 failed, 0 skipped.
- `bash -n scripts/harness-qa-live-smoke.sh scripts/harness-qa-dogfood.sh`: exit 0.
- `git diff --check`: exit 0.
- Scope: only the two scripts, their existing Node regression suite, and this plan were committed. The coordinating agent owns the shared plan index.
- The original command captures remain original evidence. Derived diagnostics and the owned scan receipt report only fixed status metadata and match counts; grep errors fail closed. Both normal dogfood paths reuse the same scanner, and unsuccessful smoke/tool captures are never echoed raw to stderr.
- Independent verification and issue closure remain the coordinating agent's final gates.
