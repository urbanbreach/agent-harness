# Agent Harness PRD — Missing-Specs Companion

**Companion to:** [`agent_harness_opencode_ui_pi_backend_prd.md`](./agent_harness_opencode_ui_pi_backend_prd.md)

**Status:** Specification-only. This document tells implementers *exactly* what the PRD requires but the tree does **not** yet implement. No source files were modified while producing it.

**Authority:** Subordinate to the root [`AGENTS.md`](../AGENTS.md), the PRD, and [`docs/AGENTS.md`](./AGENTS.md). Every implementation below must still honor all AGENTS.md invariants: coordinator owns events/permissions/lifecycle, replay is side-effect-free, hashline edits stay the normal edit path, no provider/auth secrets persisted, etc.

**Rule of thumb:** Every row in this file is its own design ticket. *Do not* batch them into a single PR. Pick up missing items as the matching PRD phase, and treat each one as a standalone cycle: failing test → smallest correct change → evidence record.

---

## 1. How to use this doc

1. Read this together with the parent PRD \u00a70.6 parity bar and \u00a717 task cards.
2. Treat each task header below as a self-contained briefing for the implementing agent.
3. For UI workstream tasks, re-read the cited OpenCode reference file *immediately* before planning, and plan from **observable behavior** (layout, timings, keybindings, focus rules, text, glyph shape) — not from SolidJS/reactive internals.
4. Every UI task ends with a *rendering verification* block. That block must be executed before the task is considered complete.
5. After a task merges, mark `[ ]` as `[x]` here, add an evidence row to PRD \u00a717, and append any rescope note to PRD \u00a715.

---

## 2. OpenCode translation rules (mandatory)

OpenCode is the strongest UI/UX reference for the selected local-coding surfaces. Harness must match its *observable behavior* as closely as possible, implemented natively in Rust/Ratatui/event-sourced Harness architecture.

### 2.1 Always compare against the real reference before planning

Re-read the OpenCode file(s) listed in the task header. Do not rely on this document or the PRD for exact wording, geometry, or timing. Those documents point you *to* the source; the source tells you the behavior.

Primary local references:

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/home.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/sidebar.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/footer.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/permission.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/command-palette.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/prompt/index.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-session-list.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-model.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-variant.tsx`
- `inspirations/screenshots opencode ui parity/`
- `inspirations/opencode-ui-images/`

### 2.2 What to copy vs. what to adapt

| Copy exactly (if applicable) | Adapt to Harness |
|---|---|
| Key chord defaults (leader `ctrl+x`, row labels, scroll family) | KeyMap implementation: sequences, pending leader state, timeout, multi-binding |
| Text/label strings, icon names, prompt placeholder sets | Harness theme tokens; use Harness color palette and brand copy |
| Composer editing motion semantics | Unicode-safe word/line/grapheme operations |
| Modal flow stages, button order, Esc behavior | Event-sourced permission/approval pipeline |
| Footer status cluster layout and glyphs | Ratatui layout + deterministic snapshot |
| Model/variant/agent dialogs: favorites, recents, provider-jump | Existing model_metadata/switcher modules |
| Session list: pin, two-press delete, rename | TUI-local persistence + coordinator commands |

### 2.3 What must **never** be copied

- SolidJS signals / stores / fine-grained reactivity patterns.
- OpenCode plugin architecture, browser/doom-loop bridges, cloud share, account surfaces.
- Source file names, internal identifiers, API routes, or backend execution models.
- Any code that would move event append, permission resolution, or provider retry out of `harness-core::coord`.

### 2.4 Verification style

Every UI change that affects visible chrome must include *one* of:

- A deterministic render test with `insta` snapshot compared to an OpenCode screenshot at the same terminal geometry.
- A PTY capture (`script(1)` or `cargo nextest run -p harness-tui --test pty_e2e`) of the new surface next to the matching OpenCode screenshot, with a written diff of differences and the engineering reason.
- For pure-motion behaviors (cursor motion, undo stack, leader-key timeout), a focused deterministic unit test that exercises edge cases.

---

## 3. Baseline / docs / testing

### [x] T-DOC-01 · Repair doc fragments and stale claims

- **What is missing:** `docs/architecture.md` lines 246-248 still read like a stranded sentence continuation without a preceding topic sentence. The stale parity screenshot in the read-only `inspirations/` folder is still present.
- **Target state:** Decide for each fragment whether (a) it needs a preceding header, (b) should be removed, or (c) should be expanded. Replace or remove the stale screenshot with a current deterministic render capture if the PRD UI workstream matches the skeleton.
- **Implementation note:** Doc-only change; no code change unless removing the stale inspiration asset.
- **Verification:** `cargo nextest run -p harness --test config_docs_reference_test` passes; `scripts/test-lanes.sh quality-gates` passes; a maintainer reviews the edited paragraph for flow.

### [x] T-TEST-01 follow-up evidence

- **What is missing:** The snapshots now pass, but the tree has no permanent record of what behavior changed vs. fixture drift.
- **Target state:** Add one line to `docs/testing.md` or a CHANGELOG-style note in PRD \u00a715 naming the two snapshots and whether they represent new behavior or fixture repair.
- **Verification:** The note is present and references the exact snapshot names.

---

## 4. Transcript performance

All five T-PERF tasks are unstarted and interdependent. Implement them in this order.

### [x] T-PERF-02 · Per-activity text revisions + section-level cache structure

- **What is missing:** `crates/harness-tui/src/app/transcript_cache.rs` stores a single transcript-wide cache epoch. `app/transcript_state.rs:50-84` mixes decoration-only stamps (animation phase, hover target) into the same hash used for measured layout.
- **OpenCode reference behavior:** Not directly visible; this is an architectural hardening requirement. Ratatui must keep measured layouts stable unless the *text* or *geometry* under that section changes.
- **Target state:**
  1. Give each `ActivityEntry` a revision counter held in `app/session_projection.rs` or `app/activity.rs`.
  2. Bump the counter only when mutable fields of that activity change (text delta, status change, tool/unit output change), not on global hover/spinner events.
  3. Replace `TranscriptRenderCache` with a section cache keyed by `(activity_index, section_index, segment_key)` plus a per-section row-height map.
- **Acceptance:** A transient event elsewhere (spinner tick, mouse hover) does not invalidate any section whose content did not change.
- **Verification:** New test `tests/perf_transcript_test.rs` asserts build count stays at 1 when only animation phase or hover target changes.
- **References in tree:** `app/transcript_cache.rs`, `app/transcript_state.rs:44-84`, `app/session_projection.rs` for activity struct.

### [x] T-PERF-01 · Split measured-text-key from decoration key

- **What is missing:** Decoration-only fields (`transcript_animation_phase`, `hovered_transcript_target`) are hashed into the cache stamp that controls whether the measured layout is recomputed.
- **OpenCode reference behavior:** OpenCode separates rendered text width from ephemeral CSS/pulse states; a state tick should not re-layout the transcript.
- **Target state:**
  1. Cache key = `measure_key ^ decoration_key`.
  2. `measure_key` depends only on text content, expansions, and persistent display settings.
  3. `decoration_key` depends on spinner phase, hover, focus, selection span, etc.
  4. Re-measure only when `measure_key` changes; re-render when `decoration_key` changes.
- **Acceptance:** A spinner tick triggers a redraw but does not increment the transcript layout build counter.
- **Verification:** `tests/perf_transcript_test.rs` reports a decoration-only change leaves `build_count_for_test()` unchanged.

### [x] T-PERF-03 · Compact selection snapshot rows

- **What is missing:** Transcript selection currently stores one `String`/cell per grid position for styling and copy; PRD calls for compact row spans.
- **OpenCode reference behavior:** Selection in OpenCode is implemented as char-offset-to-visual-line ranges; copying extracts the selected text directly.
- **Target state:**
  1. Replace the per-cell selection grid with a `Vec<SelectionRow>` where each row holds `(line_index, start_cell, end_cell, Style)`.
  2. Rebuild only on explicit selection-change events, not hover/spinner.
- **Acceptance:** Copy-selection over a 500-message transcript no longer allocates proportional to message count \u00d7 terminal width \u00d7 character cells.
- **Verification:** Memory/heap profiling before/after on a 500-message fixture, or a deterministic time-budget test in `perf_transcript_test.rs`.

### [x] T-PERF-04 · Measurement wrap-correctness property test

- **What is missing:** No property test asserting that measured row count equals rendered row count across line lengths, CJK characters, styled spans, etc.
- **Target state:** Add a test that generates synthetic activity text of varying widths and styles, runs the layout pipeline, and asserts `measured_rows == rendered_rows`.
- **Verification:** Test passes with `quickcheck` or `proptest`; part of `cargo nextest run -p harness-tui`.

### [x] T-PERF-05 · Long-session perf harness

- **What is missing:** No automated benchmark for a 500-message/100-line streaming transcript.
- **Target state:** A runnable test or script that loads a 500-event session, drives a streaming delta, and reports per-delta layout/cache time. Record a before/after row.
- **Verification:** The script runs in CI simulation lane and produces a deterministic number with an explicit budget; failure means a regression.

---

## 5. TUI state maintainability

### [x] T-REF-01 · Extract `ComposerState`

- **What is missing:** `crates/harness-tui/src/app.rs:218-223` stores composer text, cursor, history, and draft inline. This has to grow for selection, undo, stash, and shell-mode state.
- **OpenCode reference behavior:** `component/prompt/index.tsx` isolates prompt state (mode, selection, draft, stash, autocomplete) inside the prompt component.
- **Target state:**
  1. Add `pub(crate) struct ComposerState` in `app/composer.rs`.
  2. Move `prompt_buffer`, `prompt_cursor`, `prompt_history*`, `prompt_history_draft`, plus the new selection, undo, stash, shell-mode fields.
  3. `AppState` embeds `pub(crate) composer: ComposerState`.
- **Acceptance:** No change in existing composer behavior; existing tests pass.
- **Verification:** `cargo nextest run -p harness-tui --test session_navigation_keybindings_test`, `cargo nextest run -p harness-tui --test deterministic_render_test`.

### [x] T-REF-02 \u00b7 Make `OverlayStack` the single source of truth

- **What is missing:** `overlay.rs` derives `OverlayStack` from `OverlayState` booleans. `app.rs:225-269` stores the booleans, creating two sources of truth.
- **OpenCode reference behavior:** OpenCode uses a derived modal stack keyed by route/command dialog state; there is one canonical visible overlay set.
- **Target state:**
  1. `AppState` owns one `OverlayStack` (or `OverlayState` becomes a view over the stack).
  2. Visibility helpers become `app.overlay_stack().top()`, `app.overlay_stack().command_palette_channel_visible()`, etc.
  3. Remove all individual `*_visible` booleans from `app.rs` if they are purely derived, or make them private methods that read the stack.
- **Acceptance:** Opening one overlay automatically closes incompatible ones based on stack rules; no state where two overlays claim focus simultaneously.
- **Verification:** New focused unit tests for overlay precedence; current signoff snapshots stay equivalent.

### [x] T-REF-03 \u00b7 Extract permission + question prompt state

- **What is missing:** Permission/question fields are flat in `app.rs:291-305`.
- **Target state:**
  1. `app/permissions.rs`: `PermissionPromptState { permission_id, stage, selection, confirm_selection }`.
  2. `app/question_prompt.rs`: `QuestionPromptState { permission_id, tab, selection, answers, custom, editing, answer_buffer, answer_cursor, answer_error }`.
  3. `AppState` embeds both structs; refactor key/mouse handlers to dispatch through them.
- **Acceptance:** Existing permission modal tests pass.
- **Verification:** `cargo nextest run -p harness-tui --test deterministic_render_test`; existing `permission_modal_preempts_palette_and_slash` and `question_permission_prompt_renders_without_pty` pass.

### [x] T-REF-04 \u00b7 Extract leaf states

- **What is missing:** Operator sidebar, terminal panel, and onboarding are flat in `AppState`.
- **Target state:**
  - `app/operator_sidebar.rs` \u2192 `OperatorSidebarState`.
  - `app/terminal_panel.rs` \u2192 `TerminalPanelState`.
  - `app/onboarding.rs` \u2192 `OnboardingState` (probably already close).
- **Acceptance:** No behavior changes; just reduced `AppState` field count.

### [x] T-REF-05 \u00b7 Extract `TranscriptViewState`

- **What is missing:** Scroll state, selection, drag state, display toggles, and cache bookkeeping are flat in `app.rs`.
- **Target state:** `app/transcript_view.rs` containing `TranscriptViewState` with all transcript-view-related state and helpers.
- **Acceptance:** Scroll/selection/drag tests pass unchanged.

---

## 6. Runtime hardening

### [x] T-RT-01 \u00b7 Terminal restore on panic / unwind drop guard

- **What is missing:** `crates/harness-tui/src/runtime.rs` calls teardown only after the event loop returns normally. A panic leaves raw mode / alternate screen / mouse capture enabled.
- **OpenCode reference behavior:** Modern terminal apps install a panic hook that restores the terminal before resuming the panic payload.
- **Target state:** Use a RAII `TerminalRestoreGuard` (or set a panic hook) that disables raw mode, leaves alternate screen, disables mouse capture/bracketed paste/keyboard enhancements, and restores saved buffer if preserved-terminal mode is active.
- **Acceptance:** After a forced panic inside the event loop, terminal state is clean and the process aborts with the original message.
- **Verification:** Deterministic unit test using a panic inside a stubbed terminal backend; PTY smoke test under `script(1)` leaves the shell usable afterward.

### [x] T-RT-02 \u00b7 No-op mouse movement does not redraw

- **What is missing:** `runtime.rs:524` `mouse_event_requires_handling()` returns `true` for every mouse move.
- **OpenCode reference behavior:** Mouse moves are used only for hover highlighting; text layout does not change.
- **Target state:**
  1. Distinguish `MouseEventKind::Moved` from wheel/button/drag events.
  2. Mouse move updates `hovered_transcript_target` but sets `redraw_requested = false` unless the hover actually changed a styled region.
  3. Scroll, down, up, drag events still redraw.
- **Acceptance:** Moving the mouse over unchanged areas does not schedule a redraw.
- **Verification:** Unit test in `runtime.rs` tests: two `Moved` events to the same cell produce no redraw; a move to a different cell redraws.

### [x] T-RT-03 \u00b7 Reload/fork event-load budget

- **What is missing:** No cap on events loaded during `r` reload or fork replay.
- **OpenCode reference behavior:** Session reloads are bounded; extremely large histories display a progress indicator.
- **Target state:**
  1. After loading N events (start with 1000), show a transient status banner.
  2. If load time exceeds a budget (start with 1 s), disable follow/sticky scroll and warn the operator.
- **Verification:** A 5000-event replay loads without hanging the event loop; signoff lane still passes.

---

## 7. Backend hardening

### [x] T-BE-05 \u00b7 Actionable mock-fixture-miss error

- **What is missing:** `crates/harness-providers/src/mock.rs:107-110` tells the user to add a fixture but does not point to `--scenario golden_path --deterministic`.
- **Target state:** Expand the error message so it prints:
  - The digest.
  - How to find existing fixtures.
  - Suggested commands: `harness run --scenario golden_path --deterministic` or `harness run --mock "your prompt" --record-fixture` (or the path defined by `MOCK_FIXTURE_RECORD`).
- **Acceptance:** A new user who hits this error can copy-paste one of the suggested commands.
- **Verification:** `cargo nextest run -p harness-providers` passes. New test asserts the error string contains the suggested command.

---

## 8. OpenCode UI workstream — local-coding surfaces

### 8.1 General implementation note

These tasks depend on T-REF-01 (`ComposerState`) and T-UI-10 (leader key). Plan them in this order:

1. Leader key + default keymap (T-UI-10)
2. Composer editing vocabulary (T-UI-11)
3. Footer status cluster (T-UI-01)
4. Shell mode (T-UI-13)
5. Prompt stash + queued prompts (T-UI-12)
6. Permission modal depth (T-UI-17)
7. Session list / model / variant / agent dialogs (T-UI-14, T-UI-16)
8. Transcript navigation + display toggles (T-UI-02)

Theme dialog T-UI-09 and sidebar polish T-UI-08a are independent and can run in parallel after the first two.

### [x] T-UI-10 \u00b7 Leader-key scheme + OpenCode-like default keymap

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/command-palette.tsx`

**What is missing:**

- No leader key support in `KeyMap`; every binding is one chord.
- No pending-leader timeout, no footer hint, no escape-to-cancel.
- No `<leader>` syntax parsing in `KeyBinding::from_str`.
- Default map has ~40 actions vs. OpenCode ~190 commands.

**Target state:**

1. Extend `KeyMap` with a `leader: KeyBinding` field (default `ctrl+x`) and a `KeySequence` parser supporting `<leader>m`, `<leader>l`, etc., plus comma-separated multi-binding in `tui.json`.
2. Add leader-state machine to `app/key_interaction.rs`:
   - On leader press: enter pending mode.
   - After timeout (e.g., 1 s) or non-bound key: cancel silently; show toast if needed.
   - Absorb input while pending.
3. Add default bindings matching OpenCode for every existing Harness action plus the new actions introduced in this workstream. Keep existing non-conflicting single-chord bindings as aliases.
4. Command palette/help rows display multi-chord bindings in OpenCode form (e.g., `ctrl+x m`).

**OpenCode defaults to mirror (minimum):**

| OpenCode binding | Harness command |
|---|---|
| `<leader>m` | model switcher |
| `<leader>l` | session list |
| `<leader>n` | new session |
| `<leader>s` | status dialog |
| `<leader>b` | toggle operator sidebar |
| `<leader>c` | compact session |
| `<leader>g` | timeline/lineage browser |
| `<leader>x` | export session |
| `<leader>y` | copy message |
| `<leader>t` | themes dialog |
| `<leader>a` | agent list |
| `ctrl+p` | command palette |
| `tab` / `shift+tab` | agent cycle |
| `ctrl+t` | variant cycle |
| `f2` / `shift+f2` | recent model cycle |
| `ctrl+r` | rename session |
| `esc` | interrupt (matches existing) |
| `pageup`/`pagedown` | page scroll |
| `ctrl+alt+b/f` | page up/down |
| `ctrl+alt+u/d` | half-page up/down |
| `ctrl+alt+y/e` | line up/down |
| `ctrl+g`/`home` | first message |
| `ctrl+alt+g`/`end` | last message |

**Acceptance:**

- `ctrl+x m` opens model switcher.
- `ctrl+x z` (unbound) cancels without side effects.
- Leader is rebindable via `tui.json` and applies after restart.
- Every new binding from the table above is drift-tested against defaults.

**Verification:**

- `cargo nextest run -p harness-tui` passes.
- New `tests/keybindings_leader_test.rs` exercises sequence dispatch + rebind + cancel.
- Deterministic palette/help snapshot updated to show leader form.

### [x] T-UI-11 \u00b7 Composer input editing vocabulary

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts` (`input_*` block)
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/prompt/index.tsx`

**What is missing:** No selection model, word/line operations, undo/redo, select-all, newline variants, or clear.

**Target state (after `ComposerState` extraction):**

Implement these `Action` variants and wire into `app/composer.rs`:

| Action | Default binding | Behavior |
|---|---|---|
| `SelectCharLeft` | `shift+left` | extend selection by char |
| `SelectCharRight` | `shift+right` | extend selection by char |
| `SelectWordLeft` | `shift+ctrl+left` / `shift+alt+b` | extend selection by word |
| `SelectWordRight` | `shift+ctrl+right` / `shift+alt+f` | extend selection by word |
| `SelectLine` | `shift+home`/`shift+end` | extend selection to line bounds |
| `SelectAll` | `ctrl+a` / `ctrl+shift+a` | select whole buffer |
| `MoveWordLeft` | `ctrl+left` / `alt+b` | move cursor by word |
| `MoveWordRight` | `ctrl+right` / `alt+f` | move cursor by word |
| `MoveLineStart` | `home` / `ctrl+a` (when not selecting) | move to line start |
| `MoveLineEnd` | `end` / `ctrl+e` | move to line end |
| `MoveBufferStart` | `ctrl+home` | move to buffer start |
| `MoveBufferEnd` | `ctrl+end` | move to buffer end |
| `DeleteWordForward` | `alt+d` / `ctrl+delete` | delete word under/after cursor |
| `DeleteWordBackward` | `ctrl+w` / `alt+backspace` / `ctrl+backspace` | delete word before cursor |
| `DeleteLine` | `ctrl+shift+k` | delete current line |
| `KillToLineStart` | `ctrl+u` | delete from cursor to line start |
| `KillToLineEnd` | `ctrl+k` | delete from cursor to line end |
| `Undo` | `ctrl+-` / `ctrl+z` | undo text+cursor+selection |
| `Redo` | `ctrl+.` / `ctrl+y` / `ctrl+shift+z` | redo |
| `InsertNewline` | `shift+enter` / `ctrl+enter` / `alt+enter` / `ctrl+j` | insert newline |
| `ClearPrompt` | `ctrl+c` (when prompt focused) | clear buffer + reset selection |

**Text boundary rules:**

- Grapheme-aware for char motions (use `unicode-segmentation` or equivalent).
- Word boundaries: whitespace, punctuation separator, CJK word boundary.
- Line boundaries inside multi-line prompt.

**Undo stack rules:**

- Bound to e.g., 100 entries.
- Coalesce consecutive identical cursor-only motions.
- Each text mutation pushes a state `(text, cursor, selection_anchor)`.
- History navigation (`HistoryUp`/`HistoryDown`) still preserves draft and works on top of the normal undo stack.
- File-mention tag offsets adjusted via existing `adjust_file_mention_tags_*` helpers after text mutations.

**Acceptance:**

- Multi-line buffer with wide/CJK characters: every binding above matches OpenCode semantics.
- Undo restores text+cursor+selection after a word-delete.
- Existing history preservation tests pass.

**Verification:**

- `tests/prompt_input_tests.rs` style unit tests for each motion.
- Deterministic render snapshot of composer with active selection.

### [x] T-UI-01 \u00b7 Footer status cluster

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/footer.tsx`
- Parity screenshots of session footer.

**What is missing:** Footer only shows cwd/shortcuts/status; no right-hand status cluster.

**Target state:**

In `ui_chrome.rs::render_footer`, add a right-aligned cluster:

- Pending permission count with `\u25b3` warning glyph and count (focused foreground on first).
- LSP count with status dot `\u2022` (green/orange/red depending on counts).
- MCP count with `\u2299` glyph, error-colored if any MCP server has failed state.
- `/status` shortcut hint.

When disconnected/showing startup home, the cluster is empty or shows the Harness brand/version line per OpenCode footer.

Layout: left cwd, middle dynamic hints, right cluster. On very narrow widths, drop the cluster in a documented order (e.g., MCP, then LSP, then permissions, then hints) and/or use the row hint line instead.

**Acceptance:**

- With a pending permission, footer shows `\u25b3 1` and the warning glyph is visible in a snapshot.
- With one failed MCP server, footer shows `\u2299 N` tinted error color.
- The existing footer tests still pass.

**Verification:**

- New deterministic render test for footer status cluster at multiple widths.
- PTY capture of live session with pending permission, next to OpenCode footer screenshot, with written diff.

### [x] T-UI-13 \u00b7 Shell mode (`!` prefix)

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/prompt/index.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/home.tsx`

**What is missing:** No shell submission mode in composer.

**Target state:**

1. `ComposerState` gains `ShellMode` variant with current command.
2. When composer is empty and user types `!`, enter shell mode:
   - Composer placeholder swaps to a rotating set of shell examples (`ls -la`, `git status`, `pwd`, etc.).
   - Composer accent/style changes per theme shell-mode token.
3. `Esc` or Backspace at column 0 exits shell mode.
4. In shell mode, submitting sends `UiIntent::RunShellCommand { command }`.
5. CLI/wiring in `crates/harness/src/tui/` converts the intent to coordinator `Command::RequestToolCall` with actor `operator`, tool id `bash`, and the command. This reuses the normal `bash` permission path, allowlist, blocked-command hints, output caps, artifacts, and events.
6. The resulting lifecycle renders as a normal `bash` tool row.
7. Replay mode and startup-without-session: shell mode must refuse entry with a toast explaining the restriction.

**Acceptance:**

- `!` at col 0 toggles shell-mode placeholders and accent.
- `Esc` and Backspace-at-0 exit.
- Submitting `git status` produces a coordinator-audited `bash` lifecycle and requires the normal `bash` permission.
- Replay mode never enters shell mode.

**Verification:**

- Deterministic render snapshot for shell-mode composer.
- Coordinator-side test proving the intent reuses `RequestToolCall`.
- PTY smoke test: shell-mode submit reaches pending permission.

### [x] T-UI-12 \u00b7 Prompt stash and queued prompts

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/prompt/index.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-stash.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts` (`session_queued_prompts`)

**What is missing:** No stash dialog, no queue indicator, no queued-prompt management.

**Target state:**

Part A — Stash:

1. New `app/prompt_stash.rs` with `PromptStashState` and persistence under `<session-dir>/tui/prompt-stash.json` using the same versioned JSON pattern as `prompt-history.json`.
2. Actions (these are palette/command actions):
   - `prompt.stash` — save current composer text+cursor+selection, then clear composer.
   - `prompt.stash.pop` — restore most recent stash to composer.
   - `prompt.stash.list` — open select-style stash dialog.
3. Stash dialog shows entries with date preview; `ctrl+d` deletes an entry.

Part B — Queue:

1. When `SubmitPrompt` fires while a turn is in progress, the coordinator already supports `QueuedAgentTurn` in `coord/state.rs`. Verify that the CLI side does not reject the prompt but instead routes it to the coordinator queue.
2. Composer shows a `queued N` indicator after successful queue.
3. Queue-management dialog lists pending queued prompts; removing a prompt is only legal before scheduling (coordinator list/remove command may be needed if not already present).

**Acceptance:**

- Stash \u2192 composer clears, pop restores text+cursor.
- Submitting during a running turn queues the prompt and shows the indicator.
- Removing a queued prompt before scheduling prevents its execution.

**Verification:**

- Unit tests for stash round-trip.
- Stash/queue dialog render snapshots.
- Coordinator integration test for submit-while-busy queue path.

### [x] T-UI-17 (partial — see PRD §15.1: selector listing done; per-kind titles and embedded diff preview deferred) \u00b7 Permission modal typed titles + embedded edit diff

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/permission.tsx`
- `inspirations/opencode-ui-images/session-diff.png`

**What is missing:** Modal uses generic wording; no per-kind typed icon/title form; no embedded edit diff preview.

**Target state:**

1. Build per-kind permission title from `PermissionEntry` metadata:
   - `edit` \u2192 `Edit <workspace-relative-path>` with embedded scrollable diff view.
   - `read` \u2192 `Read <path>`.
   - `glob` \u2192 `Glob "<pattern>"`.
   - `grep` \u2192 `Grep "<pattern>"`.
   - `list` \u2192 `List <dir>`.
   - `task` \u2192 `Task <description>`.
   - `webfetch` \u2192 `WebFetch <url>`.
   - `bash` \u2192 `Shell command` with summary.
   - generic tool \u2192 `Call tool <name>`.
2. For `edit`, render the proposed hashline operations/diff using existing `ui_diff*.rs` pipeline in a scrollable frame inside the modal.
3. \u201cAllow always\u201d second stage lists the exact recorded selectors (path, command digest, pattern) using the reference \u201cPatterns allowed:\u201d presentation form.
4. Esc maps to Deny.
5. Preserve existing countdown behavior.

**Acceptance:**

- Edit permission modal shows a scrollable diff preview.
- Read/glob/grep/list/task/webfetch/bash show typed icon+title.
- Always-stage lists selectors explicitly.
- Existing permission tests pass; replay-mode render unchanged.

**Verification:**

- New deterministic render test with one fixture per permission kind.
- PTY capture next to `session-diff.png` for edit permission visual match.

### [x] T-UI-14 \u00b7 Session list dialog: pin / delete / rename

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-session-list.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-session-rename.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts`

**What is missing:** Picker only resumes; no pin, delete, rename, two-press confirm.

**Target state:**

1. Pin/unpin (TUI-local persistence):
   - Store in `<session-dir>/tui/session-pins.json`.
   - `ctrl+f` toggles pin on the selected row.
   - Render Pinned group first, then normal sessions sorted by updated time.
2. Rename:
   - `ctrl+r` opens a shared input-style dialog whose text is pre-filled with session title.
   - Dialog emits `UiIntent::UpdateSessionTitle { title }`.
   - Coordinator `Command::UpdateSessionTitle` records `SessionTitleUpdated`.
3. Delete:
   - First `ctrl+d` arms the row: row label swaps to \u201cPress ctrl+d again to confirm\u201d.
   - Second `ctrl+d` deletes.
   - Implementation is a CLI-side intent handler (`crates/harness/src/sessions.rs`) that moves the run directory to a sibling `trash/` folder under the session root, reusing session path safety and active/writer-locked checks.
   - Any other key disarms.
   - Failure shows an error dialog.

**Acceptance:**

- Pin reorders and persists across restarts.
- Two-press delete moves session dir to trash.
- Rename round-trips through `SessionTitleUpdated` event.
- No in-place mutation of replay-derived data.

**Verification:**

- Picker render snapshots (pinned group, armed-delete row).
- `harness/src/sessions.rs` trash-move intent handler test with tempdir session corpus.
- Rename integration test.

### [x] T-UI-16 \u00b7 Model / variant / agent dialogs

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-model.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-variant.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-agent.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts`

**What is missing:** No favorites, no `f2` recent cycling, no provider-jump, no variant or agent list dialogs.

**Target state:**

1. Model switcher favorites:
   - `app/model_metadata.rs`: add `favorites: BTreeSet<String>` persisted to `<session-dir>/tui/model-favorites.json`.
   - `ctrl+f` toggles favorite of selected model.
   - Render Favorites group first, then provider-grouped remaining models.
2. Recent-model cycling:
   - `f2`/`shift+f2` cycle recently selected models (derived from persisted recents).
   - Emits `UiIntent::SwitchModel` with a toast showing the new model.
3. Filter ranking:
   - Keep existing subsequence filter but add score boost for prefix and word-boundary matches (e.g., score = prefix_bonus + word_boundary_bonus + subsequence_score).
   - Favorites match at top regardless of score when filter is empty.
4. Variant dialog:
   - New `dialog-variant`-style overlay listing Default + each named variant, marking the current one.
   - Opened by `ctrl+t` when model switcher is not already open (current `ctrl+t` cycles in-place; preserve existing behavior and open the dialog with a different default binding when possible).
5. Agent list dialog:
   - New thin dialog over existing agent-cycle metadata.
   - Opened by `<leader>a`.

**Acceptance:**

- Favorite flag persists and reorders.
- `f2` routes next prompt to selected model.
- Filter ranks `gpt-4` above `some-gpt-4-plugin` for query `gpt`.
- Variant dialog marks current variant.

**Verification:**

- Extend `model_switcher_metadata_test` with scoring table.
- New dialog snapshots.

### [x] T-UI-02 \u00b7 Transcript navigation + display-toggle vocabulary

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx` (`sessionBindingCommands`, global scroll vocabulary)
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts`

**What is missing:** Message-jump family, copy/export commands, scrollbar toggle.

**Target state:**

Add these actions (names reference OpenCode semantics):

| Action | Default binding | Behavior |
|---|---|---|
| `FirstMessage` | `ctrl+g` / `home` (when not composing) | jump to first activity |
| `LastMessage` | `ctrl+alt+g` / `end` | jump to last activity |
| `NextMessage` | `ctrl+alt+n` / `n` (transcript focus) | jump to next message boundary |
| `PreviousMessage` | `ctrl+alt+p` / `p` | jump to previous message boundary |
| `LastUserMessage` | `<leader>u` | jump to the last user message |
| `PageUp` | `pageup` / `ctrl+alt+b` | scroll transcript up one page |
| `PageDown` | `pagedown` / `ctrl+alt+f` | scroll transcript down one page |
| `HalfPageUp` | `ctrl+alt+u` | half-page up |
| `HalfPageDown` | `ctrl+alt+d` | half-page down |
| `LineUp` | `ctrl+alt+y` | one visual line up |
| `LineDown` | `ctrl+alt+e` | one visual line down |
| `ToggleScrollbar` | `<leader>sb` | persistently toggle scrollbar visibility |
| `ToggleTimestamps` | existing toggles menu | keep, make discoverable |
| `ToggleGenericToolOutput` | existing toggles menu | keep, make discoverable |
| `CopyMessage` | `<leader>y` | copy currently selected activity text to clipboard |
| `ExportSession` | `<leader>x` | save transcript text/markdown to file |

Also add display toggle entries in the `<leader>t` toggles dialog where not already present.

**Acceptance:**

- Each binding performs the reference scroll/jump behavior.
- `ToggleScrollbar` persists across TUI restarts via `tui.json`.
- Copy/export use existing clipboard and artifact spill mechanisms.

**Verification:**

- Keybinding integration tests for jump/scroll family.
- PTY capture of transcript scrolling next to OpenCode screenshot.

### [x] T-UI-19 \u00b7 Timeline framing + child-session dialog

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/dialog-timeline.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/dialog-fork-from-timeline.tsx`

**What is missing:** Child-session navigation is keyboard-first but lacks palette discoverability and a timeline dialog.

**Target state:**

1. Add `LineageBrowser`/`Timeline` dialog reached by `<leader>g` (requires leader key).
2. Dialog shows event timeline with compact metadata; can fork/compact from selected event.
3. Child session dialog: when a subagent entry is selected, a thin dialog lists First/Previous/Next/Parent navigation and metadata.

**Acceptance:**

- `<leader>g` opens timeline on live/replay sessions.
- Child navigation remains keyboard-first and is also discoverable in the command palette.

**Verification:**

- New `tui_signoff_manifest_test` entries.
- Snapshot test for timeline dialog.

### [x] T-UI-09 \u00b7 Theme selection dialog

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-theme-list.tsx`
- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/config/keybind.ts`

**What is missing:** Single built-in theme; no `theme` key in `tui.json`.

**Target state:**

1. Add `theme: String` to `PublicTuiConfig` (default `"default"`).
2. Implement at least one additional built-in palette (e.g., dark/high-contrast) as a second `ThemePalette`.
3. `<leader>t` opens theme dialog listing built-in themes.
4. Selection updates `AppState.theme` and writes back to `tui.json`.

**Acceptance:**

- Theme switch is visible in deterministic render snapshot.
- Invalid theme name falls back to default with a warning.

**Verification:**

- New render snapshots for default and alternate theme at same geometry.
- Config test asserting `theme` key is read.

### [x] T-UI-08a \u00b7 Sidebar brand / geometry polish

**Reference files:**

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/sidebar.tsx`
- Parity screenshots.

**What is missing:** Footer brand line, fixed-width consistency.

**Target state:**

1. Render a footer line inside the operator sidebar: bold `HARNESS` branding + short version string (c.f. OpenCode brand+version footer).
2. Keep current fixed-width behavior but align with the reference: right edge spacing, title block order (bold title, session id on non-latest channels, workspace label).
3. Ensure scrollbox scrolls accelerated / works with mouse wheel.

**Acceptance:**

- Sidebar footer line visible in deterministic render test.
- Geometry matches parity screenshot within snapshot tolerance.

**Verification:**

- Updated deterministic render snapshots for sidebar.
- PTY screenshot next to OpenCode sidebar reference.

### [x] T-UI-03 \u00b7 Error-details overlay

**Reference files:**

- Existing error/status banner code in `ui_chrome.rs` and `app/session_projection.rs`.

**What is missing:** No dedicated error overlay surfaced from backend provider failures.

**Target state:**

1. Add `OverlayKind::ErrorDetails`.
2. When a provider request fails with a recoverable category, the TUI can open an error overlay showing category, message, recovery hint, attempt count, and a \u201cResubmit\u201d option.
3. Resubmit emits a coordinator intent to replay/reschedule the turn.

**Acceptance:**

- Error overlay opens from footer/status cluster on any failed provider request.
- Recovery hint from `ProviderErrorCategory` is rendered.

**Verification:**

- Deterministic render test with a mock provider failure.

---

## 9. Implementation order recommendation

Because many UI features depend on the leader key and composer refactor, the safest phased order is:

1. **Prerequisites**
   - T-DOC-01 (docs fragment)
   - T-REF-01 (`ComposerState`)
   - T-REF-02 (overlay single source of truth)
2. **Input foundation**
   - T-UI-10 (leader key)
   - T-UI-11 (composer editing)
3. **Performance**
   - T-PERF-02, T-PERF-01, T-PERF-03 (cache structure, split keys, selection rows)
4. **Runtime safety**
   - T-RT-01 (panic restore)
   - T-RT-02 (mouse-move no-redraw)
5. **Footer + shell mode**
   - T-UI-01 (footer cluster)
   - T-UI-13 (shell mode)
6. **Dialogs**
   - T-UI-17 (permission modal depth)
   - T-UI-14 (session list)
   - T-UI-16 (model/variant/agent)
7. **Navigation + polish**
   - T-UI-02 (transcript nav)
   - T-UI-19 (timeline/child dialog)
   - T-UI-09 (theme dialog)
   - T-UI-08a (sidebar polish)
8. **Wrap-up**
   - T-BE-05 (mock error message)
   - T-RT-03 (reload budget)
   - T-PERF-04 / T-PERF-05 (perf tests)

---

## 10. Addendum: when this file becomes stale

After a task is merged:

1. Mark `[x]` in this file.
2. Update PRD \u00a717 task card with the evidence row (test name/artifact/PR).
3. If a task is deferred or rescoped, append a note in PRD \u00a715 with maintainer reasoning.

This companion doc should be reviewed whenever the parent PRD is revised.
