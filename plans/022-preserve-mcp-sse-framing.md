# Plan 022: Frame MCP SSE as bytes before decoding UTF-8

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tools/src/mcp_session.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — scoped verification passed; fresh independent verification pending
- **Issue:** [#245](https://github.com/urbanbreach/agent-harness/issues/245)
- **Priority:** P2
- **Effort:** M
- **Risk:** MED
- **Depends on:** plan 021 (`plans/021-bound-tool-output-capture.md`, [issue #244](https://github.com/urbanbreach/agent-harness/issues/244))
- **Category:** bug
- **Audit finding:** 21 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The MCP HTTP reader decodes each network chunk lossily, corrupting UTF-8 split across chunks. It also searches LF delimiters before CRLF rather than choosing the earliest frame boundary. Frame the bounded byte stream first, then decode complete frames without replacing bytes.

## Current state

`crates/harness-tools/src/mcp_session.rs:569` — Each network chunk is converted independently into a string.

```rust
async fn read_sse_response(
    mut response: reqwest::Response,
    request_id: Option<&str>,
) -> Result<Value, ToolError> {
    let mut buffer = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .tool_err("failed to read MCP SSE chunk")?
    {
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = find_sse_event_boundary(&buffer) {
            let event = buffer[..index].to_string();
            let remainder = buffer[index..].trim_start_matches(['\r', '\n']).to_string();
            buffer = remainder;
            if let Some(message) = parse_sse_event(&event)? {
                if request_id.is_none() {
                    return Ok(message);
                }
                let response_id = message.get("id").and_then(Value::as_str);
```

`crates/harness-tools/src/mcp_session.rs:599` — Delimiter search order can select a later LF boundary over an earlier CRLF boundary.

```rust

fn find_sse_event_boundary(buffer: &str) -> Option<usize> {
    buffer
        .find("\n\n")
        .or_else(|| buffer.find("\r\n\r\n"))
        .map(|index| {
            if buffer[index..].starts_with("\r\n\r\n") {
                index + 4
            } else {
                index + 2
            }
        })
```

## Conventions and exemplar

Apply after plan 021 establishes a finite pending-frame budget. Preserve request-ID matching, notification filtering, multiline data and the existing public MCP result contract. No cross-crate transport framework or lossy decoding fallback.

`crates/harness-providers/src/openai/sse.rs:87` — Use the existing provider byte-framing tests as a pattern; keep the MCP implementation local.

```rust
    #[tokio::test]
    async fn next_sse_event_uses_the_earliest_mixed_delimiter() {
        for (input, second) in [
            ("data: first\n\ndata: second\r\n\r\n", "second"),
            (": comment\r\n\r\ndata: first\r\rdata: second\n\n", "second"),
            ("\n\ndata: first\r\n\r\ndata: second", "second"),
            ("data: first\n\ndata: sécond\n\n", "sécond"),
        ] {
            for chunk_size in 1..=input.len() {
                // arrange: exercise delimiter and UTF-8 splits, including empty frames.
                let chunks: Vec<Result<Vec<u8>, String>> = input
                    .as_bytes()
                    .chunks(chunk_size)
                    .map(|chunk| Ok(chunk.to_vec()))
                    .collect();
                let mut body: OpenAiResponseBody = Box::pin(tokio_stream::iter(chunks));
                let mut buffer = Vec::new();

                // act
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tools --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| MCP integration | `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` | All selected tests pass. |
| Recorded HTTP evidence | `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_http_recorded` | Both recorded loopback cases pass. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tools/tests/mcp_http_recorded.rs` (operator-authorized follow-up: explicit recorded HTTP evidence owner)

- `crates/harness-tools/src/mcp_session.rs`

Administrative updates to `plans/022-preserve-mcp-sse-framing.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-022-preserve-mcp-sse-framing` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Cover chunk boundaries and delimiter order

Add one table-driven case in the existing mcp_session test module: split a non-ASCII JSON response at every byte, alternate LF/CRLF frames in both orders, precede the target response with a notification, and include multiline data. Add invalid UTF-8 and incomplete EOF rows that must return a safe error.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` → Split UTF-8 and mixed-delimiter rows fail on the baseline; expected JSON values retain the original Unicode.

### Step 2: Parse complete bounded frames before decoding

Retain bytes across chunks. Locate both supported delimiters and consume the earliest one with its correct length. Decode only a complete frame with strict UTF-8 validation, then parse the SSE data/JSON using existing request correlation. Carry incomplete bytes to the next chunk and enforce plan 021's cap before append. End without a matching completed response must remain an error.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` → Every chunking/delimiter row returns the same intended value, and malformed rows return no successful result.

### Step 3: Run MCP compatibility coverage

Run library framing tests and generic MCP integration tests. Keep invalid-payload diagnostics content-free and preserve HTTP session headers and request IDs.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` → All selected tests pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(mcp_session::tests)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] The same UTF-8 response survives every tested chunk boundary without replacement characters.
- [x] Mixed LF/CRLF frames are consumed in source order.
- [x] Invalid UTF-8 and incomplete unmatched EOF fail without raw payload diagnostics.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The parser needs to change public MCP result shapes or duplicate provider transport abstractions.
- Plan 021's pending-frame cap is absent or would be bypassed by the new buffer.

## Maintenance notes

Network chunks are never text boundaries. Preserve byte-first framing whenever SSE transport code is rearranged.


## Execution evidence — 2026-09-20

Implemented for #245 after #244 commit `c4f1c501`. The prerequisite supplies the
16 MiB cumulative response budget and incremental delimiter scan offset.

- SSE retains network bytes until a complete frame is available, then validates
  UTF-8 strictly. Delimiters are selected in source order and consumed exactly;
  multiline data, request-ID correlation and notification filtering remain local.
- A table-driven regression exercises every chunk size for both LF/CRLF orders,
  split Unicode, comments, notifications, unmatched IDs, and multiline JSON.
  Local HTTP responses cover invalid UTF-8, incomplete EOF, and unmatched EOF.
- Final source review also found the old SSE JSON error renderer could reproduce
  an HTML payload excerpt. Malformed SSE JSON now returns a fixed content-free
  error, with an additional malformed-HTML row in the existing regression.

Author verification used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure`,
`CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`:

- `cargo nextest run --profile ci --locked --offline -p harness-tools --lib --test mcp_generic_test -E 'test(mcp_session::tests) | binary(mcp_generic_test)'`
  rebuilt harness-tools from this worktree and passed all 12 selected tests
  (seven MCP library tests and five generic integration tests), including #244's
  at-limit and overflow cases. This run preceded the final malformed-HTML row and
  fixed JSON-error diagnostic.
- The same command rebuilt harness-tools after the final diagnostic tightening
  and again passed all 12 tests in 3.07 seconds, including the malformed-HTML row.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The operator subsequently observed possible stale workspace artifacts in the
shared Cargo target across concurrent worktrees. Author results for both this
plan and plan 021 must therefore be confirmed by the independent agent using a
fresh private target. Compile/clippy, that fresh complete verification, the index
row, publication, and issue closure remain with the operator. No visual behavior
changed and no xterm.js verification is needed for this transport-only fix.


## Static-gate follow-up — 2026-09-20

Integrated gates identified a new embedded Python sleep and two loopback HTTP
fixtures in the deterministic library test owner. The operator authorized the
minimal scope expansion to correct the test ownership without weakening gates.

- Moved the real HTTP response-limit and malformed/EOF SSE evidence into
  `tests/mcp_http_recorded.rs`. Both tests now enter through the public MCP
  registry, including real discovery/initialize/notification handshakes. The
  five size/status rows and four invalid/EOF rows remain covered.
- The network-free every-chunk Unicode/mixed-delimiter test stays in
  `mcp_session::tests::sse_frames_preserve_utf8_and_delimiter_order`.
- The shell producer waits on an explicit control socket with a deadline. The
  fixture installs the listener before starting the tool and checks control EOF
  after process cleanup; it no longer sleeps to remain alive.
- No production behavior or test-gate exemption was changed.

Fresh author verification used the new private target
`CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/fix-capture-private`
with `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and
`CARGO_PROFILE_TEST_DEBUG=0`:

- `cargo nextest run --profile ci --locked --offline -p harness-tools --lib --test mcp_http_recorded --test mcp_generic_test --test shell_timeout_boundary_test --test integrations_matrix_test -E 'test(mcp_session::tests) | test(shell_run::tests) | binary(mcp_http_recorded) | binary(mcp_generic_test) | binary(shell_timeout_boundary_test) | binary(integrations_matrix_test)'`: **51 selected tests across five binaries passed**, 139 unrelated
  library tests skipped; execution took 4.53 seconds after a fresh build. This
  supersedes the shared-target caveat for the author coverage of plans 021/022.
- `python3 scripts/check-test-suite-gates.py --gate no-sleeps --gate no-real-world-deps`:
  passed with no violations.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The independent verifier was given the new recorded owner and exact selection.
Independent review and integrated compile/lint/closure remain operator-owned.
