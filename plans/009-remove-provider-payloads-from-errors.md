# Plan 009: Keep malformed provider payloads out of errors and logs

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-providers/src/openai/stream/responses_sse.rs crates/harness-providers/src/openai/tests/tool_errors_test.rs`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 8 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: None
- **Category**: security
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/232

## Execution evidence (2026-09-20)

- Drift check: `git diff --stat 2e342840..HEAD -- crates/harness-providers/src/openai/stream/responses_sse.rs crates/harness-providers/src/openai/tests/tool_errors_test.rs` produced no differences before editing. The parser and existing privacy test matched the plan.
- Before the fix, `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(errors_do_not_leak_auth_secrets)'` exited 100: the extended test failed because the invented sentinel appeared in both serde's type-error text and the raw sample. The existing HTTP 401 assertions passed before that failure.
- After the fix, `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(errors_do_not_leak_auth_secrets) | test(malformed_stream)'` exited 0: **2 passed**, 73 skipped by selection. The privacy test covers a wrong-typed field and truncated JSON, each followed by `[DONE]`; exact event equality checks one `MalformedStream` error, unchanged remediation, no private error fields, and no completion event.
- `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` exited 0: **74 passed**, 1 ignored live-proxy smoke test skipped.
- `cargo check --workspace --locked --offline` exited 0.
- `cargo fmt --all -- --check` exited 0.
- `cargo clippy -p harness-providers --all-targets --all-features --locked --offline -- -D warnings` exited 0.
- `rg -n 'summarize_sse_data|sample=' crates/harness-providers/src/openai/stream/responses_sse.rs` exited 1 with no matches. Source review confirms the warning and public error receive the same fixed message; neither the payload nor the serde error is formatted or logged in this branch.
- `git diff --check` and `git diff --cached --check` exited 0. The commit contains only the two scoped Rust files, this execution record, and the matching plan-index row. Pre-existing user files remain unchanged except this plan's authorized execution record and index entries; unrelated index-document edits remain unstaged.
- The full workspace test suite and repository-wide quality gates were not run; historical planning results are not claimed as current verification.

## Why this matters

The Responses SSE parser includes raw event text and a value-bearing parser error in its failure message. That same message goes to tracing and the public error event, and CLI tracing can persist it. Use a content-free diagnostic for malformed events so private completions, reasoning and echoed credentials cannot enter those sinks through this error path.

## Current state

- [crates/harness-providers/src/openai/stream/responses_sse.rs:69](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L69) — Malformed JSON message includes the payload sample and parser display.
- [crates/harness-providers/src/openai/stream/responses_sse.rs:190](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L190) — summarize_sse_data retains the first 160 characters without redaction.
- [crates/harness-providers/src/openai/stream.rs:151](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream.rs#L151) — The warning helper writes the supplied message verbatim.
- [crates/harness/src/logging.rs:44](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness/src/logging.rs#L44) — CLI tracing installs a persistent file writer.
- [crates/harness-providers/src/openai/stream/chat_sse.rs:82](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/chat_sse.rs#L82) — The Chat parser already uses a content-free invalid-JSON diagnostic.
- [crates/harness-providers/src/openai/tests/tool_errors_test.rs:123](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/tests/tool_errors_test.rs#L123) — Existing error-privacy behavior test and fake transport.

[crates/harness-providers/src/openai/stream/responses_sse.rs:68](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream/responses_sse.rs#L68):

```rust
            Ok(parsed) => parsed,
            Err(err) => {
                let message = format!(
                    "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                    summarize_sse_data(data)
                );
                warn_stream_processing_failure("responses.invalid_json", &message);
                let _ = tx.send(malformed_stream_error(message)).await;
                return;
            }
```

[crates/harness-providers/src/openai/stream.rs:151](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/stream.rs#L151):

```rust
pub(crate) fn warn_stream_processing_failure(context: &str, message: &str) {
    tracing::warn!(
        context,
        message,
        "openai_compatible stream processing failed"
    );
}
```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-providers/src/openai/tests/tool_errors_test.rs:134](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/openai/tests/tool_errors_test.rs#L134):

```rust
    let ProviderStreamEvent::Error { message, .. } = &events[0] else {
        panic!("expected an error event for non-success response")
    };

    assert!(message.contains("status 401"));
    assert!(!message.contains(api_key));
    assert!(!message.to_ascii_lowercase().contains("authorization"));
}
```

Relevant design contract: [crates/harness-providers/src/lib.rs:2](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-providers/src/lib.rs#L2).

Provider-specific failures must be normalized into ProviderStreamEvent with stable categories; no provider payload or reasoning content should be added to durable evidence. Keep the existing MalformedStream category and remediation. The Chat parser's constant diagnostic is sufficient; do not add a sanitizer, logging framework, new dependency or raw debug-mode escape hatch.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` | Selected tests pass after the repair; selection must not be empty. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-providers --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-providers/src/openai/stream/responses_sse.rs`
- `crates/harness-providers/src/openai/tests/tool_errors_test.rs`

Administrative updates are limited to execution status/evidence in `plans/009-remove-provider-payloads-from-errors.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-009-remove-provider-payloads-from-errors`.
- Keep this repair in one logical change; if instructed to commit, use `fix(providers): omit raw SSE data from parse diagnostics`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Extend privacy coverage to malformed Responses streams

Extend openai_compatible_errors_do_not_leak_auth_secrets with a Responses-mode fake SSE response containing an invented private sentinel and invalid JSON or a wrong typed field. Use provider_for_transport_with_mode and collect_events from the existing test module. Assert exactly one MalformedStream error, no Done event, and no sentinel anywhere in the error fields. Include a type-mismatch case because serde's Display error can reproduce an offending value even without the explicit sample.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(errors_do_not_leak_auth_secrets)'` → The new Responses privacy case fails before step 2; the existing non-success HTTP case remains a control.

### Step 2: Remove payload-bearing diagnostics at their origin

Replace the formatted parser-error/sample message with the existing content-free invalid-SSE-JSON wording used by Chat. Pass the same safe constant to warn_stream_processing_failure and malformed_stream_error. Delete summarize_sse_data when its caller is gone. Do not log the caught serde error separately, add payload Debug fields or move samples to another log level.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-providers --lib -E 'test(errors_do_not_leak_auth_secrets) | test(malformed_stream)'` → Privacy and category tests pass. Both the warning and error event visibly receive the same safe constant.

### Step 3: Verify the provider boundary remains stable

Run the provider library suite. Confirm the removed sample helper has no remaining callers and the Responses malformed-JSON branch does not format data or the serde error. Existing HTTP error-detail handling remains out of scope; do not remove all useful non-sensitive diagnostics globally.

**Verify:** `rg -n 'summarize_sse_data|sample=' crates/harness-providers/src/openai/stream/responses_sse.rs` → No matches, exit 1. The provider-library test command in the table also passes.

### Step 4: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

Extend one existing public transport privacy test with the distinct malformed-stream branch and a parser type error. Inspect the same message passed to both sinks instead of adding a new tracing dependency solely to capture a warning. The assertions protect the error value; the single-constant call sites and removal check protect against a separate raw log message.

## Done criteria

All must hold:

- [x] Malformed Responses events produce one categorized error with no raw sample, private sentinel or serde value text.
- [x] The warning and public error use the same content-free message.
- [x] summarize_sse_data and its sample formatting are removed; the provider library suite passes.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-providers --lib` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-providers --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- A test needs live credentials or a network provider request.
- The fix moves raw payload details into another warning, debug field, artifact or durable metadata field.

## Maintenance notes

New provider parser branches should log a stable context/category and safe positions or counts, never a raw sample. Coordinate edits with plan 011, which touches the same parser's terminal branches; the malformed-JSON contract must remain intact.
