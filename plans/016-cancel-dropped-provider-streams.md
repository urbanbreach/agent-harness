# Plan 016: Cancel the HTTP reader when a provider stream is dropped

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-providers/src/openai/stream.rs crates/harness-providers/src/openai/tests.rs crates/harness-providers/src/openai/tests/stream_cancellation_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE
- **Issue:** [#239](https://github.com/urbanbreach/agent-harness/issues/239)
- **Priority:** P2
- **Effort:** S
- **Risk:** MED
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 15 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Dropping the public OpenAI-compatible event stream leaves a detached task waiting on its HTTP body. If the server stays silent, the reader never reaches a failed send and can retain its task and connection indefinitely. Tie the reader's lifetime to receiver closure while preserving ordinary completion.

## Pre-implementation baseline

`crates/harness-providers/src/openai/stream.rs:103` — The spawned reader owns the response but does not observe receiver closure.

```rust
    let start_metadata = provider_stream_start_metadata_from_headers(&response.headers);

    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        match mode {
            OpenAiApiMode::ChatCompletions => {
                chat_sse::consume_chat_sse_stream(response, tx, start_metadata).await
            }
            OpenAiApiMode::Responses | OpenAiApiMode::Auto => {
                responses_sse::consume_responses_sse_stream(response, tx, start_metadata).await
            }
        }
    });

    Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
}

```

`crates/harness-providers/src/openai/sse.rs:39` — An idle body read can wait without producing an event.

```rust
        // Recheck the suffix that could begin a split four-byte delimiter.
        scan_offset = buffer.len().saturating_sub(3);
        let Some(chunk) = body.next().await else {
            if buffer.is_empty() {
                return Ok(None);
            }
```

## Conventions and exemplar

Keep both Chat and Responses modes, request/auth behavior, bounded channel backpressure, and normalized events. Use Tokio already in the workspace; do not introduce an abort wrapper or task registry. Sender::closed observes receiver destruction even when the producer is idle.

`crates/harness-providers/src/openai/tests.rs:131` — Reuse OpenAiHttpTransport injection and OpenAiHttpResponse::new; the body can be a controlled pending stream.

```rust
            .unwrap_or_abort();
        let mut headers = HeaderMap::new();
        headers.extend(response.headers.clone());
        if response.status == 200 {
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("text/event-stream"),
            );
        }
        Ok(OpenAiHttpResponse::new(
            response.status,
            headers,
            Box::pin(tokio_stream::iter(response.chunks)),
        ))
    }
}
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

Tokio documents the closure notification used here: [Sender::closed](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.Sender.html#method.closed).

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-providers --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-providers --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Provider compatibility | `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` | All selected tests pass. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-providers/src/openai/stream.rs`
- `crates/harness-providers/src/openai/tests.rs`
- `crates/harness-providers/src/openai/tests/stream_cancellation_test.rs` (create)

Administrative updates to `plans/016-cancel-dropped-provider-streams.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-016-cancel-dropped-provider-streams` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add a controlled idle-body regression

Add stream_cancellation_test.rs under the existing OpenAI test module and register it in tests.rs. Implement only a test transport/body that signals when first polled and when dropped, then remains pending until explicitly released. Through Provider::stream_completion, cover Chat and Responses in one table: wait for the poll signal, drop the returned stream, and require the body-drop signal within a bounded timeout. Use a handshake rather than a sleep. Retain a successful stream as control.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` → The new cancellation assertion fails on the baseline; the test name includes stream_cancellation.

### Step 2: Observe receiver closure around the reader future

Inside stream_completion's spawned task, clone the sender solely for its closed() future and select between that notification and the existing mode-dispatched reader future. Closure must drop the entire reader future and its owned response. Keep the normal reader branch unchanged and ensure the observation sender also drops on normal completion. Do not wait for the next network chunk or send a synthetic error to an absent consumer.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` → The idle reader is released after consumer drop in both modes; successful completion still closes the stream normally.

### Step 3: Run provider compatibility checks

Run the focused regression and the full provider library selection. Preserve Auto fallback, terminal usage and malformed-stream error behavior. Coordinate edits with plans 009/011 if their implementation is active; this repair does not depend on their terminal-state changes.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` → All existing provider library cases and the new cancellation case pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] A blocked body is dropped after receiver destruction without any extra network payload.
- [x] Both Chat and Responses cancellation cases pass, and ordinary completion/usage behavior is unchanged.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The proposed repair waits for another HTTP chunk, introduces process-wide cancellation state, or requires a public ProviderEventStream API change.
- The controlled test only drops an unpolled task and therefore cannot demonstrate cancellation of an active reader.

## Maintenance notes

Keep ownership-based cancellation when adding a streaming mode. Plan 035 uses the same receiver-closure contract for Anthropic.

## Execution evidence — 2026-09-20

Implemented from `06467bfe` in the isolated
`codex/plan-016-cancel-dropped-provider-streams` worktree. The required drift check
against `7f5a7ec6` returned no changes in the scoped implementation files. Plans
009/011 were already committed; their parser and terminal-state behavior is
preserved. The original dirty checkout was left untouched. This checkout's
committed index did not contain plan 016, so its row was added without importing
the original checkout's unrelated index edits.

The existing reader task now selects between receiver closure and its owned
mode-specific reader future. Either outcome drops the observation sender; consumer
cancellation also drops the HTTP body immediately. The channel capacity, request
dispatch, normalized events, and public API remain unchanged.

The single table-driven regression covers Chat and Responses with cancellation
and successful-completion controls. Each case waits for the body's first poll.
Cancellation supplies no payload or EOF before requiring the drop signal; controls
release existing SSE fixtures and verify terminal usage and channel completion.

| Command | Actual result |
|---|---|
| `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-providers/src/openai/stream.rs crates/harness-providers/src/openai/tests.rs crates/harness-providers/src/openai/tests/stream_cancellation_test.rs` (before edits) | Exit 0; empty diff. |
| `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` (regression before fix) | Expected exit 100; 1 failed, 77 skipped. Both completion controls passed; cancellation retained bodies in both modes: `[(ChatCompletions, true), (Responses, true)]`. |
| `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_cancellation)'` (after fix) | Exit 0; 1 passed, 77 skipped. |
| `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` | Exit 0; 77 passed, 1 ignored live-proxy test skipped. Includes Auto fallback, usage, and malformed-stream coverage. |
| `cargo check -p harness-providers --locked --offline` | Exit 0. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy -p harness-providers --all-targets --all-features --locked --offline -- -D warnings` | Exit 0. |
| `git diff --check` | Exit 0. |
| `git status --short` | Only the three allowed implementation files and the two plan records changed. |

No dependencies or lockfile changes. No live-service, native, or whole-workspace
runtime tests were run. An xterm.js capture would not demonstrate HTTP body
lifetime, so verification used the deterministic transport boundary instead.
