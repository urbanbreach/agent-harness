# Plan 007: Preserve existing file permissions during atomic edits

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-tools/src/hashline_apply.rs crates/harness-tools/tests/hashline_apply_test.rs`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 6 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: None
- **Category**: bug
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/230

## Why this matters

The shared atomic edit writer replaces an existing file with a tempfile but never transfers its permissions. Editing an executable script can therefore remove its executable bits. Copy the existing target's permissions onto the replacement before it is persisted, preserving the existing default for newly created files.

## Current state

- [crates/harness-tools/src/hashline_apply.rs:584](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L584) — Shared atomic writer used by native edit/write paths.
- [crates/harness-tools/src/hashline_apply.rs:174](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L174) — A caller writes the edited source through that helper.
- [crates/harness-tools/src/ast_grep.rs:384](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/ast_grep.rs#L384) — AST replacement also uses the shared writer; do not patch each caller.
- [crates/harness-tools/tests/hashline_apply_test.rs:23](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/tests/hashline_apply_test.rs#L23) — Existing public edit behavior test to extend.

[crates/harness-tools/src/hashline_apply.rs:592](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L592):

```rust
    let mut temp = tempfile::Builder::new()
        .prefix(".hashline-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .tool_err("failed to create temp file")?;

    temp.write_all(content.as_bytes())
        .tool_err("failed to write temp file")?;
    temp.flush().tool_err("failed to flush temp file")?;
    temp.as_file()
        .sync_data()
        .tool_err("failed to sync temp file")?;

    temp.persist(path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            err.error
        ))
    })?;

    Ok(())
```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-tools/tests/hashline_apply_test.rs:60](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/tests/hashline_apply_test.rs#L60):

```rust

    let updated = fs::read_to_string(&file_path).unwrap_or_abort();
    assert_eq!(updated, expected_content);

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
```

Relevant design contract: [docs/permissions/permissions.md:39](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/docs/permissions/permissions.md#L39).

Native editing stays behind coordinator permission and lifecycle gates. Reuse tempfile and standard-library metadata/permissions APIs; no new crate or alternate writer. Preserve existing error propagation, atomic replacement and temp-file cleanup. This plan preserves file permissions, not ownership, ACLs, extended attributes, timestamps or cross-filesystem moves.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test` | Selected tests pass after the repair; selection must not be empty. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-tools/src/hashline_apply.rs`
- `crates/harness-tools/tests/hashline_apply_test.rs`

Administrative updates are limited to execution status/evidence in `plans/007-preserve-modes-on-atomic-edits.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-007-preserve-modes-on-atomic-edits`.
- Keep this repair in one logical change; if instructed to commit, use `fix(tools): retain file permissions during atomic edits`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Extend the public edit assertion

In hashline_apply_success_writes_file_and_emits_applied_event, set a non-default executable mode on the temporary fixture under cfg(unix) and assert the same mode after the edit. Retain all existing content, digest and event assertions. Reuse this test instead of adding a private-helper-only test.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test -E 'test(hashline_apply_success)'` → The mode assertion fails before step 2 while the existing content assertions show the edit itself succeeded.

### Step 2: Transfer existing permissions in write_atomic

Read target metadata before replacement. Treat NotFound as the existing new-file path; propagate any other metadata error with ToolResultExt rather than silently using defaults. Write/flush the temporary content, set the saved existing permissions on the temp file, then sync and persist it. Propagate a permissions failure before persistence so the original file remains unchanged. Use std::fs::Permissions and the existing error conventions; do not add chmod calls at individual edit callers.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test` → The public edit test and all existing hashline cases pass, including unchanged executable mode on Unix.

### Step 3: Verify new-file and other edit routes

Run the existing native edit routing target for new-file, delete and rename behavior. Inspect rg callers of write_atomic to confirm the single shared repair covers them without per-caller copies. Do not expand the test suite solely to repeat the same mode assertion through each delegate.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_workspace_edit_routing_test` → All existing routing cases pass; newly created files keep their existing intended default.

### Step 4: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

Add one Unix permission assertion to the existing successful public edit test. Keep the existing new-file routing coverage as the control for missing metadata. Use PermissionsExt only in Unix-gated test code to set and inspect the mode; the production operation should use the portable permissions object.

## Done criteria

All must hold:

- [x] Editing the existing executable fixture retains its permission bits and produces the correct content/digest/events.
- [x] New files still work and retain their intended default permissions.
- [x] Metadata and permission-copy errors occur before replacement; no caller-specific workarounds are added.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- The target is replaced before saved permissions have been applied successfully.
- The change requires preserving owner IDs or other metadata beyond std::fs::Permissions, or changes symlink/rename semantics.

## Maintenance notes

All atomic-edit callers benefit from this helper; future writers should reuse it. If metadata preservation beyond permissions becomes a requirement, specify that separately rather than silently extending this patch.

## Execution evidence — 2026-09-20

- Started at `8e3f9e17` on `codex/plan-007-preserve-modes-on-atomic-edits`.
  The drift check showed only prior artifact-redaction changes in the two scoped
  files. The atomic writer and edit/content assertions still matched the plan;
  all existing redaction, digest and event assertions were retained.
- Extended the existing public success test with Unix mode `0751`. Before the
  repair, content editing succeeded but the permission assertion observed `0600`.
- The shared writer now captures `std::fs::Permissions`, applies them after writing
  and flushing the tempfile, then syncs and persists it. Code inspection confirms
  metadata errors other than `NotFound` and permission-copy errors return before
  replacement; `NotFound` keeps the existing tempfile defaults. Failure injection
  and non-Unix execution were not performed.
- Caller inspection with `rg` confirmed hashline patches, full-file rewrites and
  AST replacements share this writer. Native write, exact edit, apply-patch and
  LSP edit routes reuse these paths; no caller-specific change was needed.

| Command | Result |
|---|---|
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test -E 'test(hashline_apply_success)'` before repair | Exit 100; 1 selected test failed at the mode assertion (`0600` versus `0751`), 3 skipped. |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test` | Exit 0; 4 passed, 0 skipped. |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_workspace_edit_routing_test` | Exit 0; 7 passed, 0 skipped, including creation, deletion and rename behavior. |
| `cargo check --workspace --locked --offline` | Exit 0. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0. |
| `git diff --check` | Exit 0. |

All required checks passed. The full workspace suite and historical repository-wide
gates mentioned above were not rerun and are not claimed green. Changed and
untracked paths were compared with the initial inventory; hashes confirm all
pre-existing out-of-scope files are unchanged. Only the two scoped Rust files,
this plan and the plan 007 index entry belong to the commit. The index's existing
uncommitted audit draft is preserved, with its matching plan 007 row also marked
DONE; unrelated audit content remains unstaged.
