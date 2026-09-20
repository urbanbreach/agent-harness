# Plan 012: Admit native tool execution through the coordinator scheduler

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-core/src/sched.rs crates/harness-core/src/coord/state.rs crates/harness-core/src/coord/run_lifecycle.rs crates/harness-core/src/coord/tool_execution.rs crates/harness-core/src/coord/task_lifecycle.rs crates/harness-core/src/coord/agent_turn_completion.rs crates/harness-core/src/coord/tests.rs crates/harness-core/tests/coord/17_tool_task_lifecycle_events_preserve_owner_test.rs crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/01_foreground_task_waits_for_child_agent_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — implemented and verified 2026-09-20
- **Issue:** [#235](https://github.com/urbanbreach/agent-harness/issues/235)
- **Priority:** P1
- **Effort:** M
- **Risk:** HIGH
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 11 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Native tool execution records a started task and spawns immediately without scheduler admission. Per-tool concurrency limits and stale-task detection therefore do not cover these tasks. Use the existing scheduler for admission, queued cancellation, release and watchdog ownership while preserving foreground delegation.

## Current state (planning baseline)

`crates/harness-core/src/coord/tool_execution.rs:1130` — Native execution constructs a key but unconditionally records Started.

```rust
    let task_id = format!("task_{:06}", run_state.next_task_id);
    run_state.next_task_id += 1;

    let queue_key = ConcurrencyKey::Tool {
        tool_id: tool_id.clone(),
    };
    let schedule_metadata =
        matches!(tool_id.as_str(), "task" | "agent.spawn").then(|| TaskScheduleMetadata {
            lineage: Some(tool_task_lineage_metadata(
                &tool_call_id,
                request_correlation_id.as_deref(),
                None,
            )),
        });

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        actor.clone(),
        Some(format!("task:{task_id}")),
        request_correlation_id.clone(),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.clone().into(),
            state: TaskScheduleState::Started,
            queue_key: Some(queue_key.queue_key()),
            metadata: schedule_metadata,
        }),
```

`crates/harness-core/src/sched.rs:186` — Stale detection only recognizes keys installed in scheduler gates.

```rust
    pub fn detect_stale(
        &self,
        now_mono_ms: u64,
        stale_timeout_ms: u64,
        running_tasks: &[TaskProgressSnapshot],
    ) -> Vec<StaleTask> {
        running_tasks
            .iter()
            .filter_map(|task| {
                let gate = self.gates.get(&task.key)?;
                if gate.in_flight == 0 {
                    return None;
                }

                let stale_for_ms = now_mono_ms.saturating_sub(task.last_progress_mono_ms);
```

## Conventions and exemplar

Runtime scheduling and event authority stay in the coordinator. No semaphore beside Scheduler, no new public commands or durable schema, no provider scheduling rewrite. Permission/registry/capability checks precede admission; start hooks and ToolCallStarted occur only after admission. Preserve caller actor and correlation IDs.

`crates/harness-core/src/coord/agent_turn_completion.rs:514` — Agent turns already retain queued inputs and start dequeued work without admitting twice.

```rust
        for task in dequeued {
            if let Some(queued) = run_state
                .queued_agent_turns
                .get(task.task_id.as_str())
                .cloned()
            {
                append_agent_turn_task_scheduled_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    AgentTurnTaskScheduledEventArgs {
                        task_id: &queued.task_id,
                        agent_id: &queued.agent_id,
                        request_id: &queued.request_id,
                        queue_key: &queued.queue_key,
                        state: TaskScheduleState::Started,
                        child_task: queued.child_task.as_ref(),
                    },
                )?;

                let Some(queued) = run_state.queued_agent_turns.remove(task.task_id.as_str())
                else {
                    continue;
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(tool_task_) \| test(stale_tool_) \| test(native_tool_scheduler)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-core --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Scheduler unit coverage | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(sched::)'` | All selected tests pass. |
| Nested delegation and batch compatibility | `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` | All selected tests pass, including explicit nested delegation at limit one. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-core/src/sched.rs`
- `crates/harness-core/src/coord/state.rs`
- `crates/harness-core/src/coord/run_lifecycle.rs`
- `crates/harness-core/src/coord/tool_execution.rs`
- `crates/harness-core/src/coord/task_lifecycle.rs`
- `crates/harness-core/src/coord/agent_turn_completion.rs`
- `crates/harness-core/src/coord/tests.rs` — initialize the new map in the explicit test RunState fixture.
- `crates/harness-core/tests/coord/17_tool_task_lifecycle_events_preserve_owner_test.rs`
- `crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/01_foreground_task_waits_for_child_agent_test.rs`

Administrative updates to `plans/012-schedule-native-tools.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-012-schedule-native-tools` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add coordinator-boundary scheduling cases

Extend the existing tool lifecycle integration file using FakeClock and controlled tools. With a per-tool limit of one, submit two same-tool calls and a different tool, assert same-tool FIFO admission and independent different-tool progress, then cancel a queued call and verify it never executes. Submit a blocking leaf tool normally and advance the fake clock to prove automatic stale cancellation; do not manually seed a scheduler gate.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(tool_task_) | test(stale_tool_) | test(native_tool_scheduler)'` → The limit and automatic-stale cases fail on the baseline; new cases use native_tool_scheduler in their names.

### Step 2: Split admission from admitted execution

Add a private QueuedToolCall record and RunState.queued_tool_calls map owning the selected key and ToolCallExecutionArgs. After checks, allocate a task ID and call scheduler.schedule before hooks or spawning. Record Queued or Started truthfully. Move only admitted calls into the existing running map and execution path. Initialize and drain the new map during run lifecycle; no response sender may be abandoned. Also initialize it in the explicit unit-test RunState fixture at `crates/harness-core/src/coord/tests.rs:1384`.

**Verify:** `cargo check -p harness-core --tests --locked --offline` → Production and test targets compile with the new queued state. FIFO promotion/cancellation assertions are reserved for Step 3.

### Step 3: Release and dispatch exactly once

Consume the task IDs returned by scheduler.complete through a private coordinator drain helper; dispatch them without scheduling again. Cover normal completion, failure, cancelled late results and pre-spawn start failures. Agent-turn cancellation must remove its queued tool entries, record cancellation and resolve waiting responses. Shutdown resolves queued responses without promoting work. Keep task IDs stable across admission and completion.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(tool_task_) | test(stale_tool_) | test(native_tool_scheduler)'` → Queued cancellation and ordinary completion release capacity exactly once; no waiting response hangs.

### Step 4: Preserve bounded nested delegation

Add ConcurrencyKey::NestedTool { tool_id, parent_tool_call_id } using the same configured tool limit and unchanged durable tool:<id> display. Only for task, agent.spawn and batch invoked by a current foreground child turn, use its immediate parent_tool_call_id when that identifies a still-running uncancelled delegation wrapper; otherwise use the ordinary tool key. Sibling calls within that nested scope still queue. Match these IDs explicitly: canonical_tool_id_for currently returns its input. Do not add a production alias or permit direct batch-in-batch. Extend the existing foreground-task fixture with tool/provider limits one, explicit test-only child delegation permission, a grandchild and mixed wrapper IDs through a test-only forwarding alias. Assert grandchild, child, then parent completion.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` → The nested fixture completes at limit one and preserves event ownership. Run the tools integration command in the command table for this step.

### Step 5: Protect real foreground waits and verify lifecycle compatibility

The default stale timeout is 15 seconds, while native foreground task waits allow 300 seconds. Exclude task/agent.spawn wrappers from stale snapshots only while an actual queued/running foreground ChildTaskTurnState refers to that tool call. Refresh parent progress on child termination, including queued-child cancellation, so normal stale detection resumes during result collection. Use controlled clock/progress to prove the healthy wait survives beyond the stale interval and a stuck post-child wrapper times out after a fresh interval. Preserve demotion/cancellation and run scheduler plus lineage integration coverage. Batch has no equivalent direct child-call ownership; do not silently exempt it by name or invent a general progress framework.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(tool_task_) | test(stale_tool_) | test(native_tool_scheduler)'` → All focused tests pass. The foreground wait survives only while its owned child is active; leaf tools remain subject to the watchdog.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(tool_task_) | test(stale_tool_) | test(native_tool_scheduler)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] The coordinator regression proves per-tool limit one, FIFO promotion, independent keys and cancellation without execution.
- [x] A normally submitted stale leaf tool is automatically cancelled without a manually inserted scheduler gate.
- [x] Nested foreground delegation completes at limit one and its legitimate child wait is not mistaken for a stalled leaf tool.
- [x] Completion, cancellation and shutdown resolve every queued response and release each admitted slot once.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- Existing long-running batch compatibility requires a new parent-child progress contract; report that concrete case before expanding this high-risk patch.
- A nested rule would bypass ordinary leaf-tool limits or permit denied child delegation.
- The fix requires permit-yield/reacquire state, a second scheduler, a new public coordinator command or a durable event schema change.

## Maintenance notes

Keep admission and slot release paired. Review every new terminal path and orchestration tool for queue ownership, nesting and watchdog behavior; orchestration names alone are never an exemption.


## Execution evidence — 2026-09-20

Implemented on `codex/plan-012-schedule-native-tools` in the isolated checkout
`/home/urbanbreach/Projects/agent-harness-plan-012`, based on `06467bfe`.
The required drift comparison against `7f5a7ec6` was empty for every scoped
implementation file. The original dirty checkout, configuration, and unrelated
plan records were preserved. No dependencies or event schemas changed.

Native tool calls now enter the existing scheduler after permission, registry,
and capability checks. Queued arguments and response senders stay owned by the
coordinator; promotion preserves the task ID and does not admit twice. Completion,
failure, start-hook failure, cancellation, late results, and shutdown resolve
responses and release or discard their scheduler ownership appropriately.

Nested `task`, `agent.spawn`, and `batch` calls use a bounded key only for a
current foreground child whose immediate delegation wrapper is still running.
Leaf limits and child permission checks remain in force. Watchdog protection is
limited to actual foreground child waits; child completion and queued-child
cancellation refresh the parent's progress before result collection resumes.
Batch is not exempted from the watchdog. The compatibility alias exists only in
the test fixture.

Six coordinator regression tests cover admission/FIFO/independent keys, queued
cancellation, duplicate late completion, stale cancellation and promotion,
foreground waits and fresh post-child deadlines, stop/failure response draining,
nested sibling/leaf limits and turn cancellation, and queued start-hook failure.
The native delegation fixture covers same and mixed wrapper IDs, nested batch
across child turns, provider/tool limits of one, explicit child delegation
permission, and grandchild-before-child-before-parent completion and ownership.

The existing `child_shared_denies_precede_grants_at_all_execution_gates` unit
assertion was also adjusted within the already scoped `coord/tests.rs`: it now
counts `TaskScheduled` admissions instead of immediate `ToolCallStarted` events.
That fixture deliberately does not process completion messages, so its second
allowed call correctly queues. Denied calls still cannot reach the scheduler.
This is the only test adjustment beyond the planned initializer and two scenario
files. A formatter-warning loop was replaced by `join` without changing its
output when completion handling was separated to satisfy the existing lint limit.

### Verification results

All Cargo commands used locked, offline dependencies. Candidate runs reused
`CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target`.

- **Baseline regression:**
  `cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test -E 'test(tool_task_) | test(stale_tool_) | test(native_tool_scheduler)'`
  before production edits selected 9 tests: 7 passed, the two new admission/stale
  cases failed as intended. Admission recorded four Started states at limit one;
  the stale case never produced StaleDetected within its bounded wait.
- **Final focused behavior:** the same command selected 13 tests; all 13 passed.
- **Production and test compilation:**
  `cargo check -p harness-core --tests --locked --offline` passed after queued
  state initialization. Final `cargo check -p harness-core --locked --offline`
  passed.
- **Formatting:** `cargo fmt --all -- --check` passed.
- **Core lint:**
  `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings`
  passed, including the adjusted permission assertion.
- **Scheduler units:**
  `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(sched::)'`
  selected 4 tests; all passed. They also passed in the expanded final run below.
- **Native compatibility:**
  `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test`
  selected 40 tests; all passed.
- **Native fixture lint:**
  `cargo clippy -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test --all-features --locked --offline -- -D warnings`
  passed.
- **Expanded coordinator coverage:**
  `cargo nextest run --profile ci --locked --offline -p harness-core --lib --test coord_test -E 'test(coord::) | test(sched::) | binary(coord_test)'`
  selected 419 tests: **418 passed, one independently reproduced baseline failure**.
- **Whitespace and scope:** `git diff --check` passed; `git status --short` showed
  only the nine permitted implementation/test files and these two plan records.

### Independent baseline failure and limits

The expanded run fails
`compaction_v2_summary_generation_success_captures_usage_and_provenance` at
`crates/harness-core/tests/coord/29_compaction_v2_summary_generation_test.rs:97`:
recorded summary usage is 100/100/200 rather than 11/50000/50011. The same assertion
fails on untouched source at `06467bfe`, verified with a **fresh separate target**:

```bash
CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-235-baseline \
  cargo nextest run --profile ci --locked --offline -p harness-core --test coord_test \
  -E 'test(compaction_v2_summary_generation_success_captures_usage_and_provenance)'
```

That baseline run selected one test from the original 167-test binary and failed
with the identical values. An earlier attempt using the shared target was not
accepted as baseline evidence because it reused the candidate binary. The
compaction failure is outside this issue's implementation scope and was left
unchanged. No full-workspace, live-service, PTY, native-signoff, or performance
result is claimed. Existing batch compatibility passed; no general batch
progress contract or watchdog exemption was added.
