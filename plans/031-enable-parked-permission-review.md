# Plan 031: Enable advertised transcript review while a permission prompt is parked

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tui/src/app/permissions.rs crates/harness-tui/src/app/key_interaction.rs crates/harness-tui/src/app/mouse_interaction.rs crates/harness-tui/src/ui_chrome.rs crates/harness-tui/src/app/tests/permission_modal_tests.rs crates/harness-tui/src/app/tests/permission_modal_tests_part2_test.rs crates/harness-tui/src/app/tests/permission_modal_tests_part6_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — focused tests and xterm.js verified; independent integrated review pending.
- **Issue:** [#254](https://github.com/urbanbreach/agent-harness/issues/254)
- **Priority:** P2
- **Effort:** M
- **Risk:** MED
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 30 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

When a permission prompt is parked away from composer focus, the footer advertises review controls but the permission overlay consumes every key and returns early. Users cannot use the promised disclosure/scroll behavior. Allow only safe transcript review actions through that overlay and remove the invisible shortcuts hint.

## Current state

`crates/harness-tui/src/app/permissions.rs:512` — The parked prompt handles focus restoration then consumes every other key.

```rust
    pub(super) fn handle_permission_modal_key(&mut self, key: KeyEvent) {
        let Some(permission) = self.active_permission_view() else {
            return;
        };

        if self.focus != super::Focus::Prompt {
            if question_row_walk(&key).is_some()
                || key.code == KeyCode::Char(' ') && key.modifiers.is_empty()
            {
                self.focus = super::Focus::Prompt;
            }
            return;
        }
```

`crates/harness-tui/src/ui_chrome.rs:885` — The footer nevertheless advertises thinking disclosure and shortcuts.

```rust
            (
                "Ctrl+e",
                if app.transcript_thinking_visible() {
                    ":collapse thinking"
                } else {
                    ":expand thinking"
                },
            ),
            ("Ctrl+x", ":shortcuts"),
```

## Conventions and exemplar

The permission overlay continues to own input. Reuse current transcript scrolling, selection and thinking-disclosure actions through an explicit safe whitelist; never fall through to a general handler that can submit, switch sessions, open hidden surfaces or edit drafts. DESIGN.md requires stable overlay ownership and existing semantic tokens. ui.rs deliberately hides review/help surfaces beneath a permission modal, so remove the parked Ctrl+x shortcuts hint rather than introducing another modal.

`crates/harness-tui/src/app/tests/permission_modal_tests.rs:195` — Extend the existing parked-permission behavior coverage.

```rust
    app.handle_key(key(KeyCode::Esc));

    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert!(app.active_permission().is_some());
    assert_eq!(app.focus, Focus::List);

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.focus, Focus::Prompt);
    assert!(intents.lock().unwrap_or_abort().is_empty());
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(permission_modal) \| test(permission_feedback_paste) \| test(question_mouse_wheel) \| test(question_compact_footer)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tui --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tui --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tui/src/app/permissions.rs`
- `crates/harness-tui/src/app/key_interaction.rs`
- `crates/harness-tui/src/app/mouse_interaction.rs`
- `crates/harness-tui/src/ui_chrome.rs`
- `crates/harness-tui/src/app/tests/permission_modal_tests.rs`
- `crates/harness-tui/src/app/tests/permission_modal_tests_part2_test.rs`
- `crates/harness-tui/src/app/tests/permission_modal_tests_part6_test.rs`

Administrative updates to `plans/031-enable-parked-permission-review.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-031-enable-parked-permission-review` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Extend the parked-permission regression

With a pending permission and transcript focus, press the advertised Ctrl+e and supported transcript scroll keys/wheel. Assert disclosure/viewport changes while draft text, permission state and emitted decision/submission intents remain unchanged. Keep Tab/Space restoring prompt focus. Extend existing question-wheel and paste cases as controls.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer)'` → The parked review-action assertions fail on the baseline; permission decisions remain absent.

### Step 2: Dispatch only safe review actions from the overlay

In the permission handler explicitly recognize supported transcript scrolling/selection and thinking disclosure when parked; invoke existing action methods directly. Keep focused question navigation and decision keys owned by the permission path. For wheel events, allow transcript scrolling only in transcript hit regions while parked; permission/question regions retain their existing behavior.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer)'` → Parked review works, focused question controls still work, and no lower-priority generic key handler runs.

### Step 3: Make footer promises match visible behavior

Remove the parked Ctrl+x shortcuts hint because that surface is deliberately hidden while a permission modal exists. Preserve Tab/Space and Ctrl+e hints and their responsive rendering. Run the focused library selection, including existing compact-footer cases.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer)'` → Every remaining advertised parked control works and the question/paste/footer cases pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tui --lib -E 'test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Parked Ctrl+e and transcript scroll actions change only review state.
- [x] Tab/Space restores permission focus; focused question wheel/paste behavior remains covered.
- [x] No review action submits text or a permission decision, and no hidden shortcuts surface is advertised.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- A proposed fallthrough reaches session switching, prompt editing or another overlay.
- The fix requires redesigning modal stacking to display help; removing that inaccurate parked hint is the scoped solution.

## Maintenance notes

Any new parked-review hint must have an explicit safe dispatch path and preserve permission ownership.

## Execution evidence — 2026-09-20

- The scoped drift check from the planning baseline to `3d8e3d4f` found no TUI changes. Work used isolated branch `codex/issues-tui`.
- `cargo nextest run --profile ci --locked --offline -p harness-tui --lib --test dashboard_test -E 'binary(dashboard_test) | test(permission_modal) | test(permission_feedback_paste) | test(question_mouse_wheel) | test(question_compact_footer) | test(session_stack::tests) | test(session_navigation::tests) | test(render_purity)'` passed **96/96** (6 dashboard and 90 library cases). Log: `/tmp/tui-private-regressions.log`.
- The final run used a private target directory cloned with reflinks from the shared dependency cache, followed by `cargo clean -p harness -p harness-core -p harness-providers -p harness-tools -p harness-tui -p harness-testkit` in that private directory. All required workspace crates were freshly rebuilt; earlier shared-target runs and captures are superseded.
- Cargo used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness-fix-tui/target/tui-private`, `CARGO_BUILD_JOBS=2`, and debug information disabled for dev/test profiles. The test run compiled the modified library and both selected test targets.
- `cargo fmt --all -- --check` and `git diff --check` passed. Workspace check/clippy and independent verification are owned by the integrating agent to avoid duplicate shared-target builds. No unmodified-baseline regression run was performed.
- `HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/agent-harness-tui-private-frames` produced deterministic ANSI fixtures during the passing tests. `node scripts/qa/render-recorded-frames.mjs /tmp/agent-harness-tui-private-frames /tmp/harness-xterm-tui-private` rendered seven frames using xterm.js 6.0.0 and Chromium. The implementation agent inspected the relevant 120×40 and 60×20 screenshots. Manifest: `/tmp/harness-xterm-tui-private/manifest.json`.
- `plans/README.md` is reserved for the integrating agent; this checkout does not alter it. Independent verification, final quality-gate evidence, and issue closure remain with that agent.
- The permission handler explicitly permits thinking disclosure, row selection, Page Up/Down, Ctrl+Up/Down, and Home/End while parked. Wheel scrolling is restricted to the transcript hit region; question-option scrolling remains owned by the question.
- Parked review preserves draft text, active permission, and absence of decision/submission intents. Tab/Space restores permission focus. Both permission and compact-question footer regressions verify the removal of the hidden shortcuts promise.
- The first combined run passed 95/96; the new scroll fixture used Markdown soft breaks and did not overflow. Distinct paragraphs and a settled response corrected the fixture; the final 96/96 run passed. An earlier compile attempt exposed a missing type import in the concurrently developed lineage change, corrected before the final run.
- Inspected `permission-parked-review-{120x40,60x20}-motion-0ms.png`: transcript remains scrollable above the parked dock; the footer shows working Tab/Space and Ctrl+e actions without shortcuts.
