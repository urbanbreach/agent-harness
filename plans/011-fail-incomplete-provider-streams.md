# Plan 011: Require a valid terminal outcome before completing provider streams

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-providers/src/openai/stream/responses_sse.rs crates/harness-providers/src/openai/stream/chat_sse.rs crates/harness-providers/src/openai/stream_payload.rs crates/harness-providers/src/openai/stream_event.rs crates/harness-providers/src/openai/tool_call.rs crates/harness-providers/src/openai/tests/tool_errors_test.rs crates/harness-providers/src/openai/tests/usage_option_test.rs crates/harness-providers/src/openai/tests/responses_cache_test.rs crates/harness-core/tests/coord/09_failed_turn_context_preserves_provider_error_test.rs`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 10 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plan 009: Keep malformed provider payloads out of errors and logs (`plans/009-remove-provider-payloads-from-errors.md`); [prerequisite issue](https://github.com/urbanbreach/agent-harness/issues/232)
- **Category**: bug
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/234

## Execution evidence (2026-09-20)

- The initial drift check found only plan 009's expected parser/privacy-test changes from commit `515f61ce`. Its execution record is DONE, and the content-free malformed-JSON contract remains intact. The initial working-tree status was recorded before editing; unrelated user changes are preserved.
- Before the repair, `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_terminal) | test(chat_sse_stream_reports_usage)'` exited 100: **1 passed, 1 failed**, 74 skipped. The new regression saw `Started`, partial text, and `DoneWithMetadata` at premature EOF, with no error.
- Both parsers now require an explicit compatible end marker or a valid protocol completion before flushing pending tool calls. Chat keeps consuming trailing usage, validates recognized finish reasons, and discards error payload contents. Responses rejects failure/incomplete events and unsuccessful or contradictory statuses using fixed categorized diagnostics. Existing explicit item completions remain live; core discards them if the response subsequently fails.
- Terminal definitions were checked against the official [Responses streaming guide](https://developers.openai.com/api/docs/guides/streaming-responses) and [Chat streaming reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events). Verification uses only local scripted transports.
- Final `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_terminal) | test(chat_sse_stream_reports_usage)'` exited 0: **3 passed**, 74 skipped. Coverage includes premature EOF, pending tools, late errors after completion, explicit item completion followed by failure, inconsistent statuses, compatible markers, empty arguments, and trailing usage.
- Final `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` exited 0: **76 passed**, 1 ignored live-proxy smoke test skipped. Existing malformed-argument, privacy, Unicode framing, usage, and metadata cases pass; body-read failures retain `TransportFailure` even after a completion event.
- Final `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(failed_turn_context_preserves_provider_error_partial_output)'` exited 0: **1 passed**, 166 skipped. A registered, permitted completed tool call followed by an error produces no tool lifecycle events or successful task outcome, preserves partial assistant text without tool messages, and allows the next successful turn. Core production code is unchanged.
- Final `cargo check --workspace --locked --offline` exited 0.
- Final `cargo fmt --all -- --check` exited 0.
- `cargo clippy -p harness-providers -p harness-core --all-targets --all-features --locked --offline -- -D warnings` initially rejected parser complexity (25/20, then 22/20) and subsequently a test `format_collect`. Consolidating parsing/validation and using `join` resolved these findings; the final command exited 0 without lint suppressions.
- `git diff --check` and `git diff --cached --check` exited 0. The changed source paths match the allowed scope. This execution record and the plan-index entry are included; unrelated index-document additions and other user work remain unstaged.
- The full workspace test suite and repository-wide quality gates were not run. Historical planning results and known unrelated gate failures are not represented as current verification.

## Why this matters

OpenAI stream consumers currently emit Done after body EOF regardless of whether the protocol completed. The Responses consumer also ignores documented failure event forms and flushes pending tool calls on this path. Distinguish successful termination, explicit failure and transport truncation so incomplete responses cannot produce a successful turn or executable pending tool intents.

## Current state

- [crates/harness-providers/src/openai/stream/responses_sse.rs:47](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L47) — EOF currently falls through to tool completion and Done.
- [crates/harness-providers/src/openai/stream/responses_sse.rs:99](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L99) — Only the response.error spelling is rejected; incomplete is grouped with completion.
- [crates/harness-providers/src/openai/stream/chat_sse.rs:118](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/chat_sse.rs#L118) — Chat unconditionally finishes after EOF.
- [crates/harness-providers/src/openai/stream_payload.rs:37](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream_payload.rs#L37) — Response status is available but currently only merged into metadata.
- [crates/harness-providers/src/openai/tool_call.rs:304](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/tool_call.rs#L304) — Pending Responses tool calls are drained by an explicit helper.
- [crates/harness-core/src/agent/streaming.rs:611](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/agent/streaming.rs#L611) — Core collects tool completions and, on Error, returns before parsing executable intents.
- [crates/harness-providers/src/openai/tests/usage_option_test.rs:4](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/tests/usage_option_test.rs#L4) — Existing successful Chat cases include usage after finish_reason without a DONE sentinel.
- [crates/harness-core/tests/coord/09_failed_turn_context_preserves_provider_error_test.rs:3](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/tests/coord/09_failed_turn_context_preserves_provider_error_test.rs#L3) — Existing failed-turn behavior preserves partial text for continuation.

[crates/harness-providers/src/openai/stream/responses_sse.rs:47](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L47):

```rust
    let done_context = loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) => break "responses.done_after_stream_end",
            Err(message) => {
                let message = format!("openai_compatible SSE stream transport error: {message}");
                warn_stream_processing_failure("responses.transport", &message);
```

[crates/harness-providers/src/openai/stream/responses_sse.rs:99](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L99):

```rust
                handle_responses_tool_item_done(&tx, &mut tool_calls, parsed).await
            }
            "response.completed" | "response.done" | "response.incomplete" => {
                apply_response_completion(parsed, &mut usage, &mut finished_metadata);
                true
            }
            "response.error" => {
                warn_stream_processing_failure(
                    "responses.error_event",
                    "openai_compatible responses stream returned error event",
                );
                let _ = tx
                    .send(malformed_stream_error(
                        "openai_compatible responses stream returned error event",
                    ))
                    .await;
                return;
            }
            _ => true,
```

[crates/harness-providers/src/openai/stream/chat_sse.rs:118](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/chat_sse.rs#L118):

```rust
    if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
        return;
    }

    send_stream_event(
        &tx,
        ProviderStreamEvent::DoneWithMetadata {
            usage,
            metadata: non_empty_finished_metadata(finished_metadata),
        },
        "chat.done_after_stream_end",
    )
    .await;
}

```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-providers/src/openai/tests.rs:164](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/tests.rs#L164):

```rust
fn assert_single_error_category(events: &[ProviderStreamEvent], expected: ProviderErrorCategory) {
    let error_events = events
        .iter()
        .filter(|event| matches!(event, ProviderStreamEvent::Error { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        error_events.len(),
        1,
        "expected exactly one error event: {events:?}"
    );
    let ProviderStreamEvent::Error {
        message,
        category,
        remediation,
        ..
    } = error_events[0]
    else {
```

Relevant design contract: [crates/harness-providers/src/lib.rs:2](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/lib.rs#L2).

The provider crate says, “Keep transport/request normalization here so the coordinator and agent loop can remain provider-agnostic.” Preserve live text/reasoning delivery, valid finish metadata, tool argument validation and partial output on failure. A successful Chat finish_reason followed by usage and EOF remains valid without [DONE]. Existing explicit compatible end markers remain supported; plain EOF alone is not one. Unknown informational events may be ignored but cannot establish success. Plan 009's content-free diagnostics must remain intact.

The official [OpenAI streaming guide](https://developers.openai.com/api/docs/guides/streaming-responses) documents error events and ResponseFailedEvent. Use the existing local fake-transport fixtures for verification; no live request is required.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` | Selected tests pass after the repair; selection must not be empty. |
| Coordinator failure behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(failed_turn_context_preserves_provider_error_partial_output)'` | Failure preserves partial text, produces no tool execution and does not turn into Done. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-providers -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-providers/src/openai/stream/responses_sse.rs`
- `crates/harness-providers/src/openai/stream/chat_sse.rs`
- `crates/harness-providers/src/openai/stream_payload.rs`
- `crates/harness-providers/src/openai/stream_event.rs`
- `crates/harness-providers/src/openai/tool_call.rs`
- `crates/harness-providers/src/openai/tests/tool_errors_test.rs`
- `crates/harness-providers/src/openai/tests/usage_option_test.rs`
- `crates/harness-providers/src/openai/tests/responses_cache_test.rs`
- `crates/harness-core/tests/coord/09_failed_turn_context_preserves_provider_error_test.rs`

Administrative updates are limited to execution status/evidence in `plans/011-fail-incomplete-provider-streams.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-011-fail-incomplete-provider-streams`.
- Keep this repair in one logical change; if instructed to commit, use `fix(providers): fail streams without a valid terminal outcome`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Add terminal-outcome cases to the fake transport tests

Extend tool_errors_test.rs with one table covering early EOF after text, early EOF with a pending tool call, error, response.failed, and response.incomplete. Select Chat or Responses explicitly with provider_for_transport_with_mode. Assert exactly one Error and no Done for failure cases; assert pending calls are not flushed by early EOF. Use the existing usage_option_test success table and Responses completion-without-sentinel fixture as controls. Also cover an explicitly completed tool item followed by a failure: that earlier item event must not make the whole response successful.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_terminal) | test(chat_sse_stream_reports_usage)'` → The new failure cases expose baseline false completion; existing valid usage behavior is retained.

### Step 2: Track explicit terminal state in both parsers

Use a small local state/flag with clear names, not a reusable state-machine framework. In Chat, record finish_reason across chunks, continue consuming trailing usage, and allow EOF only after a recognized finish reason or the existing explicit [DONE] end marker. In Responses, recognize response.completed and compatible response.done as successful outcomes only when supplied status is consistent; keep the existing explicit [DONE] compatibility behavior if no failure was seen. Treat error, response.failed and response.error as failure immediately; response.incomplete or an explicitly unsuccessful status must not become successful Done. Use an existing categorized error with a safe fixed message: MalformedStream for premature EOF/protocol contradictions, Other for explicit provider failure/incomplete status, and preserve TransportFailure for body-read errors. Do not copy server error bodies into diagnostics or add new public error categories.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(stream_terminal) | test(chat_sse_stream_reports_usage)'` → Every failure case has one error and zero Done events; valid no-sentinel completion retains its usage and metadata.

### Step 3: Gate pending tool completion on successful termination

Only drain pending tool-call accumulators on an established successful terminal path. On EOF without success or an explicit failure, return after the error and discard pending state. Preserve explicit per-item completion events already emitted during a stream; the core's error branch must discard their prospective intents if the response later fails. Keep final argument JSON validation, one-time emission and successful empty-argument behavior for legitimate no-argument calls. Do not change coordinator scheduling or execute tools from a stream parser.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` → All provider tests pass, including malformed arguments, usage, Responses metadata, Unicode framing and the new terminal cases.

### Step 4: Verify failure cannot reach tool execution

Extend failed_turn_context_preserves_provider_error_partial_output with a scripted completed tool item followed by an Error, using a known registered tool function name from the existing fixture/tool-definition helper. Assert that the failed response adds no ToolCallStarted/ToolCallFinished event and preserves the partial assistant text and failure outcome for continuation. Keep the next successful turn as a control. Core production code should need no change; the current provider_error return already precedes parse_tool_intents.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(failed_turn_context_preserves_provider_error_partial_output)'` → The failed response never invokes a tool, preserves partial output, and the follow-up turn remains valid.

### Step 5: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

Use existing in-memory transport chunks and provider event collection. Name the added table test with stream_terminal so the focused selector is stable. Cover genuinely different terminal/failure states, not many equivalent literals. Preserve the existing usage-after-finish-without-sentinel case; requiring [DONE] everywhere would be a regression.

## Done criteria

All must hold:

- [x] Premature EOF and explicit failure/incomplete events produce one Error and no Done.
- [x] EOF alone never flushes pending tool calls; failures after explicit item completion still produce no core tool execution.
- [x] Successful Chat finish_reason plus trailing usage plus EOF and successful Responses completion without a sentinel remain supported.
- [x] Malformed-stream diagnostics retain plan 009's privacy guarantees and all existing provider tests pass.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-providers -p harness-core --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- Plan 009 is not complete or the implementation reintroduces raw payload/server-error diagnostics.
- The fix would require every provider to send the same terminal marker or would drop the supported trailing usage case.
- The core executes tool intents before the provider response is known to have succeeded, contradicting the cited flow; report that boundary rather than adding ad hoc parser-side execution gates.
- A durable event-schema change, retry-policy redesign or Anthropic rewrite is needed; those are separate work.

## Maintenance notes

Treat terminal semantics as part of the normalized Provider contract. Future transports must distinguish EOF from success, and explicit failure must override accumulated tool state. Stream cancellation and incremental Anthropic work are separate audit findings 15 and 34.
