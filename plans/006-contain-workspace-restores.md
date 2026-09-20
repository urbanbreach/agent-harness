# Plan 006: Validate restore targets before changing workspace files

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-core/src/path_selector.rs crates/harness-core/src/tool.rs crates/harness-core/src/coord/revert.rs crates/harness-core/src/prompt_rewind.rs crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs crates/harness-core/src/coord/tests/workspace_snapshot_secret_tests.rs`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 5 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: plan 004: Evaluate file permissions against effective workspace targets (`plans/004-align-file-permissions-with-targets.md`); [prerequisite issue](https://github.com/urbanbreach/agent-harness/issues/227)
- **Category**: security
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/229

## Why this matters

Coordinator revert uses a lexical fallback when a missing target cannot be canonicalized, while prompt rewind accepts absolute paths and does not resolve symlink ancestors. Both restore routes can reach outside the workspace. Validate the complete set of restore paths before mutation and reuse the effective-target resolution introduced for permission checks.

## Current state

- [crates/harness-core/src/coord/revert.rs:188](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/revert.rs#L188) — Missing-target canonicalization falls back to the unchecked lexical candidate.
- [crates/harness-core/src/coord/revert.rs:134](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/revert.rs#L134) — Current-workspace scanning reads files before applying restores.
- [crates/harness-core/src/prompt_rewind.rs:135](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/prompt_rewind.rs#L135) — Public atomic rewind combines conversation planning and file writes.
- [crates/harness/src/sessions/rewind.rs:141](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness/src/sessions/rewind.rs#L141) — CLI invokes the public restore boundary with a loaded snapshot.
- [crates/harness-tools/src/hashline_apply.rs:319](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L319) — Existing ancestor-containment behavior to preserve.
- [crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs:81](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs#L81) — Existing successful restore behavioral test.
- [crates/harness-core/src/coord/tests/workspace_snapshot_secret_tests.rs:43](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/tests/workspace_snapshot_secret_tests.rs#L43) — Protected files must remain protected.

[crates/harness-core/src/coord/revert.rs:188](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/revert.rs#L188):

```rust
fn resolve_within_workspace(workspace_root: &Path, relative: &str) -> Result<PathBuf, String> {
    let candidate = workspace_root.join(relative);
    let canonical = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.clone());
    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if !canonical.starts_with(&canonical_root) {
        return Err(format!("path escapes workspace root: {relative}"));
    }
    Ok(candidate)
}
```

[crates/harness-core/src/prompt_rewind.rs:155](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/prompt_rewind.rs#L155):

```rust
    for entry in file_snapshot {
        let relative = normalize_relative_path(&entry.path);
        if relative.is_empty() || relative.contains("..") {
            let err = format!("invalid snapshot path `{relative}`");
            return Err(rollback_or_escalate(workspace_root, &backups, err));
        }
        let target = workspace_root.join(&relative);
        let previous = if target.is_file() {
            match fs::read_to_string(&target) {
                Ok(content) => Some(content),
```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs:118](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs#L118):

```rust
    assert!(summary.failed_paths.is_empty());

    assert_eq!(
        fs::read_to_string(workspace.join("keep.txt")).unwrap_or_abort(),
        "keep-original"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("change.txt")).unwrap_or_abort(),
```

Relevant design contract: [crates/harness-core/src/prompt_rewind.rs:128](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/prompt_rewind.rs#L128).

atomic_prompt_rewind documents: “Plan conversation first; on failure return without touching files” and “Events stay append-only.” Preserve those contracts and the existing snapshot digest/content checks. Restore never grants external-directory access: its inputs must name workspace-relative files. Coordinator revert may still report partial failures for ordinary I/O errors; this plan changes invalid-path handling to fail before any file mutation. The separate duplicate-target/partial-write rollback defect in audit finding 13 is not part of this plan.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(revert_) \| test(snapshot_) \| test(atomic_prompt_rewind)'` | Selected tests pass after the repair; selection must not be empty. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-core/src/path_selector.rs`
- `crates/harness-core/src/tool.rs`
- `crates/harness-core/src/coord/revert.rs`
- `crates/harness-core/src/prompt_rewind.rs`
- `crates/harness-core/src/coord/tests/workspace_snapshot_tests.rs`
- `crates/harness-core/src/coord/tests/workspace_snapshot_secret_tests.rs`

Administrative updates are limited to execution status/evidence in `plans/006-contain-workspace-restores.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-006-contain-workspace-restores`.
- Keep this repair in one logical change; if instructed to commit, use `fix(recovery): contain restore targets before filesystem mutation`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Confirm the prerequisite resolver contract

Plan 004 must be complete. Verify its crate-internal effective-target resolver handles canonical roots, existing and absent leaves, dangling symlinks, and explicit invalid/external outcomes. Use that helper for restore with a strict containment decision; do not duplicate its ancestor traversal. If the helper's final name differs, record the name in this plan before implementation. Read the current revert and rewind code against the excerpts; changes made by the prerequisite are expected only in the shared helper.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_path) | test(path_selector)'` → The prerequisite behavior checks pass and the helper can reject external targets without invoking permission grants.

### Step 2: Preflight coordinator revert before reads or mutations

Reject absolute snapshot paths and actual ParentDir components before normalization; do not reject a harmless filename merely because it contains two dots. Canonicalize the workspace root without a lexical fallback. Resolve every eligible snapshot target and current-file removal target, rejecting external or unresolvable ancestors. In current_workspace_entries, validate each enumerated file before reading its content. If any path is invalid, emit the existing WorkspaceReverted failure result with empty restored/removed lists and leave files unchanged. Keep existing protected-entry filtering and digest checks. Pass the resolved targets to apply_restore and recheck containment immediately before write/removal; do not return the old unchecked candidate.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(revert_) | test(snapshot_)'` → Existing restore and protected-content tests pass; the extended restore case rejects an escaped missing target without reading or modifying the external fixture.

### Step 3: Apply the same preflight to prompt rewind

After successful conversation planning, validate every FileSnapshotEntry path into a resolved target before entering the mutation loop. Reject absolute, parent-traversal, dangling-link and external-target cases using structured AtomicPromptRewindError outcomes. Retain the existing append-only and rollback contracts, and use validated paths for backups. Do not take this opportunity to rewrite the duplicate-target/partial-write transaction logic from finding 13. Extend the existing invalid-path test into a table and assert the earlier legitimate target remains unchanged when a later path is rejected.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(atomic_prompt_rewind)'` → All rewind cases pass; invalid paths cause no workspace mutation and events.jsonl remains byte-for-byte unchanged.

### Step 4: Verify both public restore routes

Extend revert_restores_workspace_from_snapshot with a Unix fixture whose parent directory is replaced by a symlink to another temporary directory and whose destination leaf is absent. Retain a normal deleted-file restore control. Extend the rewind invalid-path table with absolute paths, missing leaves under symlinked parents, and an ordinary filename containing two dots. Use temp directories only and assert external sentinel bytes are unchanged.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(revert_) | test(snapshot_) | test(atomic_prompt_rewind)'` → All selected behavior checks pass, including protected-file exclusions and unchanged journal bytes.

### Step 5: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

Extend the existing restore and rewind tests rather than testing only a private resolver. Symlink cases are Unix-gated; absolute-path and component validation cases remain platform-neutral. Assert that all legitimate files and external sentinels remain unchanged when preflight fails. No destructive reproduction against the user's workspace is allowed.

## Done criteria

All must hold:

- [x] Both restore routes reject absolute/traversing/external targets and absent leaves behind external symlink ancestors.
- [x] All targets are validated before mutation; coordinator current-file scanning validates before reading content.
- [x] Normal deleted-file restoration and sensitive/binary snapshot protection still pass.
- [x] Rewind and replay do not rewrite or execute historical events.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(revert_) | test(snapshot_) | test(atomic_prompt_rewind)'` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- Plan 004 is incomplete or its helper silently falls back to lexical containment after an I/O error.
- A repair requires unsafe descriptor-relative APIs, a new sandbox, or a durable snapshot/event schema migration.
- The implementation relies on restoring redacted before-images, removes protected-file exclusions, or expands into finding 13's transaction rewrite.

## Maintenance notes

Filesystem links can change between validation and I/O; this repair closes the deterministic containment defects and narrows that window without claiming an OS sandbox. Any later transaction rewrite must retain preflight and use these resolved targets.

## Execution evidence (2026-09-20)

- Drift check at `a5769825`: only the expected prerequisite changes in
  `path_selector.rs` and `tool.rs`; restore implementation and tests match the plan.
- Plan 004 is complete at `8d8993e1`. Its helper is
  `crate::path_selector::effective_workspace_target`; `relative: None` identifies
  external targets, and missing leaves resolve through canonical existing ancestors.
  Dangling symlinks and resolution errors fail closed.
- `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E
  'test(permission_path) | test(path_selector)'`: exit 0, 8 passed.
- Existing local changes were recorded before implementation. Only this plan's
  dedicated index row is staged from the already-modified `plans/README.md`;
  the existing audit-index expansion remains unstaged, with its plan 006 row updated.


### Implementation and behavior

- Both restore routes use `resolve_restore_target`, a strict relative-path wrapper
  around plan 004's `effective_workspace_target`. Absolute paths, actual parent
  components, dangling links, external targets, and unresolvable ancestors fail
  before any restore or removal starts. Filenames containing two dots remain valid.
- Coordinator revert preflights snapshot targets, validates each enumerated current
  file before reading it, and records preflight failures in `WorkspaceReverted`
  with empty restored/removed lists. Existing ignored-file and digest protections remain.
- Writes, removals, and rewind rollback use the resolved targets and recheck them
  before I/O. Rewind plans the conversation before filesystem work and returns
  `AtomicPromptRewindError::FileRestore` on invalid input without entering rollback.
  The duplicate-target/partial-write transaction algorithm was not changed.
- Extended existing behavioral tests cover invalid snapshot tables, missing leaves
  behind a replaced symlink parent, and an unreadable external file enumerated by
  Git. Failed preflight preserves modified/deleted/added workspace files, external
  sentinel bytes, and rewind journal bytes; normal deleted-file restores, filenames
  containing two dots, ordinary write-error rollback, and protected files still pass.
- Before the fix, the two extended restore/rewind tests failed as expected (exit 100).
  The first lint pass found excessive nesting and test cognitive complexity; test
  scenarios were extracted without suppressions. An intermediate compile attempt
  found a missing test import. Final verification below is green.

### Final verification

- `cargo check --workspace --locked --offline`: exit 0.
- `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(revert_) | test(snapshot_) | test(atomic_prompt_rewind)'`:
  exit 0, 24 passed, 857 skipped (non-empty selection).
- `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_path) | test(path_selector) | test(replay_of_reverted_session)'`:
  exit 0, 9 passed, 872 skipped; includes side-effect-free replay and prerequisite coverage.
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings`:
  exit 0, no lint suppressions.
- `git diff --check`: exit 0.
- Scope review: four source/test files plus this execution plan and the plan 006
  index row. No dependency, lockfile, event schema, or startup-probe changes.
  Hash comparison confirms unrelated pre-existing configuration and plan files
  remain untouched; existing untracked user artifacts remain outside the commit.
- No baseline blocker affects these required checks. The full workspace suite and
  unrelated audit/branding gates were not rerun; their prior failures are not
  represented as fixed. Path checks narrow link races but do not provide an OS sandbox.
