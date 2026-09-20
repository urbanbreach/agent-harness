# Plan 035: Yield Anthropic response events before the HTTP body ends

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-providers/src/anthropic.rs crates/harness-providers/src/anthropic/provider.rs crates/harness-providers/src/anthropic/stream.rs crates/harness-providers/src/anthropic/stream_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — focused/provider compatibility checks passed; independent review and centralized lint pending.
- **Issue:** [#258](https://github.com/urbanbreach/agent-harness/issues/258)
- **Priority:** P2
- **Effort:** M
- **Risk:** MED
- **Depends on:** plan 011 (`plans/011-fail-incomplete-provider-streams.md`, [issue #234](https://github.com/urbanbreach/agent-harness/issues/234)); plan 016 (`plans/016-cancel-dropped-provider-streams.md`, [issue #239](https://github.com/urbanbreach/agent-harness/issues/239))
- **Category:** perf
- **Audit finding:** 34 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The Anthropic implementation reads the entire response before returning any events, so users see no incremental output and a late body error discards all earlier text. Stream and normalize complete SSE frames as bytes arrive, preserving accumulated usage and tool-call state.

## Current state

`crates/harness-providers/src/anthropic/provider.rs:50` — Streaming and non-streaming responses currently share whole-body collection.

```rust
                let bytes = match response.bytes().await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return Box::pin(tokio_stream::iter(vec![
                            ProviderStreamEvent::categorized_error(
                                format!("failed to read anthropic response body: {error}"),
                                ProviderErrorCategory::TransportFailure,
                            ),
                        ]));
                    }
                };
                let raw = String::from_utf8_lossy(&bytes);
                let events = if request.stream {
                    parse_anthropic_sse_stream(&raw)
                } else {
                    parse_anthropic_response(&raw)
                };
                Box::pin(tokio_stream::iter(events))
            }
```

`crates/harness-providers/src/anthropic.rs:419` — The complete-input parser already centralizes event normalization and tool state.

```rust
pub fn parse_anthropic_sse_stream(raw: &str) -> Vec<ProviderStreamEvent> {
    let mut state = AnthropicSseStreamState::default();
    let mut events = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data: ") {
            continue;
        }
        let data = &trimmed[6..];
        if data == "[DONE]" {
            break;
        }
        if let Some(event) = parse_anthropic_sse_event(data) {
            events.extend(anthropic_sse_to_provider_event_with_usage(
                &event, &mut state,
            ));
        }
    }
    events
```

## Conventions and exemplar

Keep the existing Provider contract and request construction. Preserve non-streaming JSON behavior, content-block order, tool arguments, terminal usage and stop metadata. Plans 011 and 016 establish the required contracts: EOF alone is not success, failed responses cannot execute pending calls, and dropping a consumer releases an idle body. Inline these contracts here; no shared transport framework or new dependency is needed.

`crates/harness-providers/src/anthropic.rs:867` — The terminal-usage test protects normalized token accounting; reuse its event fixtures.

```rust
        // act
        let events = parse_anthropic_sse_stream(raw);

        // assert
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 8_000,
                    completion_tokens: 840,
                    total_tokens: 8_840,
                }),
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_stop_reason: Some(stop_reason),
                    ..
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

The [Anthropic streaming protocol](https://platform.claude.com/docs/en/build-with-claude/streaming) describes ordered content blocks, message_stop and in-stream error events.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-providers --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-providers --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Provider compatibility | `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` | All selected tests pass. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-providers/src/anthropic.rs`
- `crates/harness-providers/src/anthropic/provider.rs`
- `crates/harness-providers/src/anthropic/stream.rs` (create)
- `crates/harness-providers/src/anthropic/stream_test.rs` (create)

Administrative updates to `plans/035-stream-anthropic-responses-incrementally.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-035-stream-anthropic-responses-incrementally` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Make the current normalization state usable one frame at a time

Extract only the incremental state/event handling needed from parse_anthropic_sse_stream into the private stream module. Keep the public complete-input parser as a compatibility wrapper using the same normalization, with its existing successful fixtures. Do not change request mapping or add provider features.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'` → Existing Anthropic parser, tool assembly and terminal usage cases pass before transport wiring changes.

### Step 2: Wire streaming HTTP bodies to the incremental parser

For request.stream, return a bounded receiver stream after headers and process response.bytes_stream() in an owned reader task. Frame bytes before UTF-8 decoding, support split delimiters/code points and multiline data, and bound retained undecoded frame bytes with a named 16 MiB internal frame limit checked before growth. Reuse the provider SSE framing pattern without making a cross-crate framework. Normalize and send each complete event immediately. Keep non-streaming JSON collection on its existing branch.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'` → A new controlled-body test receives its first TextDelta while the producer remains paused before EOF.

### Step 3: Preserve failure, completion and cancellation semantics

Reuse AnthropicSseStreamState for usage and block assembly. Require message_stop for successful streaming completion; EOF without it, malformed framing or a body read error emits one safe categorized error and no Done. Explicit error events end the response with content-free diagnostics. Never flush incomplete tools on failure. Select the reader against receiver closure so a dropped consumer releases a silent body. Preserve already-delivered partial text.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'` → The controlled fixture covers partial text then error, truncated EOF, terminal usage and consumer drop; failure rows contain no Done.

### Step 4: Verify the public transport wiring and compatibility

Add only the smallest deterministic transport seam needed for a paused byte stream, keeping it private and used by the production streaming branch. Reuse existing fixtures for split Unicode, tool blocks and terminal usage. Ensure the test actually exercises the branch replacing response.bytes(), not just the old complete-input parser. Run the provider library suite.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'` → Incremental delivery is observed before the fixture sends EOF; existing non-streaming JSON and provider cases still pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] The first text event arrives while the HTTP body is still paused before EOF.
- [x] Failure after partial output preserves that output, emits one categorized error and never emits Done.
- [x] UTF-8/framing splits and terminal usage remain correct; receiver drop releases the body.
- [x] Streaming mode no longer awaits response.bytes() or retains a whole-response event vector.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The implementation requires changing the public provider event schema, coordinator execution, or unrelated request mapping.
- A live external request is required for the proposed regression, or the test never enters the new streaming transport path.
- Plans 011/016 have not established the stated terminal and cancellation contracts, or their current implementation contradicts them.

## Maintenance notes

Keep frame limits, cancellation and terminal success separate. Future Anthropic content types must preserve incremental behavior; do not restore whole-body collection to accommodate them.

## Execution evidence (2026-09-20)

Implemented for issue #258 in an isolated worktree based on `3d8e3d4f`, which
contains the #234 terminal/coordinator fix (`06467bfe`) and #239 dropped-consumer
cancellation fix (`b441a4cc`). The drift check found no changes in the Anthropic
files relative to the planning baseline; these prerequisites changed the shared
coordinator and OpenAI transport contracts, which this reader now follows.

The existing normalization/usage state now lives in the private stream module. Both
the complete-input compatibility parser and the HTTP streaming branch use it. The
streaming branch returns a bounded receiver after headers and reads `bytes_stream()`
in a task selected against receiver closure. It frames bytes before decoding, handles
split UTF-8, CR/LF/CRLF delimiters and multiline data, and checks a named 16 MiB retained
frame limit before buffer growth. The non-streaming JSON path keeps whole-body parsing.
Only `message_stop` succeeds. EOF, malformed frames, and body errors fail once without
flushing pending tool state; error diagnostics omit body/event contents.

Three controlled-body tests exercise the private reader called directly by the
production streaming branch, not only the complete-input parser. They cover paused
first output, mixed/split delimiters and Unicode, tool arguments and terminal usage,
transport failure, premature EOF, invalid JSON/UTF-8, an explicit error followed by a
stop, an unterminated stop frame, oversized input, and release of a silent body after
consumer drop. No external service, credentials, sleep, or network fixture is needed.

All Cargo commands used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.

- `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(anthropic)'`: **18 passed** after state extraction, then **21 passed** after transport wiring and new controlled-body coverage.
- `cargo nextest run --profile ci --locked --offline -p harness-providers --lib`: **80 passed, 1 skipped** on the final implementation, including the mixed-line-ending/empty-data handling refinement. The skipped test is the pre-existing ignored live-proxy smoke test requiring `HARNESS_LIVE_PROXY=1` and local proxy access.
- `rustfmt --check --edition 2021 crates/harness-providers/src/anthropic.rs crates/harness-providers/src/anthropic/provider.rs crates/harness-providers/src/anthropic/stream.rs crates/harness-providers/src/anthropic/stream_test.rs`: **passed**.
- `git diff --check`: **passed**.
- The coordinating agent requested one integrated workspace check, Clippy, and formatting pass to avoid redundant queued builds. Those final checks and independent review remain pending here; this plan does not claim they passed.
- Implementation commit scope: only the four allowed Rust files and this plan. The coordinating agent owns the `plans/README.md` status update.

The 16 MiB ceiling applies to retained undecoded frame bytes; assembled tool arguments,
the bounded output queue, and the current transport chunk are separate existing costs.
There is no new dependency, shared transport framework, durable payload storage, or
provider/schema/coordinator change.
