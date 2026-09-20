# Plan 017: Contain archive traversal and honor session inclusion

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness/src/lib.rs crates/harness/tests/cli_authority_matrix_cli_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE
- **Issue:** [#240](https://github.com/urbanbreach/agent-harness/issues/240)
- **Priority:** P1
- **Effort:** M
- **Risk:** MED
- **Depends on:** none
- **Category:** security
- **Audit finding:** 16 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The archive walker follows directory symlinks and silently ignores read_dir errors. wrap also archives the workspace before checking --with-sessions, so default session contents can enter an archive without opt-in. Decide membership before traversal and reject links instead of following them outside the selected root.

## Current state

`crates/harness/src/lib.rs:1181` — The workspace archive is built before the session-inclusion flag is handled.

```rust
            let _ = writeln!(io.stderr, "failed to create {}: {err}", parent.display());
            return 2;
        }
    }
    let archive = match build_session_tar(&workspace) {
        Ok(archive) => archive,
        Err(err) => {
            let _ = writeln!(io.stderr, "failed to build workspace archive: {err}");
            return 2;
        }
    };
    if let Err(err) = std::fs::write(&output, &archive) {
        let _ = writeln!(io.stderr, "failed to write {}: {err}", output.display());
        return 2;
    }
    if command.with_sessions {
        let _ = writeln!(io.stderr, "including session artifacts...");
    }
    let result = serde_json::json!({
        "status": "wrapped",
```

`crates/harness/src/lib.rs:1265` — The recursive walker follows filesystem type queries and flattens errors.

```rust
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let archive_path = if prefix.is_empty() {
            name_str.to_string()
        } else {
            format!("{prefix}/{name_str}")
        };

        if path.is_dir() {
            add_directory_to_tar(archive, &path, &archive_path)?;
        } else if path.is_file() {
            let data = std::fs::read(&path)
                .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(
```

## Conventions and exemplar

Keep raw session backup/export semantics; this plan controls archive membership and containment, not support-export redaction. Walk with symlink_metadata, reject symlinks explicitly and propagate directory-entry errors. Permissions remain policy checks, not an OS sandbox; do not claim protection against an adversarial concurrent filesystem swap.

`crates/harness/tests/cli_authority_matrix_cli_test.rs:13` — Use the existing in-process CliIo/CliDeps command fixture.

```rust
fn run_cli(args: &[&str], deps: CliDeps) -> (i32, String, String) {
    let args: Vec<&str> = std::iter::once("harness")
        .chain(args.iter().copied())
        .collect();
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let ExitOutcome { code, .. } = harness::run(args, &mut io, deps);
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness/src/lib.rs`
- `crates/harness/tests/cli_authority_matrix_cli_test.rs`

Administrative updates to `plans/017-contain-archive-traversal.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-017-contain-archive-traversal` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Test archive members and traversal boundaries

Extend the wrap CLI case to inspect tar member names and bytes. Exercise default exclusion, explicit --with-sessions inclusion, an in-workspace session-dir override, an external symlink and a cycle. Use Unix-gated link cases where required. Assert failure instead of silently skipping unreadable or unsafe input; preserve existing raw session backup coverage.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` → The default-session and symlink cases fail on the baseline.

### Step 2: Enforce membership in the shared walker

Make build_session_tar's shared recursion reject symlink entries with symlink_metadata and return read_dir/entry/read errors. At wrap dispatch, pass the effective session directory and determine its workspace-relative subtree before walking. Exclude that subtree unless --with-sessions is set, and always exclude the output archive itself when it resides under the root. Preserve ordinary files and stable member ordering.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` → No external link or cycle is traversed; default archives contain no selected session subtree.

### Step 3: Handle overrides explicitly and verify sibling callers

For a session-dir override outside the workspace, do not traverse it implicitly; an explicit request to include that external directory must fail with a clear unsupported-location error. Test the override path and rerunning wrap with an existing output archive. Run the existing authority matrix to protect backup/export callers of the shared tar routine.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` → All matrix tests pass; inclusion is controlled by the flag and effective directory, with no output archive recursively included.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Tar member assertions prove default session exclusion and opt-in inclusion for supported in-workspace paths.
- [x] Symlink/cycle inputs fail without archiving external bytes; directory-entry failures propagate.
- [x] Existing output archives are excluded and raw backup behavior still passes.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A change silently broadens --with-sessions to arbitrary external roots or treats a traversal error as success.
- A different public archive format is required to implement containment.

## Maintenance notes

Any new caller of the shared tar builder must specify its membership policy before traversal. Coordinate lib.rs edits with plan 025.

## Execution evidence — 2026-09-20

Implemented on `codex/plan-017-contain-archive-traversal` in the isolated
`/home/urbanbreach/Projects/agent-harness-issue-240` worktree, based on `06467bfe`.
The required drift comparison against `7f5a7ec6` was empty for both implementation
files. The original dirty checkout and its uncommitted audit index were preserved;
this branch adds only this plan's row to the committed index.

`wrap` resolves the effective session directory from the CLI override, configuration,
or default before traversal. It excludes that directory unless explicitly included
and rejects inclusion outside the workspace. Both `wrap` and raw `trace` exclude
their output archive, including on repeated invocations. Shared traversal rejects
symlink roots and entries (including cycles and dangling links), rejects unsupported
file types, propagates enumeration/metadata/read errors, and sorts member names.
Raw artifact bytes and the tar.gz format are preserved.

All Cargo build/test/lint commands below used
`CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target` to reuse build artifacts.

| Verification | Actual result |
|---|---|
| Baseline: `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` with new regressions, before implementation | 17 selected: 13 passed, 4 intentionally failed. Failures exposed default session leakage, external symlink traversal, accepted external inclusion, and unstable raw trace member ordering. |
| Final: `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` | 17 passed, 0 skipped. Member names and bytes checked for default/configured/relative/absolute session paths, both inclusion modes, relative/default/absolute output paths, reruns, and raw trace exports. Unix cases also cover symlink roots/files/directories, cycles, dangling links, unreadable files/directories, unsupported sockets, and preservation of existing output on failure. |
| `cargo check -p harness --locked --offline` | Exit 0. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0. |
| `git diff --check` | Exit 0. |
| `git status --short` | Only the two allowed Rust files and this plan/index changed in the executor worktree. |

Limits: verification ran as an unprivileged Linux user; Unix-specific cases are
platform-gated. Mid-iteration directory-entry errors were not artificially injected;
their propagation is explicit in the fallible collection, while directory-open and
file-read failures were exercised. This remains path-based containment, not protection
against adversarial concurrent filesystem swaps. No visual capture was needed for
archive membership and byte preservation; no live services or native signoff ran.
