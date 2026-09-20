# Plan 032: Remove unused reasoning and tool-input fragment archives

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-core/src/agent/streaming.rs crates/harness-core/src/agent.rs crates/harness-core/tests/coord/04_running_agent_turn_cancellation_emits_single_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#255](https://github.com/urbanbreach/agent-harness/issues/255)
- **Priority:** P2
- **Effort:** S
- **Risk:** MED
- **Depends on:** none
- **Category:** perf
- **Audit finding:** 31 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

AssistantResponse retains vectors of every reasoning and tool-input fragment, duplicating strings already assembled into final output and sent as live events. Production callers do not read these archives. Delete the redundant retained representation while preserving final content and live notifications.

## Current state

`crates/harness-core/src/agent/streaming.rs:374` — The response exposes two redundant fragment vectors and their delta-only type.

```rust
pub struct AssistantResponse {
    pub request_id: crate::ids::RequestId,
    pub provider_id: String,
    pub model_id: String,
    pub text: String,
    pub reasoning: String,
    pub reasoning_deltas: Vec<String>,
    pub tool_call_deltas: Vec<AssistantToolCallDelta>,
    pub tool_intents: Vec<AssistantToolIntent>,
    pub stop_reason: String,
    pub usage: Option<CompletionUsage>,
    pub started_metadata: ProviderRequestStartedMetadata,
    pub finished_metadata: ProviderRequestFinishedMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantToolCallDelta {
    pub tool_call_id: crate::ids::ToolCallId,
    pub function_name: Option<String>,
    pub arguments_delta: String,
}
```

`crates/harness-core/src/agent/streaming.rs:578` — Each incoming fragment is cloned into an archive alongside useful processing.

```rust
                }
            }
            ProviderStreamEvent::ReasoningDelta(delta) => {
                if !delta.is_empty() {
                    reasoning.push_str(&delta);
                    reasoning_deltas.push(delta.clone());
                    emit(AgentRuntimeEvent::ProviderReasoningDelta {
                        request_id: provider_request_id.clone(),
                        delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id,
                function_name,
                arguments_delta,
            } => {
                let tool_call_id = crate::ids::ToolCallId::from(tool_call_id);
                tool_call_deltas.push(AssistantToolCallDelta {
                    tool_call_id: tool_call_id.clone(),
                    function_name,
                    arguments_delta: arguments_delta.clone(),
                });
                if !arguments_delta.is_empty() {
                    emit(AgentRuntimeEvent::ProviderToolInputDelta {
```

## Conventions and exemplar

Keep final reasoning text, tool_intents, request/stop/usage metadata and emitted AgentRuntimeEvent fragments unchanged. Remove AssistantToolCallDelta and its public re-export only after confirming there are no production readers in the workspace. This removes public Rust fields; preserve the repository's release compatibility policy and report any known external consumer before deletion.

`crates/harness-core/tests/coord/04_running_agent_turn_cancellation_emits_single_test.rs:319` — Keep the existing assertions for emitted provider reasoning/tool-input events.

```rust
    let events = events.lock().unwrap_or_abort();
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderRequestStarted(started)
            if started.request_id.as_str() == "provider_call_1"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderReasoningDelta { request_id, delta }
            if request_id == "provider_call_1" && delta == "thinking"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderToolInputDelta {
            request_id,
            tool_call_id,
            delta,
        } if request_id == "provider_call_1"
            && tool_call_id.as_str() == "first_call"
            && delta == "{"
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(provider_single_call_returns_tool_intents_without_executing_tools)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-core --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| All workspace callers | `cargo check --workspace --locked --offline` | Exit 0. |
| Removed representation | `rg -n 'reasoning_deltas\|tool_call_deltas\|AssistantToolCallDelta' crates` | No matches; rg exits 1. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-core/src/agent/streaming.rs`
- `crates/harness-core/src/agent.rs`
- `crates/harness-core/tests/coord/04_running_agent_turn_cancellation_emits_single_test.rs`

Administrative updates to `plans/032-remove-unused-stream-fragment-archives.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-032-remove-unused-stream-fragment-archives` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Confirm the complete reader set

Search the workspace for reasoning_deltas, tool_call_deltas and AssistantToolCallDelta. Classify declaration, construction, export and test references. The expected only read is the test archive-length assertion; production consumers use final reasoning/tool intents and live events. Coordinate with active plan 011 terminal-state work.

**Verify:** `rg -n 'reasoning_deltas|tool_call_deltas|AssistantToolCallDelta' crates` → The search shows no production read requiring these fields.

### Step 2: Delete the redundant representation

Remove both fields, local vectors, pushes, constructor assignments, AssistantToolCallDelta and its re-export. Delete only the now-invalid archive-length assertion in the existing semantic test. Leave final assembly and live event assertions intact; do not replace the archive with another buffer or callback layer.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(provider_single_call_returns_tool_intents_without_executing_tools)'` → The focused provider-single-call case passes with unchanged final tool intents and live notifications.

### Step 3: Compile callers and run focused behavior

Run workspace compilation and the focused existing test. Re-run the archive-symbol search: it should return no matches (rg exit 1 is expected). No new benchmark or mirror-only test is necessary for removing a representation with no consumers.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(provider_single_call_returns_tool_intents_without_executing_tools)'` → Workspace compilation and behavioral coverage pass; no archive symbol remains.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(provider_single_call_returns_tool_intents_without_executing_tools)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] No archive field, accumulator or delta-only export remains in crates/.
- [x] Existing final-content/tool-intent and live-event assertions pass, and workspace consumers compile.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A new production reader or known supported external consumer needs the archive; report its use before deleting public fields.
- The deletion would remove live events, final reasoning or tool-call assembly.

## Maintenance notes

Keep transient fragments transient. A future consumer needing history must establish a concrete bounded requirement before reintroducing retention.


## Execution evidence (2026-09-20)

- Baseline was `3d8e3d4f`; the scoped files had no drift from the planned `7f5a7ec6` revision.
- Complete archive-symbol search found declarations, accumulation, export, and one test-only length assertion; no production reader or known supported external consumer. Workspace version is `0.1.0`, with its Rust API described as internal in workspace lint policy. Final text, reasoning, tool intents, and emitted notifications are unchanged.
- Removed both fields and accumulators plus the delta-only type and export; removed only the obsolete archive-length assertion from the existing semantic test.
- `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(provider_single_call_returns_tool_intents_without_executing_tools)'`: **1 passed**, 172 skipped.
- `rg -n '\b(reasoning_deltas|tool_call_deltas|AssistantToolCallDelta)\b' crates`: **no matches**, exit 1. Word boundaries avoid the unrelated provider function `consume_tool_call_deltas`, which the handoff's unanchored pattern also finds.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Cargo checks use `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.
- Independent review and the index update are handled by the integrating coordinator; no issue has been closed from this worktree.

- Per-worktree `cargo check -p harness-core --locked --offline`, `cargo check --workspace --locked --offline`, and `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` were queued on the shared Cargo lock, then canceled at the integrating coordinator's request. Workspace check/Clippy will run once on the integrated changes; these unrun checks are not claimed as passing here.

## Independent closeout — 2026-09-20

Independent agent `verify_existing_core` verified issue #255: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
