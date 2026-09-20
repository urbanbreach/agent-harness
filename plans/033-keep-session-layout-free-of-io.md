# Plan 033: Load session lineage before TUI layout and navigation projection

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tui/src/app.rs crates/harness-tui/src/app/lifecycle.rs crates/harness-tui/src/app/session_stack.rs crates/harness-tui/src/app/session_navigation.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — focused tests and xterm.js verified; independent integrated review pending.
- **Issue:** [#256](https://github.com/urbanbreach/agent-harness/issues/256)
- **Priority:** P2
- **Effort:** M
- **Risk:** MED
- **Depends on:** none
- **Category:** perf
- **Audit finding:** 32 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Session-stack predicates read and parse meta.json during layout and parent/subagent queries. Repeated synchronous I/O makes rendering depend on disk changes and violates the pure-projection convention. Load lineage at explicit session transitions and let layout read retained state.

## Current state

`crates/harness-tui/src/app/session_stack.rs:27` — Lineage helpers synchronously read metadata.

```rust
fn harness_lineage(run_dir: &Path) -> Option<Value> {
    let body = fs::read_to_string(run_dir.join("meta.json")).ok()?;
    let metadata: Value = serde_json::from_str(&body).ok()?;
    metadata.get("harness_lineage").cloned()
}

fn harness_lineage_parent_run_id(run_dir: &Path) -> Option<String> {
    harness_lineage(run_dir)?
        .get("parent_run_id")
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
        .map(str::to_string)
```

`crates/harness-tui/src/app/lifecycle.rs:438` — Replay construction already provides an explicit session-load boundary.

```rust
    pub fn new_replay(session_path: PathBuf, events: Vec<EventEnvelopeV1>) -> Self {
        let mut state = Self::new();
        state.replay_mode = true;
        state.session_path = Some(session_path);
        state.replace_events(events);
        state.replay_mode = true;
        state.focus = Focus::Details;
        state
```

## Conventions and exemplar

Store only parsed relationship/parent metadata needed by existing predicates. Constructors and explicit disk/navigation transitions may read; layout and frame projection may not. Preserve current event-based fallback and malformed/missing metadata behavior. No global cache, watcher, new dependency or changes to live coordinator ownership.

`crates/harness-tui/src/app/session_stack.rs:953` — Extend the existing fork-lineage composer visibility test.

```rust
    fn fork_lineage_keeps_the_live_composer_visible() {
        let run = tempfile::tempdir().unwrap_or_abort();
        fs::write(run.path().join("meta.json"), r#"{"harness_lineage":{"relationship":"child_session_materialization","parent_run_id":"parent"}}"#).unwrap_or_abort();
        let app = AppState::new_live(Some(run.path().to_path_buf()), false, None);
        assert_eq!(app.current_parent_session_id().as_deref(), Some("parent"));
        assert!(!app.current_subagent_session_present());
        assert!(app.current_subagent_session_info().is_none());
        assert!(crate::layout::FrameLayoutPlan::for_app(
            &app,
            ratatui::layout::Rect::new(0, 0, 120, 40)
        )
        .composer
        .is_some());
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(session_stack::tests) \| test(session_navigation::tests) \| test(render_purity)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tui --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tui --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tui/src/app.rs`
- `crates/harness-tui/src/app/lifecycle.rs`
- `crates/harness-tui/src/app/session_stack.rs`
- `crates/harness-tui/src/app/session_navigation.rs`
- `crates/harness-tui/src/runtime.rs` — authorized follow-up for the live-history load boundary identified by independent review.

Administrative updates to `plans/033-keep-session-layout-free-of-io.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-033-keep-session-layout-free-of-io` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Add retained lineage at session-load boundaries

Add the minimal AppState lineage fields and populate them where new_live/new_replay and disk navigation install a session path. Use the existing metadata parser once at those boundaries. Audit every internal session_path assignment and session snapshot initializer; do not narrow the public field's API as part of this repair.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` → Existing fork/subagent/root layout cases still pass.

### Step 2: Preserve lineage through navigation and reset

Include the retained lineage in session navigation snapshots so parent/child push-pop restores the matching metadata. Clear it at both session reset paths currently assigning session_path=None in session_navigation.rs. Convert current_parent_session_id and current_subagent predicates to pure retained-data/event projection.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` → Navigation to another session refreshes lineage, returning restores it, and a root reset cannot retain the previous parent.

### Step 3: Prove repeated layout does not read metadata

Extend the existing fork-lineage test: construct the app, then alter/remove meta.json and repeatedly evaluate layout and parent/subagent predicates. Results must remain stable until an explicit session reload, after which they reflect the new metadata. Cover snapshot restoration and missing/malformed metadata through the existing test table.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` → The changed/deleted-file case passes with stable layout, explicit reload refreshes it and all selected tests pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Layout/lineage predicates contain no filesystem read and remain stable after metadata changes on disk.
- [x] Explicit session loads refresh retained lineage, navigation restores it and resets clear it.
- [x] Existing fork/subagent composer visibility and render-purity cases pass.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A supported internal caller mutates session_path without an identifiable load transition; add it to the explicit refresh path before finishing.
- The proposed solution introduces a global metadata cache, background watcher or rendering side effect.

## Maintenance notes

Every new internal session transition must populate, snapshot or clear lineage. Coordinate shared app.rs edits with plan 030.

## Execution evidence — 2026-09-20

- The scoped drift check from the planning baseline to `3d8e3d4f` found no TUI changes. Work used isolated branch `codex/issues-tui`.
- `cargo nextest run --profile ci --locked --offline -p harness-tui --lib --test dashboard_test -E 'binary(dashboard_test) | test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer) | test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` passed **96/96** (6 dashboard and 90 library cases). Log: `/tmp/tui-private-regressions.log`.
- The final run used a private target directory cloned with reflinks from the shared dependency cache, followed by `cargo clean -p harness -p harness-core -p harness-providers -p harness-tools -p harness-tui -p harness-testkit` in that private directory. All required workspace crates were freshly rebuilt; earlier shared-target runs and captures are superseded.
- Cargo used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness-fix-tui/target/tui-private`, `CARGO_BUILD_JOBS=2`, and debug information disabled for dev/test profiles. The test run compiled the modified library and both selected test targets.
- `cargo fmt --all -- --check` and `git diff --check` passed. Workspace check/clippy and independent verification are owned by the integrating agent to avoid duplicate shared-target builds. No unmodified-baseline regression run was performed.
- `HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/agent-harness-tui-private-frames` produced deterministic ANSI fixtures during the passing tests. `node scripts/qa/render-recorded-frames.mjs /tmp/agent-harness-tui-private-frames /tmp/harness-xterm-tui-private` rendered seven frames using xterm.js 6.0.0 and Chromium. The implementation agent inspected the relevant 120×40 and 60×20 screenshots. Manifest: `/tmp/harness-xterm-tui-private/manifest.json`.
- `plans/README.md` is reserved for the integrating agent; this checkout does not alter it. Independent verification, final quality-gate evidence, and issue closure remain with that agent.
- Session constructors load parsed parent/fork state and the small parent task/sibling display context. Navigation snapshots retain that data, inline child views use their parent snapshot, and both session resets clear it. The public `session_path` field remains unchanged.
- The caller audit also found parent event-file reads in `current_subagent_session_info` and `focused_demote_handle_id` (used by status rendering); these now use retained display context. Layout, parent, subagent, and demotion predicates no longer read files. Remaining reads occur only at explicit load/navigation boundaries.
- Regression coverage changes/removes metadata and parent events after loading, repeatedly checks layout and display stability, refreshes through a new replay load, restores the old snapshot, and exercises both reset paths plus missing/malformed metadata.
- Inspected `subagent-retained-lineage-{120x40,60x20}-motion-0ms.png` after deleting source metadata and parent events: the Explore subagent footer/navigation remains stable and the composer stays hidden.

### Independent-review follow-up — live historical lineage

- Independent review found that live construction precedes historical event ingestion. Missing or malformed `meta.json` left the cached parent task and sibling context empty even though the historical lineage correctly identified the parent; replay already loaded in the correct order.
- The live `app_for_mode` path in `runtime.rs` now refreshes retained lineage once after the complete historical event batch. This scope extension was explicitly authorized by the integrating agent under the user's issue-completion request. The refresh adds no per-event file I/O; display, layout, and navigation predicates remain pure.
- Audited every `ingest_historical_event` caller across the workspace: `runtime.rs` is the only real history-load batch; `app.rs` uses it once for a synthetic diagnostic `RunFinished` event; the remaining callers are lifecycle, plan, context-budget, startup, and session-lineage test fixtures. Live constructor wrappers share the canonical constructor. Replay loads events before lineage, disk navigation constructs replay state, and snapshot restoration restores the cached context.
- Extended `subagent_display_retains_parent_context_after_files_are_removed` with a sibling and a table covering missing and malformed metadata. The live ingest-then-refresh sequence and replay both retain the task title, sibling count, and demotion handle; repeated layout remains stable after both metadata and the parent event journal are removed.
- `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` passed **19/19** using the private target and existing debug/job settings above. Log: `/tmp/tui-lineage-refresh-check.log`. `cargo fmt --all -- --check` and `git diff --check` passed.
- Fresh ANSI captures were generated with `HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/agent-harness-tui-lineage-refresh-frames`; xterm.js rendering uses `/tmp/harness-xterm-tui-lineage-refresh/manifest.json`. Inspected both 120×40 and 60×20 captures: the event-derived live session shows Explore (1 of 2), parent/previous/next navigation, and no composer after its source files were removed. Independent follow-up verification remains with the reviewing agent.
