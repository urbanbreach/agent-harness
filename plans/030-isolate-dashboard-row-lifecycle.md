# Plan 030: Keep child-task events from completing or hiding parent dashboard rows

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tui/src/dashboard/status.rs crates/harness-tui/src/dashboard/model.rs crates/harness-tui/src/dashboard/projection.rs crates/harness-tui/src/app.rs crates/harness-tui/tests/dashboard_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#253](https://github.com/urbanbreach/agent-harness/issues/253)
- **Priority:** P2
- **Effort:** M
- **Risk:** MED
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 29 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Dashboard input groups the shared journal by run ID, while status inference accepts any child task terminal event as the row's completion. Background notifications can also infer a row as its own parent, hiding it from the root list. Apply lifecycle events to the row they own and reject self-parent links.

## Current state

`crates/harness-tui/src/dashboard/status.rs:46` — Any agent-turn completion/cancellation or background notification can change row status.

```rust
            EventV1::TaskCompleted(data)
                if data
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope)
                    == Some(TaskTerminalScope::AgentTurn) =>
            {
                DashboardStatus::Completed
            }
            EventV1::RunFinished(_) => DashboardStatus::Completed,
            EventV1::RunFailed(_) => DashboardStatus::Failed,
            EventV1::TaskCancelled(_) | EventV1::AgentStopped(_) => DashboardStatus::Cancelled,
            EventV1::StaleDetected(_) => DashboardStatus::Stale,
            EventV1::BackgroundTaskNotification(notification) => match notification.status {
                BackgroundTaskNotificationStatus::Completed => DashboardStatus::Completed,
                BackgroundTaskNotificationStatus::Cancelled => DashboardStatus::Cancelled,
                BackgroundTaskNotificationStatus::Failed => DashboardStatus::Failed,
                BackgroundTaskNotificationStatus::TimedOut => DashboardStatus::Stale,
            },
```

`crates/harness-tui/src/dashboard/model.rs:137` — A notification's parent is accepted without checking the row's identity.

```rust
    pub(crate) fn event_parent_id(events: &[&EventEnvelopeV1]) -> Option<String> {
        events.iter().find_map(|event| match &event.payload {
            EventV1::BackgroundTaskNotification(notification) => {
                Some(notification.parent_session_id.to_string())
            }
            _ => event.lineage_parent_session_id().map(str::to_string),
        })
```

## Conventions and exemplar

Pure dashboard projection remains pure and no event schema changes are needed. Use explicit actor/request/lineage ownership, not an assumption that a run ID equals an agent ID. Preserve legitimate parent-child links and the row's own terminal transitions. Reuse existing semantic status styling; no visual redesign.

`crates/harness-tui/tests/dashboard_test.rs:315` — Extend the existing registry/read-model assertion for an owned completed row.

```rust
    let registry = DashboardReplayRegistry::from_sessions(vec![session(
        "live",
        None,
        SessionModeSource::InteractiveLive,
        events,
    )]);
    let model = build_dashboard_read_model(&registry, &DashboardEligibilityRules::default())
        .unwrap_or_abort();
    let row = model.row("live").unwrap_or_abort();
    assert_eq!(row.title.as_deref(), Some("Renamed live session"));
    assert_eq!(row.status, DashboardStatus::Completed);
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tui --test dashboard_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tui --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tui --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tui/src/dashboard/status.rs`
- `crates/harness-tui/src/dashboard/model.rs`
- `crates/harness-tui/src/dashboard/projection.rs`
- `crates/harness-tui/src/app.rs`
- `crates/harness-tui/tests/dashboard_test.rs`

Administrative updates to `plans/030-isolate-dashboard-row-lifecycle.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-030-isolate-dashboard-row-lifecycle` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add mixed-journal ownership cases

Extend dashboard_test.rs with a parent that remains running or permission-blocked while its shared journal receives child completion, child cancellation and a completed background notification. Assert the parent stays visible as a root with its own status, while the child keeps a valid link/status. Retain the existing own-agent-turn completion assertion as the control.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --test dashboard_test` → The mixed-journal rows expose false completion or disappearance on the baseline.

### Step 2: Filter lifecycle signals by row ownership

Thread the row's existing session/owner context into status inference and accept task/provider/agent terminal signals only for that row's owner. A RunFinished/RunFailed belonging to the row remains terminal; a child background notification updates child/link metadata without terminating its containing parent. Preserve existing ordering and readiness/permission precedence.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --test dashboard_test` → Parent status is unchanged by child terminals and changes correctly on its own terminal event.

### Step 3: Reject self-parent inference at both inputs

In model/projection and AppState's dashboard input construction, ignore a candidate parent equal to the row's own run/session ID, whether it comes from catalog data or an event notification. Preserve real child links and avoid filtering the whole row out as a substitute.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --test dashboard_test` → The parent stays in the root list and the child relationship is retained.

### Step 4: Run dashboard compatibility coverage

Run the complete dashboard integration target, including existing sorting, replay and live-session behavior. Coordinate app.rs edits with plan 033; this issue does not add metadata caching.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --test dashboard_test` → All dashboard cases pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tui --test dashboard_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Mixed child completion/cancellation/notification events cannot finish or hide the parent row.
- [x] The row's own lifecycle still changes status, and valid child links remain visible.
- [x] No inferred parent ID equals its own row ID.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- Owner identity cannot be established from existing actor/request/lineage data; report the ambiguous event fixture rather than guessing from names.
- The fix requires changing durable history or coordinator lifecycle ownership.

## Maintenance notes

Shared journals contain multiple actors. New terminal event types must specify which row they may update.

## Execution evidence — 2026-09-20

- The scoped drift check from the planning baseline to `3d8e3d4f` found no TUI changes. Work used isolated branch `codex/issues-tui`.
- `cargo nextest run --profile ci --locked --offline -p harness-tui --lib --test dashboard_test -E 'binary(dashboard_test) | test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer) | test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` passed **96/96** (6 dashboard and 90 library cases). Log: `/tmp/tui-private-regressions.log`.
- The final run used a private target directory cloned with reflinks from the shared dependency cache, followed by `cargo clean -p harness -p harness-core -p harness-providers -p harness-tools -p harness-tui -p harness-testkit` in that private directory. All required workspace crates were freshly rebuilt; earlier shared-target runs and captures are superseded.
- Cargo used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness-fix-tui/target/tui-private`, `CARGO_BUILD_JOBS=2`, and debug information disabled for dev/test profiles. The test run compiled the modified library and both selected test targets.
- `cargo fmt --all -- --check` and `git diff --check` passed. Workspace check/clippy and independent verification are owned by the integrating agent to avoid duplicate shared-target builds. No unmodified-baseline regression run was performed.
- `HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/agent-harness-tui-private-frames` produced deterministic ANSI fixtures during the passing tests. `node scripts/qa/render-recorded-frames.mjs /tmp/agent-harness-tui-private-frames /tmp/harness-xterm-tui-private` rendered seven frames using xterm.js 6.0.0 and Chromium. The implementation agent inspected the relevant 120×40 and 60×20 screenshots. Manifest: `/tmp/harness-xterm-tui-private/manifest.json`.
- `plans/README.md` is reserved for the integrating agent; this checkout does not alter it. Independent verification, final quality-gate evidence, and issue closure remain with that agent.
- Lifecycle ownership uses recorded root/child agent bindings, task lineage, and request/task correlations; run IDs are never treated as agent IDs. Child notifications preserve parent status and only mark their child row as background. Self-parent candidates are rejected for catalog, journal, child-link, and AppState inputs.
- The shared-journal table protects running and permission-blocked parents against child scheduling/completion/cancellation/stopping/provider fragments/notifications, with an owned parent completion control. Existing dashboard compatibility tests pass.
- Inspected `dashboard-parent-running-{120x40,60x20}-motion-0ms.png`: the parent remains in the working root roster after all child terminal signals.

## Independent closeout — 2026-09-20

Independent agent `verify_tui` verified issue #253: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
