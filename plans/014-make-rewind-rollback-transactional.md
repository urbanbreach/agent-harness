# Plan 014: Preserve original bytes when a rewind operation fails

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-core/src/prompt_rewind.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE (2026-09-20)
- **Issue:** [#237](https://github.com/urbanbreach/agent-harness/issues/237)
- **Priority:** P1
- **Effort:** M
- **Risk:** HIGH
- **Depends on:** plan 006 (`plans/006-contain-workspace-restores.md`, [issue #229](https://github.com/urbanbreach/agent-harness/issues/229))
- **Category:** bug
- **Audit finding:** 13 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Rewind records a backup after writing each target and overwrites that backup when the same effective path occurs again. A partial write or a later failure can therefore leave changed bytes or roll back to an intermediate state. Preflight unique targets and replace each file atomically so failed rewinds restore the original workspace.

## Current state

`crates/harness-core/src/prompt_rewind.rs:163` — Backups are collected while operations are already being applied.

```rust
    let mut backups: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    let mut files_restored = 0usize;
    let mut files_unchanged = 0usize;

    for (entry, target) in file_snapshot.iter().zip(targets) {
        recheck_restore_target(workspace_root, &target)
            .map_err(|err| rollback_or_escalate(workspace_root, &backups, err.to_string()))?;
        let previous = if target.is_file() {
            match fs::read_to_string(&target) {
                Ok(content) => Some(content),
                Err(err) => {
                    return Err(rollback_or_escalate(
                        workspace_root,
                        &backups,
                        format!("read {}: {err}", target.display()),
                    ));
                }
            }
```

`crates/harness-core/src/prompt_rewind.rs:200` — A target is written before its original bytes enter the rollback map.

```rust
        }
        recheck_restore_target(workspace_root, &target)
            .map_err(|err| rollback_or_escalate(workspace_root, &backups, err.to_string()))?;
        if let Err(err) = fs::write(&target, &entry.content) {
            return Err(rollback_or_escalate(
                workspace_root,
                &backups,
                format!("write {}: {err}", target.display()),
            ));
        }
        backups.insert(target, previous);
        files_restored = files_restored.saturating_add(1);
    }

```

## Conventions and exemplar

Plan 006 already established effective-target containment. Preserve that resolver, preflight all paths and recheck containment before replacement. Preserve existing file permissions. This is an in-process failure atomicity repair, not a new crash-consistent multi-file transaction system. Core has tempfile only as a dev dependency; use std for production replacement.

`crates/harness-core/src/memory.rs:270` — Reuse the existing create_new, write_all, sync_all and same-directory rename pattern; match its error propagation without introducing a dependency.

```rust
fn write_file_atomically(
    temp_path: &Path,
    final_path: &Path,
    body: &[u8],
) -> Result<(), MemoryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|source| MemoryError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    restrict_file_permissions(temp_path).map_err(|source| MemoryError::Write {
        path: temp_path.display().to_string(),
        source,
    })?;
    file.write_all(body)
        .and_then(|_| file.sync_all())
        .map_err(|source| MemoryError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    drop(file);
    fs::rename(temp_path, final_path).map_err(|source| MemoryError::Replace {
        path: final_path.display().to_string(),
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(prompt_rewind::tests::)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-core --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-core/src/prompt_rewind.rs`

Administrative updates to `plans/014-make-rewind-rollback-transactional.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-014-make-rewind-rollback-transactional` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Reject ambiguous restore sets before mutation

Resolve the complete target set with the existing containment helper, reject duplicate effective paths including contained symlink aliases, and reject unsupported target types. Snapshot each unique original file and its permissions once before any write. Extend the existing atomic_prompt_rewind and preflight_all_paths fixtures; assert duplicate plans fail without touching workspace bytes.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(prompt_rewind::tests::)'` → The duplicate-target regression fails on the baseline and passes after preflight is added.

### Step 2: Replace targets and rollback files atomically

Write each replacement into a uniquely named create_new temporary file in the effective target's directory. Apply the intended existing mode, write/sync, recheck containment and rename only after success. Register original state before attempting mutation. On a later error restore originals by the same replacement routine, remove only files created by this operation, clean owned temporary files, and report rollback failures without hiding the original error.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(prompt_rewind::tests::)'` → The existing later-target failure case restores every original byte and leaves no owned temporary file.

### Step 3: Exercise failure during temporary output

Use the smallest private test-only injection seam around temporary writing to fail after a partial temporary write, then exercise a later-operation failure in the existing behavioral test. Check original bytes, modes on Unix, absence of newly created targets and unchanged rewind/event records. No general filesystem abstraction is needed.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(prompt_rewind::tests::)'` → All rewind tests pass, including partial temporary write and later-operation rollback.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(prompt_rewind::tests::)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Duplicate effective targets are rejected before mutation.
- [x] Injected partial-write and later-operation failures preserve original workspace bytes and existing modes.
- [x] Owned temporary files are cleaned and failed operations publish no successful rewind record.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A repair weakens plan 006 containment or follows an unresolved outside-workspace path.
- Rollback cannot safely distinguish pre-existing files from files owned by this operation.
- Cross-process crash atomicity or arbitrary directory restores are required; they exceed this plan.

## Maintenance notes

Keep permission preservation, target containment and rollback ownership checks together whenever restore behavior changes.

## Execution evidence — 2026-09-20

Implemented on `codex/plan-014-make-rewind-rollback-transactional` in the isolated
`/home/urbanbreach/Projects/agent-harness-plan-014` worktree, based on `06467bfe`.
The user's active branch and pre-existing changes were left untouched.
The required baseline drift command produced no diff; prerequisite plan 006 is
present in commit `8e3f9e17` and its containment helpers remain unchanged.

Preflight now rejects duplicate effective paths (including contained symlink
aliases), directories and special files, and captures original bytes and modes
before writing. Forward writes and rollback share a standard-library replacement
routine: exclusive same-directory temporary file, complete write, original mode,
sync, containment recheck, rename. Only completed replacements enter the undo
list; rollback removes operation-created files, restores originals in reverse
application order and continues after an individual failure. Original and rollback
errors are both retained, including temporary-file cleanup errors.

The existing preflight fixture now covers duplicate existing and absent targets,
normalized paths, contained file/directory symlink aliases and unsupported types.
The restore fixture checks successful mode preservation and unchanged-file counts.
A private, test-only write-failure queue exercises partial temporary writes at
each position in a four-file restore, including after multiple successful renames.
Assertions cover binary original bytes, executable/read-only Unix modes, absent
new targets, no owned temporary files, preserved unowned files and byte-identical
event records. A failed rollback write also proves that remaining files are
restored and both errors are reported without partially overwriting the failed
rollback target.

The build, lint and nextest commands below used
`CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target` to reuse the
existing build cache; source and plan changes remain isolated in the new worktree.

| Check | Actual result |
|---|---|
| `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-core/src/prompt_rewind.rs` before implementation | Exit 0; no drift. |
| `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(prompt_rewind::tests::)'` with the duplicate regression and unchanged production code | Expected exit 100: 7 passed, 1 failed because duplicate `ok.txt` was accepted and restored twice. |
| Same focused command after preflight | Exit 0: 8 passed, 874 skipped. |
| Same focused command after atomic replacement and failure injection | Exit 0: 8 passed, 874 skipped. |
| Same focused command after separating success/failure tests for lint | Exit 0: 9 passed, 874 skipped. |
| `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(sessions::rewind::tests::)'` | Exit 0: 6 passed, 288 skipped; existing CLI rewind and dry-run behavior remains compatible. |
| `cargo check -p harness-core --locked --offline` | Exit 0. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Final exit 0. The first run flagged the expanded test's cognitive complexity (25/20); splitting its success and failure cases resolved it without suppressing the lint. |
| `git diff --check` | Exit 0. |
| `git status --short` | Only `prompt_rewind.rs` and the two permitted plan records changed. |

Material limits: this is handled in-process file failure recovery on the tested
Linux platform, not a crash-consistent or cross-process filesystem transaction.
Existing containment rechecks are preserved. As before, newly created parent
directories may remain after a failed file restore; directory rollback, ownership,
ACLs and extended attributes are outside this plan. If the filesystem also refuses
rollback or cleanup, the error reports that failure rather than claiming success.
No dependencies, lockfile, durable event schema or session histories changed.
