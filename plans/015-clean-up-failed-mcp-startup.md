# Plan 015: Terminate MCP processes when initialization fails or is cancelled

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tools/src/mcp_session.rs crates/harness-tools/tests/integrations_matrix_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE
- **Issue:** [#238](https://github.com/urbanbreach/agent-harness/issues/238)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 14 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

A stdio MCP process is spawned before initialization completes. Initialization error or cancellation drops the session without a kill-on-drop policy, leaving the child alive. Bind that process lifetime to its session and reap it on completed startup failures.

## Current state

`crates/harness-tools/src/mcp_session.rs:145` — The command is spawned without kill_on_drop.

```rust
        if command.is_empty() {
            return Err(ToolError::Execution(format!(
                "MCP server `{server_id}` has empty stdio command"
            )));
        }

        let mut process = Command::new(&command[0]);
        process
            .args(command.iter().skip(1))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }
        if !env.is_empty() {
            process.envs(env.iter());
        }

        let mut child = process.spawn().map_err(|err| {
```

`crates/harness-tools/src/mcp_session.rs:209` — Initialization happens after the process has been created.

```rust
    ) -> Result<Self, ToolError> {
        let process = starter.start(server_id, command, env, cwd)?;
        let timeout = Duration::from_secs(timeout_secs.max(1));
        let mut session = Self {
            child: process.child,
            stdin: process.stdin,
            stdout: BufReader::new(process.stdout),
            next_id: 1,
            timeout,
            metadata: McpSessionMetadata::default(),
        };
        let initialize = session
            .request(
                "initialize",
                json!({
```

## Conventions and exemplar

Keep successful MCP session reuse and existing timeout/error categories. Use Tokio process ownership already present. No daemon manager, new crate, or changes to discovered tool names.

`crates/harness-tools/src/shell_run.rs:483` — The shell tool already enables kill_on_drop for its child.

```rust
) -> Result<ShellProcessOutput, ToolError> {
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_shell_process_group(&mut command);
    let child = command.spawn().tool_err("failed to execute command")?;
    await_child_output(child, timeout_ms).await
}

fn configure_shell_process_group(command: &mut tokio::process::Command) {
    #[cfg(unix)]
    command.process_group(0);
}

```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tools --test integrations_matrix_test -E 'test(mcp_)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tools --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| MCP compatibility | `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` | All selected cases pass. |
| Generic MCP registry | `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` | All selected cases pass. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tools/src/mcp_session.rs`
- `crates/harness-tools/tests/integrations_matrix_test.rs`

Administrative updates to `plans/015-clean-up-failed-mcp-startup.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-015-clean-up-failed-mcp-startup` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add a process-lifetime regression

Extend integrations_matrix_test.rs with one controlled stdio fixture that reports its PID/readiness and then either rejects initialization or remains pending until the caller is cancelled. Wait for readiness using a handshake, then assert the process exits within a bounded timeout. Include a successfully initialized session as the reuse control; do not accept tools.is_empty() alone as cleanup evidence.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test integrations_matrix_test -E 'test(mcp_)'` → Failure/cancellation rows expose a surviving child on the baseline.

### Step 2: Make startup own and clean up its child

Set kill_on_drop(true) on the Command before spawn. On a completed initialization error, explicitly kill and wait for the child using the existing bounded request/cleanup policy, returning the original initialization error. Dropping a pending startup future must drop the owned child and trigger termination even before the session reaches the cache.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test integrations_matrix_test -E 'test(mcp_)'` → Failure and cancellation release the child; a healthy initialized session remains usable.

### Step 3: Run MCP compatibility checks

Run the focused integration selection and MCP library/generic integration coverage. Preserve stdout protocol handling and avoid logging raw response payloads in cleanup diagnostics.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test integrations_matrix_test -E 'test(mcp_)'` → All selected MCP tests pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tools --test integrations_matrix_test -E 'test(mcp_)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Both a rejected initialize request and cancellation of pending initialize terminate the spawned process.
- [x] Normal session reuse remains covered and passes.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The test depends on an installed external MCP service rather than a controlled local child.
- Cleanup would hide the startup error or indefinitely wait for a child that ignores termination.

## Maintenance notes

Keep kill-on-drop enabled before every stdio MCP spawn. Plan 021 relies on this lifetime rule when oversized transport data aborts a request.

## Execution record — 2026-09-20

Implemented [issue #238](https://github.com/urbanbreach/agent-harness/issues/238)
on `codex/plan-015-clean-up-failed-mcp-startup` in the isolated checkout
`/home/urbanbreach/Projects/agent-harness-plan-015`, based on `06467bfe`.
The required drift check against `7f5a7ec6` found no changes in either scoped
implementation file. The original dirty checkout was preserved.

Stdio commands now enable kill-on-drop before spawning. Errors from either the
initialize request or initialized notification trigger bounded `Child::kill`,
which also waits for/reaps the child, before returning the original startup error.
Cancelled startup futures retain ownership of the child until drop.

Extended the existing cancellation/restart integration case into one table-driven
process-lifetime regression. A controlled Python child reports its PID over a
Unix socket and acknowledges readiness before rejection, cancellation, or healthy
operation. Rejection checks the exact original error and that the child has been
reaped before return. Both failure paths check socket closure and process removal;
the healthy control returns successive call counts from the same session. The
fixture releases any surviving baseline child before reporting failure.

The pre-fix focused run selected six tests: five passed and the new regression
failed with `MCP startup left children alive: ['r', 'c']`, reproducing both leaks.
During implementation, a shadowed timeout name, a cancellation-handshake race,
and test-loop lint findings were corrected before the final checks below.

All Cargo build/test/lint commands used
`CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target` to reuse the local
build cache. No dependencies or lockfile changed.

| Final command | Result |
|---|---|
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test integrations_matrix_test -E 'test(mcp_)'` | PASS: 6 selected, 6 passed; 7 unrelated tests skipped. |
| `cargo check -p harness-tools --locked --offline` | PASS, exit 0. |
| `cargo fmt --all -- --check` | PASS, exit 0. |
| `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | PASS, exit 0. |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` | PASS: 4 selected, 4 passed; 161 unrelated tests skipped. |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` | PASS: all 5 tests, including stateful session reuse. |
| `git diff --check` | PASS, exit 0. |
| `git status --short` | Only the two scoped implementation files and this plan/index changed. |

Verification used local controlled children on Linux, with no live MCP service or
credentials. Visual/xterm.js verification was unnecessary for this process-lifetime
change. The full workspace test suite was not run; the scoped checks above passed.
