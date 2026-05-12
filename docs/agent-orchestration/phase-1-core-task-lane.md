# Phase 1: Core Task Lane MVP

Use this file as a loose implementation prompt for an agent. The goal is to make the existing coordinator-owned child-agent/task lane reliable enough to serve as the foundation for all later orchestration work.

## Task

Harden the existing `task` / `background_output` orchestration path so child agents can be spawned, resumed, observed, cancelled, and replayed through `harness-core` without introducing a second orchestration runtime.

## Expected Outcome

- A minimal, event-sourced child-agent orchestration lane is stable and covered by tests.
- `harness-tools` remains a thin adapter over coordinator APIs.
- Replay/projections can recover child-agent task state from `events.jsonl` without runtime side effects.
- Worker actors still cannot spawn agents directly.

## Required Context

Read these before editing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tools/AGENTS.md`
- `docs/architecture.md`
- `crates/harness-core/src/event.rs`
- `crates/harness-core/src/coord.rs`
- `crates/harness-core/src/agent.rs`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-tools/src/agent_ops.rs`

## Current Architecture To Preserve

- `harness-core::coord` is the only scheduling, permission, event append, and lifecycle authority.
- `EventV1` already includes agent/task/background/permission/policy events.
- `ToolCapability::SpawnAgent` maps to `PermissionKind::Task`.
- `agent_ops::task` already calls coordinator methods and returns sync or background results.
- Replay must remain side-effect free.

## Must Do

- Start by mapping the current `task` flow from tool args to coordinator scheduling to event projection.
- Keep all spawn/resume/cancel lifecycle decisions behind coordinator methods.
- Verify `AgentSpawned`, `TaskScheduled`, `TaskCompleted`, `TaskCancelled`, `TaskResultLate`, `BackgroundTaskNotification`, and `TaskLineageMetadata` are sufficient before adding events.
- If an event addition is truly needed, update projections, docs, schema/drift tests, and replay tests in the same change.
- Lock down the supervisor-only spawn invariant with tests.
- Lock down worker redelegation denial with tests.
- Ensure background output reads from event/projection state, not an in-memory handle that disappears on resume.
- Ensure cancellation and late-result behavior are deterministic.
- Preserve existing public tool ids and schema strictness.

## Must Not Do

- Do not spawn detached subprocesses as the primary orchestration mechanism.
- Do not add a `BackgroundManager` outside the coordinator.
- Do not move scheduling or permission decisions into `harness-tools`, CLI, or TUI.
- Do not introduce team/mailbox/worktree concepts in this phase.
- Do not add new dependencies.
- Do not weaken existing tests or compatibility assertions to make the lane pass.

## Likely Files

- `crates/harness-core/src/coord.rs`
- `crates/harness-core/src/event.rs`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-core/src/perm.rs`
- `crates/harness-core/tests/coord.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-tools/src/lib.rs`
- `docs/architecture.md`
- `docs/testing.md`

## Suggested Implementation Steps

1. Write or identify tests for the existing happy path: supervisor schedules child task, child completes, parent can fetch output.
2. Add missing tests for failure modes: unknown child profile, worker actor spawn attempt, cancellation, late result, resumed child session, and replay-derived background output.
3. Refactor coordinator internals only if necessary to make the task lifecycle explicit and testable.
4. Keep `agent_ops.rs` focused on argument validation, permission metadata, coordinator calls, and response formatting.
5. Update projections so the event log is the complete source for child task observability.
6. Update docs only after the behavior is protected by tests.

## Verification

Run the narrowest relevant checks first, then widen as needed:

```bash
cargo test -p harness-core coord::
cargo test -p harness-tools
cargo test -p harness --test event_docs_reference
cargo check --workspace
```

If public event or config docs change, also run the documented drift tests and update generated schemas only through the existing project workflow.
