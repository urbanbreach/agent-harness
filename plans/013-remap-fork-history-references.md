# Plan 013: Remap retained-history IDs when forking a compacted session

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-core/src/session_lineage/materialization.rs crates/harness-core/tests/session_lineage_materialization/01_session_lineage_materializes_child_atomically_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE
- **Issue:** [#236](https://github.com/urbanbreach/agent-harness/issues/236)
- **Priority:** P1
- **Effort:** M
- **Risk:** MED
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 12 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Fork materialization assigns child entry IDs but copies SessionCompaction.first_kept_entry_id unchanged. Continuation prefers that typed ID to the sequence fallback, so a fork can silently omit its retained history. Rewrite the reference with the same source-to-child identity mapping used for copied entries.

## Current state

`crates/harness-core/src/session_lineage/materialization.rs:365` — The payload is copied before the child entry is re-identified.

```rust
/// Policy: payloads, actors, timestamps, and monotonic times remain unchanged so replay observes the
/// same completed work. The envelope gets a fresh child `run_id`, contiguous child-local `seq`, and a
/// new event id derived from that child identity. `correlation_id` and `causation_id` are cleared so
/// the child log cannot imply causal links to the parent run's event ids; stream keys are only
/// rewritten for the run-scoped `run:<source>` key and otherwise preserved.
pub fn rewrite_child_event_envelope(
    source: &EventEnvelopeV1,
    source_run_id: Option<&str>,
    child_run_id: &str,
    child_seq: u64,
) -> EventEnvelopeV1 {
    let mut rewritten = source.clone();
    rewritten.event_id = child_event_id(child_run_id, child_seq);
    rewritten.seq = child_seq;
    rewritten.run_id = crate::ids::RunId::from(child_run_id);
    rewritten.correlation_id = None;
    rewritten.causation_id = None;
    rewritten.stream_key =
        rewrite_stream_key(source.stream_key.as_deref(), source_run_id, child_run_id);
    rewritten
```

`crates/harness-core/src/agent/provider_boundary/continuation/projection.rs:95` — Retained history is gated on an entry ID match.

```rust
    let mut include_entry = first_kept.is_none();
    let tool_ids = tool_ids(view);
    for entry in &view.entries {
        include_entry |= first_kept == Some(entry.id.as_str());
        let excluded = entry
            .turn_id
            .as_ref()
            .is_some_and(|turn_id| excluded_turn_ids.contains(turn_id.as_str()));
        if include_entry && !excluded {
            messages.extend(entry_message(
                entry,
                &view.owner.agent_id,
                &attachments_for_entry(view, &entry.id),
                &tool_ids,
                lower_attachments,
            ));
```

## Conventions and exemplar

Append-only source journals remain unchanged. Use the existing namespace_entry_id identity helper and materialization error type. Legacy compactions with no typed ID retain their sequence fallback; do not remove typed IDs to force fallback. No event schema change.

`crates/harness-core/tests/session_compaction_shape_test.rs:120` — Use a real typed compaction boundary, not an unrelated synthetic event field.

```rust
}

#[test]
fn session_compaction_adapter_maps_v2_fields_to_canonical_summary() {
    // Given: a typed compaction boundary and all summary-generation metadata.
    let run_id = RunId::new("run-compaction-shape");
    let first_kept_entry_id =
        LegacyIdentityNamespace::new(&run_id).entry_id(2, "event-2", "user_message");
    let events = vec![
        envelope(
            1,
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --test session_lineage_materialization_test --test session_compaction_shape_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-core --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-core/src/session_lineage/materialization.rs`
- `crates/harness-core/tests/session_lineage_materialization/01_session_lineage_materializes_child_atomically_test.rs`

Administrative updates to `plans/013-remap-fork-history-references.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-013-remap-fork-history-references` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Exercise a compacted fork through canonical continuation

Extend the existing atomic materialization integration fixture with a real compaction summary and a retained suffix. Materialize the child and project its canonical provider continuation. In a small table cover a typed boundary, a legacy sequence-only boundary and an unresolved typed boundary. Assert the successful cases contain both summary and retained suffix, and that source bytes remain identical.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test session_lineage_materialization_test --test session_compaction_shape_test` → The typed-boundary row demonstrates the missing retained suffix on the baseline; existing lineage cases still pass.

### Step 2: Remap typed references before publishing the child

In materialization.rs build the source entry ID to child entry ID map for the complete copied prefix before serializing entries. Rewrite SessionCompaction.first_kept_entry_id through that map. Reject an unresolved typed reference with a structured materialization error before installing the child directory. Preserve legacy None and every unrelated payload field; use the existing staged materialization cleanup.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test session_lineage_materialization_test --test session_compaction_shape_test` → Typed and legacy cases retain the expected history; unresolved typed references leave no published child.

### Step 3: Verify identity and atomicity compatibility

Run both named integration targets. Check child IDs and ancestry remain deterministic and that failed materialization leaves the source journal byte-for-byte unchanged.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test session_lineage_materialization_test --test session_compaction_shape_test` → All selected tests pass; no changes to replay schemas or source histories.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-core --test session_lineage_materialization_test --test session_compaction_shape_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Typed and legacy compacted forks retain the expected summary and suffix in canonical provider continuation.
- [x] An unresolved typed boundary fails before child publication and source journal bytes remain unchanged.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## Execution evidence — 2026-09-20

Implemented on `codex/plan-013-remap-fork-history-references` in the isolated
`/home/urbanbreach/Projects/agent-harness-plan-013` worktree, based on `06467bfe`.
The required drift comparison against `7f5a7ec6` showed no changes in either
implementation file. The original checkout's branch and uncommitted files were
preserved; only this plan's index entry was added to the executor's tracked index.

The prefix rewrite now builds its complete source-to-child entry map using the
existing `EventIdentityNamespace::source_entry_id` helper (the shipped identity
helper referred to as `namespace_entry_id` above). It remaps typed compaction
boundaries before serialization. An unresolved reference returns
`UnresolvedCompactionBoundary`; the existing pending-directory guard removes the
staging directory before any child is published. Legacy `None` boundaries and
all other compaction payload fields remain unchanged.

One table-driven regression extends the existing materialization fixture. It
uses the TUI snapshot entry point, matching the original atomic fixture, and
reads the persisted child through `CanonicalSessionProjection` and
`lower_provider_continuation`. Typed and legacy rows preserve the summary and
both retained messages, excluding old history and the uncopied source tail. The
unresolved row references that real uncopied tail and checks the structured error,
absence of a published child, staging cleanup, and unchanged source journal bytes.
Successful rows also verify unchanged source bytes and the complete compaction
payload except for the remapped typed identity.

Before the production fix, the two named integration targets selected **16 tests**:
**15 passed and the new regression failed**. Its typed row received an empty
user-message list instead of `["retained history", "continued history"]`, while
the summary was present. This was the intended baseline failure, distinct from
the final passing run below.

Commands ran from the executor worktree with locked, offline dependencies.
Cargo compilation commands used
`CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target` to reuse the
existing build cache. No dependencies or lockfiles changed.

| Final command | Actual result |
|---|---|
| `cargo nextest run --profile ci --locked --offline -p harness-core --test session_lineage_materialization_test --test session_compaction_shape_test` | Exit 0; **16 passed, 0 skipped**, including existing identity, ancestry, rollback, and concurrent materialization cases. |
| `cargo check -p harness-core --locked --offline` | Exit 0. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0. |
| `git diff --check` | Exit 0. |
| `git status --short` | Only the two allowed implementation files and this plan/index changed in the executor worktree. |

Verification was scoped to the requested offline checks; no live-service or
native signoff was run. Source histories and event schemas were not changed.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A typed boundary refers outside the copied prefix in an intentional supported lineage format; establish that format before choosing a fallback.
- The change requires rewriting the source session or changing the event schema.

## Maintenance notes

Any future event payload containing an entry identity must be reviewed when entries are re-namespaced. Keep the map and typed-reference rewrite together.
