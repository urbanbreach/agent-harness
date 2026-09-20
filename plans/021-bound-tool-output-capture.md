# Plan 021: Enforce byte limits while capturing shell and MCP output

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tools/src/shell_run.rs crates/harness-tools/src/mcp_session.rs crates/harness-tools/tests/shell_timeout_boundary_test.rs crates/harness-tools/tests/integrations_matrix_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — scoped verification passed; integrated checks and independent review pending
- **Issue:** [#244](https://github.com/urbanbreach/agent-harness/issues/244)
- **Priority:** P2
- **Effort:** M
- **Risk:** MED
- **Depends on:** plan 015 (`plans/015-clean-up-failed-mcp-startup.md`, [issue #238](https://github.com/urbanbreach/agent-harness/issues/238))
- **Category:** perf
- **Audit finding:** 20 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Shell output is fully buffered by wait_with_output before preview limits apply, and MCP framing/HTTP response paths also allocate unbounded input. A tool can exhaust memory before truncation or artifact handling runs. Enforce finite byte budgets while reading and terminate owned processes when a budget is exceeded.

## Current state

`crates/harness-tools/src/shell_run.rs:500` — wait_with_output buffers both complete streams.

```rust
    child: tokio::process::Child,
    timeout_ms: u64,
) -> Result<ShellProcessOutput, ToolError> {
    let mut process_group = ShellProcessGroupGuard::new(&child)?;
    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
        .await
        .map_err(|_| ToolError::Execution(format!("command timed out after {timeout_ms} ms")))?
        .tool_err("failed to execute command")?;
    process_group.disarm();

    Ok(output.into())
}

```

`crates/harness-tools/src/mcp_session.rs:339` — Content length controls an allocation before a body-size limit is checked.

```rust
        }
    }

    async fn read_framed_message(&mut self, length: usize) -> Result<Value, ToolError> {
        loop {
            let mut header_line = String::new();
            let header_read = timeout(self.timeout, self.stdout.read_line(&mut header_line))
                .await
                .map_err(|_| ToolError::Execution("MCP stdio read timed out".to_string()))?
                .map_err(|err| ToolError::Execution(format!("failed to read MCP header: {err}")))?;
            if header_read == 0 {
                return Err(ToolError::Execution(
                    "MCP stdio server closed before message body".to_string(),
                ));
            }
            if header_line == "\n" || header_line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0_u8; length];
        timeout(self.timeout, self.stdout.read_exact(&mut body))
            .await
```

## Conventions and exemplar

Build on plan 015 child ownership. Use fixed internal budgets: 16 MiB combined shell stdout+stderr, 16 MiB per MCP response/frame and 8 KiB total MCP header bytes. Keep existing preview and redacted artifact behavior below the cap. No new configuration system, dependency, raw spill file or process-wide accounting.

`crates/harness-tools/tests/shell_timeout_boundary_test.rs:99` — Keep real process-exit assertions, not only an error return.

```rust
    let process = started_process(&pid_path).await;

    // assert
    expect_execution_error(error, "timed out");
    assert!(
        elapsed < Duration::from_millis(800),
        "tool call ran past the configured timeout: {elapsed:?}"
    );
    assert_process_exited(process).await;
    assert!(
        !temp.path().join("late-marker.txt").exists(),
        "timed-out shell must not create its delayed marker"
    );
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tools --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Capture and framing units | `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(shell_run::tests) \| test(mcp_session::tests)'` | All selected cases pass. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tools/src/shell_run.rs`
- `crates/harness-tools/src/mcp_session.rs`
- `crates/harness-tools/tests/shell_timeout_boundary_test.rs`
- `crates/harness-tools/tests/integrations_matrix_test.rs`

Administrative updates to `plans/021-bound-tool-output-capture.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-021-bound-tool-output-capture` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add boundary cases to existing process/transport tests

Use controlled local producers to cover exactly-at-limit and one-byte-over combined shell output, including simultaneous stdout/stderr, an oversized MCP Content-Length, an overlong header/line, and oversized HTTP success/error/SSE input. Assert a bounded failure with a content-free message, process exit/cleanup and successful ordinary output/spill below the limit. Keep fixtures deterministic and reuse existing subprocess helpers.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test` → The overflow cases expose full capture/allocation on the baseline; existing timeout/cancellation cases remain in the target.

### Step 2: Drain shell pipes with an explicit shared byte budget

Replace wait_with_output capture with concurrent pipe reads under the existing overall timeout/cancellation handling. Track one local combined byte count before extending buffers; do not sequentially read pipes or deadlock when either fills. On overflow, terminate and reap through the existing process-group guard, then return a fixed output-limit error without captured contents. Preserve exit-code, preview and artifact semantics for accepted output.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test` → Combined output cannot exceed the cap in retained buffers; dual-pipe and cleanup cases pass.

### Step 3: Bound every MCP frame and HTTP body path

Limit total header bytes and line growth before append; validate parsed Content-Length before allocation. Read HTTP success and error bodies incrementally with the same finite response budget. Check incomplete SSE frame growth before appending chunks, preserving the framing behavior for plan 022 to repair separately. Overflow must drop/close the affected session using plan 015 cleanup, never return a successful truncated protocol result.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test` → Oversized length/header/body/frame cases fail safely with no payload in diagnostics; below-cap requests still pass.

### Step 4: Verify cancellation and MCP compatibility

Run the integration targets and shell/MCP library selections. Keep the lifetime limit distinct from byte budgets: a quiet body still needs cancellation/timeout, and a fast producer still needs a cap. Document the intentional internal cap beside its constant.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test` → All selected cases pass without relaxing preview, timeout or redaction checks.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] At-limit output succeeds and one-byte-over input fails before retained capture exceeds its budget.
- [x] Both shell pipes are drained concurrently and overflow terminates/reaps the process.
- [x] MCP declared lengths, headers, HTTP bodies and pending frames are bounded before allocation/growth.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- Existing supported fixtures require a larger cap; document their measured size before changing the proposed budget.
- The approach writes unredacted overflow to disk or serializes pipe reads.
- The requested bound expands to total multi-page MCP results; that is a separate aggregate policy.

## Maintenance notes

Future transport paths must apply limits before growth. Plan 022 should reuse these byte buffers and caps when repairing SSE framing.


## Execution evidence — 2026-09-20

Implemented for #244 on `codex/issues-tool-capture`, based on `3d8e3d4f`.
The baseline drift in MCP startup/cleanup and its integration fixture is the
understood #238 prerequisite. No unrelated behavior drift was found.

- Shell pipes now drain concurrently under one 16 MiB raw-byte budget; timeout,
  cancellation and overflow preserve process-group cleanup, and explicit failures
  kill and reap the owned child. Accepted output retains preview/artifact behavior.
- MCP stdio checks line/header growth and declared frame sizes before allocation;
  header bytes include the first Content-Length line and terminating separator.
  Read failures terminate and reap the unusable stdio child before returning.
- HTTP success/error bodies and SSE input are read incrementally with a 16 MiB
  response budget. SSE scanning resumes at the possible split-delimiter suffix,
  avoiding repeated scans of an at-limit frame. UTF-8/framing repair remains #245.
- Fixed overflow errors contain no captured payload. No dependency, configuration,
  raw spill file, aggregate multi-page policy, or visual behavior was added.

Verification used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure`,
`CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`:

1. Baseline: `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test -E 'test(shell_capture_bounds_combined_pipes_and_terminates_overflow)'`
   selected one regression and failed as expected: overflow reached the 5-second
   command timeout instead of failing at the byte boundary.
2. `cargo nextest run --profile ci --locked --offline -p harness-tools --test shell_timeout_boundary_test --test integrations_matrix_test --lib -E 'test(shell_timeout_boundary_test) | test(integrations_matrix_test) | test(shell_run::tests) | test(mcp_session::tests) | binary(shell_timeout_boundary_test) | binary(integrations_matrix_test)'`
   selected 44 tests: 43 passed; only the at-limit HTTP/SSE case initially failed
   because the old whole-buffer rescan exceeded the HTTP deadline. All shell,
   process-cleanup, stdio framing and existing integration cases passed.
3. After adding the incremental SSE scan offset,
   `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'`
   selected six tests: all passed in 2.32 seconds, including the previously failing
   HTTP success/error/SSE limits. The other 38 passing tests were unaffected.
4. `cargo fmt --all -- --check` and `git diff --check`: passed.

The operator explicitly centralized compile/clippy and final independent-agent
verification in the integrated checkout because concurrent worktree builds share
one Cargo lock. Those checks are not claimed as run here; the operator also owns
`plans/README.md`, publication, and issue closure.
