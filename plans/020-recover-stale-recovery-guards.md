# Plan 020: Recover abandoned writer-recovery guards without racing another writer

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-core/src/store.rs crates/harness-core/src/store/tests.rs crates/harness-core/src/crash_recovery.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#243](https://github.com/urbanbreach/agent-harness/issues/243)
- **Priority:** P1
- **Effort:** M
- **Risk:** HIGH
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 19 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Writer acquisition can reclaim a dead writer lock, but first requires create_new on a recovery marker that is never reclaimed when its owner dies. A crash during recovery can permanently block later recovery. Serialize reclamation with a stable OS file lock so stale cleanup cannot delete a new claimant's guard.

## Current state

`crates/harness-core/src/store.rs:327` — Recovery-marker acquisition precedes stale writer cleanup.

```rust
            Ok((file, contents)) => Ok(Self {
                path,
                contents,
                _file: file,
            }),
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                let _recovery_guard = WriterLockRecoveryGuard::acquire(run_dir, &path)?;
                if stale_writer_lock(run_dir, &path) {
                    let _ = fs::remove_file(&path);
                    let (file, contents) = create_writer_lock(&path).map_err(|source| {
                        EventStoreError::AcquireWriterLock {
                            path: display_path(&path),
                            source,
                        }
                    })?;
```

`crates/harness-core/src/store.rs:360` — An existing marker makes acquisition fail even after its owner dies.

```rust
}

impl WriterLockRecoveryGuard {
    fn acquire(run_dir: &Path, writer_lock_path: &Path) -> Result<Self, EventStoreError> {
        let path = run_dir.join(WRITER_LOCK_RECOVERY_FILE_NAME);
        let (file, contents) =
            create_writer_lock(&path).map_err(|source| EventStoreError::AcquireWriterLock {
                path: display_path(writer_lock_path),
                source,
            })?;
        Ok(Self {
            path,
            contents,
            _file: file,
        })
    }
}
```

## Conventions and exemplar

Keep live/unknown owners conservative, preserve journal repair ordering and do not use PID existence alone as a race-free unlink protocol. Use std::fs::File::try_lock available in the current stable toolchain, with no unsafe code or new dependency. Recovery inspection remains side-effect-free.

`crates/harness-core/src/store/tests.rs:133` — Extend existing dead-PID writer and concurrent-acquisition behavior tests.

```rust
fn jsonl_open_recovers_dead_pid_writer_lock() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_stale_pid_lock");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\n").unwrap_or_abort();

    let store =
        JsonlFileEventStore::open(temp_dir.path(), "run_stale_pid_lock", false).unwrap_or_abort();

    assert!(store.file_path().exists());
    let lock_contents = fs::read_to_string(run_dir.join(".writer.lock")).unwrap_or_abort();
    assert!(lock_contents.contains(&format!("pid={}", std::process::id())));
}

#[cfg(target_os = "linux")]
#[test]
fn jsonl_open_serializes_concurrent_dead_pid_writer_lock_recovery() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_concurrent_stale_pid_lock";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\n").unwrap_or_abort();

    assert_single_concurrent_writer(temp_dir.path(), run_id);
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

Use the nonblocking standard-library file lock API: [File::try_lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests) \| test(crash_recovery::tests)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-core --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-core/src/store.rs`
- `crates/harness-core/src/store/tests.rs`
- `crates/harness-core/src/crash_recovery.rs`
- `crates/harness/src/sessions.rs` (necessary CLI recovery fixture correction, authorized by the integrating coordinator under the user's all-issues request)

Administrative updates to `plans/020-recover-stale-recovery-guards.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-020-recover-stale-recovery-guards` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Characterize stale and live recovery ownership

Extend store/tests.rs with a valid existing journal plus dead writer and recovery markers. Assert a new writer can recover it. Extend the existing concurrent writer test with synchronized claimants and both stale markers; exactly one may hold the writer while the winner remains open. Include a live recovery marker that must remain untouched, plus the public crash-recovery apply path. Revise the existing fixtures at `crates/harness-core/src/crash_recovery.rs:576` and `crates/harness-core/src/crash_recovery.rs:615` that use a live pid=1 marker yet expect deletion: use Linux-gated provably dead-owner cleanup coverage, explicit live/unknown-owner preservation, and platform-neutral tail-repair coverage without an unreclaimable marker. Changing the PID literal alone is insufficient on non-Linux systems, where process liveness remains conservative.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests) | test(crash_recovery::tests)'` → The abandoned-recovery-marker case fails on the baseline; live-owner exclusion remains intact.

### Step 2: Serialize marker lifecycle through one stable lock inode

Open/create a separate per-run .writer.lock.recovery-mutex file and acquire its nonblocking exclusive File lock before inspecting or creating the legacy recovery marker. Never unlink this mutex file; holding its inode stable is essential. Store the locked File in RecoveryGuard until its owned marker has been removed on drop. Under this lock, retain live/unknown owners, reclaim only a provably dead legacy owner, then create_new the recovery marker and run existing writer acquisition. Propagate unsupported/failed locking as an unavailable acquisition, never success.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests)'` → The concurrent stale-marker case admits one writer without removing another claimant's marker; public cleanup assertions are completed in Step 3.

### Step 3: Use the same exclusion in orphan cleanup

Route crash_recovery's orphan-marker cleanup through the same locked recovery helper and recheck ownership while the lock is held. Add the stable mutex file to the unborn-run artifact allowance; it is not itself evidence of a crash and read-only inspection must not create it. Preserve marker owner tokens, conservative non-Linux liveness behavior and existing tail-repair semantics.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests) | test(crash_recovery::tests)'` → The public recovery apply case succeeds for dead markers; live guards and unrelated files remain untouched.

### Step 4: Run recovery and exclusion checks

Run both unit selections, including the existing tail corruption and concurrent-writer cases. Use barriers rather than sleep to assert exclusion. Verify the stable mutex survives cleanup and can be locked again after its owner exits.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests) | test(crash_recovery::tests)'` → All selected tests pass; no process can unlink the inode used for synchronization.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests) | test(crash_recovery::tests)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] A valid journal with dead writer and recovery markers can be opened and recovered.
- [x] Concurrent reclamation admits exactly one live writer and preserves live guards.
- [x] Read-only inspection creates no lock artifacts and existing tail-repair tests pass.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The deployed Rust toolchain lacks std file locking or filesystem locking is unavailable; report that platform limitation instead of falling back to racy PID/unlink code.
- A required compatibility scenario includes concurrently running older binaries that never take the new mutex; establish the rollout contract first.
- The change requires silently reclaiming an owner whose liveness is unknown.

## Maintenance notes

The mutex pathname must never be deleted during cleanup; replacing its inode defeats exclusion. Keep all marker-mutating paths behind the same lock.


## Execution evidence (2026-09-20)

- Baseline was `3d8e3d4f`; the scoped files had no drift from the planned `7f5a7ec6` revision. Installed compiler is Rust `1.98.0`, with standard-library file locking available.
- Recovery now acquires a nonblocking lock on the stable `.writer.lock.recovery-mutex` inode before reading or reclaiming an abandoned recovery marker. The file remains on disk; its locked handle outlives owned-marker cleanup. Only a parsed dead PID is reclaimed; live, unknown, or unreadable recovery owners remain conservative.
- Orphan cleanup reuses the same guard, so it rechecks recovery ownership while holding the mutex. Failed acquisition leaves orphan markers visible in the recovery report. Writer recovery propagates acquisition errors. Read-only inspection does not create the mutex and does not consider it crash evidence; unborn-run inspection permits the stable mutex artifact.
- Extended existing dead-writer/concurrent-writer regressions with dead recovery markers, preserving valid journal sequence and barrier-synchronized exclusion. Added public-path checks for live/unknown ownership and a held mutex preventing both orphan cleanup and writer recovery; reopening the same mutex pathname proves cleanup did not replace its inode and dropping the owning handle releases its lock.
- Corrected the existing core and CLI fixtures that expected deletion of a live `pid=1` marker. Dead-PID cleanup cases are Linux-gated; tail repair and live/unknown ownership coverage remain platform-neutral. The CLI fixture was outside the handoff's original scope; its minimal correction was explicitly authorized by the integrating coordinator. Other live-marker fixtures only inspect markers and need no change.
- Baseline regression run: `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(store::tests) | test(crash_recovery::tests)'`: **33 passed, 3 expected failures**. The failures were abandoned-marker recovery, concurrent abandoned-marker recovery, and preserving a live orphan marker.
- Final same command after implementation and the mutex regression: **37 passed**, 849 skipped, including existing tail-repair and legacy-writer controls.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Cargo checks use `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.
- Concurrent marker mutation is serialized for binaries using this mutex protocol. No mixed-version concurrent-write rollout requirement or supported external reader was identified; no dependency was added. Non-Linux dead-owner reclamation stays conservative and was not runtime-tested on this Linux host.
- Independent review and the index update are handled by the integrating coordinator; no issue has been closed from this worktree.

- Per-worktree `cargo check -p harness-core --locked --offline`, `cargo check --workspace --locked --offline`, and `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` were queued on the shared Cargo lock, then canceled at the integrating coordinator's request. Workspace check/Clippy will run once on the integrated changes; these unrun checks are not claimed as passing here.

- `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(reopen_session_applies_crash_recovery_for_marker_then_summarizes)'`: **1 passed**, 293 skipped; the corrected CLI recovery fixture reports successful cleanup through the public reopen command.

## Independent closeout — 2026-09-20

Independent agent `verify_existing_core` verified issue #243: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
