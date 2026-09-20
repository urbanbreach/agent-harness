# Plan 025: Return failure for CLI commands that have no implementation

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness/src/lib.rs crates/harness/tests/cli_authority_matrix_cli_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#248](https://github.com/urbanbreach/agent-harness/issues/248)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 24 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

share, setup and MCP list/stdio/health command paths return success-shaped output without performing the requested operation. Scripts and users can mistake those placeholders for completed work. Keep parsing compatibility but return an explicit unsupported diagnostic and nonzero status.

## Current state

`crates/harness/src/lib.rs:1125` — Sharing manufactures a success-shaped result.

```rust
fn execute_share(command: ShareCommand, io: &mut CliIo<'_>) -> i32 {
    let link = format!("https://share.harness.local/{}", command.target);
    if command.no_copy {
        let _ = writeln!(io.stdout, "{link}");
    } else {
        let _ = writeln!(
            io.stderr,
            "shareable link generated (clipboard copy not implemented)"
        );
        let _ = writeln!(io.stdout, "{link}");
    }
    if let Some(expires) = command.expires {
        let _ = writeln!(io.stderr, "link expires in: {expires}");
    }
    0
}

```

`crates/harness/src/lib.rs:1208` — MCP subcommands report guessed results without runtime execution.

```rust
fn execute_mcp(command: McpCommand, io: &mut CliIo<'_>) -> i32 {
    match command {
        McpCommand::List => {
            let _ = writeln!(io.stdout, "{{\"servers\": []}}");
            0
        }
        McpCommand::Stdio { command, args } => {
            let args_display = args.join(" ");
            let _ = writeln!(
                io.stderr,
                "starting MCP stdio server proxy: {command} {args_display}"
            );
            let _ = writeln!(io.stdout, "{{\"status\": \"stdio_proxy_started\"}}");
            0
        }
        McpCommand::Health { server_id } => {
            if server_id.trim().is_empty() {
                let _ = writeln!(io.stderr, "server_id must not be empty");
                return 2;
            }
            let _ = writeln!(io.stderr, "checking health of MCP server: {server_id}");
            let result = serde_json::json!({
                "server_id": server_id,
                "configured": false,
                "enabled": false,
                "status": "not_configured",
            });
            let _ = writeln!(io.stdout, "{result}");
            0
        }
    }
```

## Conventions and exemplar

Hosted sharing, setup persistence and new MCP command implementations are outside this issue. Preserve existing command names/arguments and empty-server-ID validation. Use clear stderr and nonzero status; do not emit fake URLs, configured-state claims or success JSON.

`crates/harness/tests/cli_authority_matrix_cli_test.rs:13` — Exercise the public CLI boundary with injected IO and dependencies.

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

Administrative updates to `plans/025-fail-unsupported-cli-commands.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-025-fail-unsupported-cli-commands` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Replace placeholder-output expectations with behavioral assertions

Extend the authority matrix table for share, both setup modes, and MCP list/stdio/health. Give MCP cases a real configured fixture entry so the test cannot pass by guessing not-configured. Assert nonzero status, a specific unsupported/unavailable diagnostic and no success-shaped stdout or filesystem side effect.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` → Existing stubs fail the new public-boundary assertions.

### Step 2: Fail explicitly at each unimplemented handler

Update the corresponding lib.rs handlers to return the established CLI failure status with concise operation-specific stderr. Retain parser compatibility and meaningful validation, and mark those paths unavailable in their existing Clap help text. Delete fabricated result/URL branches rather than constructing a service layer.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` → All placeholder cases now fail honestly; unrelated implemented commands retain their existing results.

### Step 3: Run authority-matrix coverage

Run the complete target and inspect --help in the existing in-process fixture if it already asserts command descriptions. Do not add a test that only mirrors the exact help wording.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` → The entire authority matrix passes without requiring network access.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test cli_authority_matrix_cli_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Every enumerated stub returns nonzero with an unsupported diagnostic and no fabricated success output.
- [x] Existing implemented CLI commands and empty-ID validation still pass.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- One of the listed handlers has acquired a real implementation since the baseline; preserve it and reconcile that row.
- The repair requires implementing a hosted service or changing the CLI response schema for an implemented command.

## Maintenance notes

A future implementation should replace its unsupported branch and update the same behavioral table only after observable work exists. Coordinate lib.rs edits with plan 017.

## Execution evidence — 2026-09-20

- Based on the archive prerequisite commit `3d8e3d4f`; its expected `lib.rs`
  and authority-matrix changes were retained.
- Deleted success-shaped share/setup/MCP placeholder output. Each retained
  command now reports an operation-specific unsupported diagnostic on stderr,
  emits no stdout, and exits 2. Empty MCP health server IDs retain their
  validation diagnostic. Root and MCP subcommand help label availability.
- Replaced three placeholder-success tests with one public-boundary table for
  share, both setup modes, and MCP list/stdio/health. A parsed, enabled MCP
  fixture entry prevents guessed not-configured output from passing. Each
  operation must leave the fixture unchanged and create no additional files.
- Initial shared-target verification command: `cargo nextest run --profile ci --locked
  --offline -p harness --test prompt_cli_test --test cli_authority_matrix_cli_test
  --lib -E 'binary(prompt_cli_test) | binary(cli_authority_matrix_cli_test) |
  test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) |
  test(mock_model_picker)'`: **42 passed**, including all 15 authority-matrix
  cases. The fixture was then strengthened with a full config parse assertion.
- A subsequent shared-target run linked old CLI handlers (share returned status
  0 and the removed expiry message) despite the current source returning 2.
  The integrating operator requested an isolated target instead of trusting
  shared-target recompilation. Reflink-copied the dependency cache to
  `target/issue-cli-private`, then cleaned all six workspace packages only in
  that private target before rebuilding the focused cases.
- Final private-target verification repeated the combined nextest command above:
  **42 passed**, including all 15 authority cases with the complete parsed MCP
  fixture. Nextest run ID: `2cc378b6-d0de-44f2-bca4-3de6b345f40e`.
- Final Cargo commands used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-cli-private`,
  `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`.
- `cargo fmt --all -- --check` and `git diff --check`: exit 0.
- Scoped `cargo check` and `cargo clippy` were queued, then canceled before
  execution at the integrating operator's request. Integrated workspace checks
  and independent review remain the closure gate, and `plans/README.md` is left
  to that operator. Neither deferred check is claimed as passed here.

## Independent closeout — 2026-09-20

Independent agent `fix_provider_sessions` verified issue #248: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
