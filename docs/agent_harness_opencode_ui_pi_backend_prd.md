# Agent Harness PRD: OpenCode-grade Ratatui UX and Pi-inspired Backend Hardening

**Status:** Audit-complete implementation PRD. Produced from a read-only audit of the
`dev` branch on 2026-06-10 (HEAD `d46894ef`); revised the same day to (a) fold in
the detailed OpenCode UI findings from a deeper source comparison and (b) keep
the overall plan balanced across performance, maintainability, UI, backend
hardening, and evidence work. No source files were modified while producing
this document.

**Audience:** A future implementation agent (or agents) executing this plan in
small, reviewable phases.

**Authority:** Subordinate to [`docs/roadmap-v1.md`](roadmap-v1.md) for product
scope and to the root [`AGENTS.md`](../AGENTS.md) invariants for runtime
architecture. Where this PRD proposes new work, it must be implemented without
weakening anything those documents already guarantee.

---

## 0. Read this first

### 0.1 Strict purpose and governing goal

The governing goal of this PRD:

> Make Agent Harness faster, more maintainable, more reliable, and much closer
> to the best inspiration harnesses, while making the TUI as close to OpenCode
> as is feasible for local coding surfaces without compromising Harness
> architecture.

That breaks into five workstreams of comparable weight, plus a baseline gate
and an evidence gate around them:

1. **Baseline integrity** — reconcile the clean-tree snapshot failures and doc
   drift found by the audit, so every later claim is honest.
2. **Transcript performance and rendering-cache hardening** — fix the
   whole-transcript invalidation model so long, streaming-heavy sessions stay
   responsive. This is the first major technical priority after baseline.
3. **TUI state maintainability** — focused, risk-reducing extractions from
   `AppState` and overlay-state unification. Supportive work, not a rewrite.
4. **OpenCode UI workstream** — maximum feasible OpenCode UI/UX parity for the
   selected local-coding TUI surfaces in §6.1/§6.5, adapted to Harness-native
   Rust/Ratatui/event-sourced architecture. One major workstream among five,
   not the governing structure of the plan.
5. **Backend/session/tool/provider hardening** — audited adoptions from Pi,
   OpenCode, OMO, Codex, and Senpi: bounded provider retry, error metadata and
   recovery hints, session rename, queued-turn surfaces, diagnostics — all
   coordinator-owned.
6. **Testing, docs, dogfooding, and evidence** — every workstream lands with
   deterministic coverage, updated docs/schemas, and recorded evidence.

This is **not** a greenfield plan, **not** a rewrite plan, **not** a UI-only
roadmap, and **not** a global OpenCode clone plan. OpenCode is the strongest
UI/UX reference for the TUI workstream; for everything else it is one
inspiration among several. Every task in §17 traces back to a specific audited
finding in §2–§14.

### 0.2 Implementation-agent operating rules

1. Read the root `AGENTS.md` and the crate-scoped `AGENTS.md` for every crate
   you touch **before** the first edit of a phase.
2. Load the mandatory coding skill `karpathy-guidelines` before the first code
   edit, per root `AGENTS.md` §MANDATORY CODING SKILLS. Delegated coding tasks
   must include it in `load_skills`.
3. Work phase by phase (§16). Do not start phase N+1 while phase N acceptance
   criteria are unmet, except for explicitly independent tasks.
4. TDD where the change is behavioral: failing test → smallest correct change →
   evidence row.
5. Honor the UPDATE-TOGETHER table in root `AGENTS.md`: config keys, event
   variants, tool ids, and test-lane behavior changes update their paired
   docs/schemas/tests in the same change.
6. Use the narrowest test lane that proves a change
   (`scripts/test-lanes.sh fast` first; see `docs/testing.md`).
7. Snapshot updates go through `cargo insta review` only after deciding whether
   behavior changed or the fixture drifted (see crate AGENTS anti-patterns).
8. For OpenCode UI workstream tasks: **re-read the cited OpenCode reference
   file and the parity screenshots before implementing.** Do not implement
   reference behavior from memory or from this document alone — this document
   tells you *where to look*, the reference tells you *what it does*.

### 0.3 Non-negotiable invariants (verbatim from the audited tree)

- Events are the source of truth; replay is side-effect free and derives from
  JSONL in contiguous `seq` order.
- The coordinator (`harness-core::coord`) is the only event-append, task
  scheduling, permission resolution, hook, compaction, and lifecycle authority.
- Permission checks precede tool execution; worker redelegation bypasses remain
  blocked.
- Hashline edits validate anchors, reject overlaps, apply bottom-up, and write
  atomically. Hashline editing stays the normal file-changing path.
- Provider-context compaction writes checkpoint artifacts/events and never
  rewrites `events.jsonl`.
- Session inspection tools read replay-derived data only.
- Provider metadata persisted to events/artifacts must be redacted; never store
  raw requests/responses, auth headers, cookies, keys, PEM blocks, or hidden
  reasoning text.
- Runtime config (`harness.json{,c}`) and TUI config (`tui.json{,c}`) are
  separate public contracts.
- Replay mode in the TUI is read-only and must not emit live submission intents.
- `inspirations/` is read-only reference material; copy observable behavior,
  never source code, package layout, or branding.

### 0.4 Anti-gaming rules

- Do not weaken, delete, `#[ignore]`, assert-loosen, or snapshot-rubber-stamp
  any test to reach green. The two deterministic render snapshot failures found
  on the clean tree (§2.3) must be resolved by deciding behavior-vs-fixture, not
  by blind `--accept`.
- Do not check off a task card in §17 without running its listed verification
  commands and recording the result.
- Do not declare a UI parity task done by implementing a vaguely "similar"
  surface, and do not silently drift from the reference either: for selected
  parity targets, differences from OpenCode are allowed only when documented in
  §6.5 and justified by Harness architecture, scope, or safety.
- Do not introduce new caching that cannot prove correctness: every new cache
  needs an invalidation test that mutates each key component and asserts a
  rebuild, plus a no-change test that asserts a cache hit.
- Do not claim performance improvements without a before/after measurement on
  the long-session scenarios in §10.7.
- Do not silently expand scope into §5 non-goals.

### 0.5 Definition of "done"

This PRD is complete if and only if, simultaneously on the working branch:

1. All P0 and P1 task cards in §17 are implemented with their acceptance
   criteria verified, or explicitly re-scoped in a documented decision appended
   to this file's §15 with maintainer-visible reasoning. P2 cards may be
   deferred but must be dispositioned (done/deferred) in writing.
2. `scripts/test-lanes.sh fast`, `quality-gates`, and `all-deterministic` pass
   with zero failures.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   passes.
4. The final acceptance criteria in §18 hold — performance budgets,
   maintainability outcomes, backend hardening, docs, **and** the scoped UI
   comparison in §18.4.
5. Docs named in §15 are updated together with the code that changed them.
6. A dogfooding evidence note (§18.7) is recorded.

### 0.6 OpenCode UI workstream parity bar (scope: TUI workstream only)

For the TUI surfaces selected in §6.1/§6.5, the quality bar is **maximum
feasible parity with OpenCode's local-coding UI/UX**, reimplemented natively:

- **Reference:** the local checkout under
  `inspirations/opencode/packages/opencode/src/cli/cmd/tui/` (components,
  routes, `config/keybind.ts`) plus the parity screenshots in
  `inspirations/screenshots opencode ui parity/` and
  `inspirations/opencode-ui-images/`. OpenCode is the strongest UI/UX
  reference for these surfaces; this PRD selects which surfaces and records
  the adaptations.
- **Parity means:** for a selected surface, the same screen composition,
  interaction flow, default keybindings (where the underlying feature exists),
  information density, and visual hierarchy — with Harness branding and
  Harness theme tokens, and with copy *shape* preserved.
- **Adaptation is expected:** OpenCode behavior must be adapted to the
  Harness-native Rust/Ratatui/event-sourced architecture. Where Harness
  semantics are stronger (durable event-recorded permission grants,
  replay-derived state, stable fork cutoffs), keep the stronger semantics and
  match the *presentation*. Every intended difference is recorded in §6.5.
- **Selection, not totality:** only §6.5 *selected parity target* and *adapted*
  rows are in scope. Excluded OpenCode features (cloud/share, plugins,
  snapshots-based undo, multimodal paste, dev tooling) must remain excluded.
- **Conflict rule:** when the reference conflicts with a §0.3 invariant,
  coordinator authority, replay safety, or roadmap scope, Harness wins and the
  adaptation is recorded. When this PRD's *description* of an in-scope surface
  disagrees with the reference source, re-read the reference and record which
  was wrong — do not invent a third behavior.
- **Verification:** selected parity-now surfaces are compared against the
  reference (source + screenshots) at matching geometry during Phase 7's
  review (§18.4); differences are documented and justified, not discovered by
  users.

### 0.7 Missing-specs companion document

A task-by-task breakdown of everything in this PRD that is not yet implemented,
together with per-task OpenCode reverse-engineering targets and the recommended
implementation order, lives in
[`agent_harness_opencode_ui_pi_backend_prd_missing_specs.md`](./agent_harness_opencode_ui_pi_backend_prd_missing_specs.md).
Every task card in §17 should be implemented as a standalone cycle with its own
failing test, smallest correct change, and evidence row. UI/UX tasks in that
companion document are specified for maximum feasible OpenCode parity and must be
verified against the OpenCode reference (source + screenshots) at matching
terminal geometry before they are considered complete.

---

## 1. Executive summary

Agent Harness is a mature, release-hardened Rust workspace, not a greenfield
project. The event-sourced coordinator, permission model, hashline editing,
provider OAuth, compaction, session lineage, and a large deterministic test
estate all exist and are documented accurately in `docs/architecture.md` and
the crate `AGENTS.md` files. Dogfooding confirms: `config validate` and
`doctor` pass (12/12 checks), the deterministic `golden_path` scenario runs and
replays, and the TUI renders a compose-first startup shell, a working command
palette with categories and keybinding hints, and a read-only replay shell with
tool rows and an operator sidebar.

The audit found the highest-impact work in five areas, of comparable weight:

1. **Transcript rendering cost model (top technical priority after
   baseline).** The TUI has real caching (`TRANSCRIPT_LAYOUT_CACHE`,
   selection-snapshot cache, render-key memo), but the cache key is
   monolithic: every ingested event bumps a global epoch, and the key
   additionally hashes the spinner animation phase, the hovered mouse target,
   and the **full text of every activity**. Consequence: during streaming,
   every delta/frame — and even every 100 ms spinner tick and every hover
   change while idle — rebuilds and re-measures the *entire* transcript and
   rehashes all transcript text. The fix (per-activity revisions,
   section-level caching, measure/decoration key split, compact selection
   snapshot) is well-localized in `crates/harness-tui/src/ui_transcript*.rs`
   and `crates/harness-tui/src/app/transcript_state.rs`, and is a prerequisite
   for any further UI surface growth.

2. **TUI state maintainability.** `AppState`
   (`crates/harness-tui/src/app.rs:184`) is a ~130-field flat struct. Behavior
   is already well-split into `app/` submodules, so this is *not* a rewrite
   candidate — but overlay/modal visibility is tracked by ten independent
   booleans alongside a separately maintained `OverlayStack`, and
   permission/question modal state is ~15 loose fields. Focused sub-state
   structs, extracted only where they reduce risk for active work, will cut
   precedence bugs and make key handling testable per surface.

3. **OpenCode UI workstream.** A file-level comparison against the OpenCode
   source under `inspirations/` shows the Harness TUI is *close in skeleton* —
   compose-first home, transcript-first session shell, sidebar, palette,
   permission modal, diff review all exist — but has concrete gaps in
   vocabulary and finish: no leader-key scheme or OpenCode-style default
   keymap (`config/keybind.ts` defines ~190 bindable commands; Harness has
   ~40 single-chord actions), thin composer editing (no selection, word/line
   ops, or undo), no shell mode / prompt stash / queued-prompt UX (the
   coordinator already supports queued turns), session list without
   pin/rename/delete, model dialog without favorites/recents cycling, a
   permission modal without typed titles or an embedded edit diff, no
   footer status cluster, and missing transcript navigation vocabulary. These
   are scoped as one workstream with a binding decision table (§6.5).

4. **Backend reliability hardening (Pi-inspired, coordinator-owned).** The
   highest-value adoption is Pi's bounded automatic retry for transient
   provider failures with operator-visible lifecycle — Harness currently has
   none (only the one-shot overflow-compaction retry). Supporting items:
   provider `retry_after_ms` metadata, exposed error recovery hints, a
   coordinator `UpdateSessionTitle` command (which also makes the roadmap's
   "editable titles" claim true), queued-turn list/remove surfaces if the TUI
   needs them, and actionable mock-fixture-miss diagnostics.

5. **Baseline integrity and evidence.** The clean `dev` tree currently fails
   two deterministic render snapshots (`command_palette_renders_without_pty`,
   `tool_lifecycle_rows_stay_ordered_without_pty`) — committed snapshot drift
   that must be reconciled first — and `docs/architecture.md` /
   `docs/roadmap-v1.md` contain orphaned sentence fragments. Every workstream
   ends in deterministic tests, doc updates, and recorded evidence.

Everything else audited — coordinator lifecycle, event schema, store recovery,
permission flow, tools, providers, testkit lanes — is in good shape and should
be **preserved**, with targeted hardening listed in §12–§14.

---

## 2. Evidence log

### 2.1 Project files inspected (first-party)

Guidance and docs (read in full unless noted):

- `AGENTS.md` (root), `crates/harness/AGENTS.md`, `crates/harness-core/AGENTS.md`,
  `crates/harness-tools/AGENTS.md`, `crates/harness-tui/AGENTS.md`
- `docs/architecture.md`, `docs/roadmap-v1.md`, `docs/config.md`,
  `docs/testing.md`, `docs/sessions-and-replay.md`,
  `docs/native-tool-catalog.md`, `docs/pre-v1-enhancements-prd.md` (§0–§3 in
  full, §4–§10 outline), `docs/pre-v1-enhancements-progress.md`,
  `docs/release-blockers.md` (head), `docs/v1-roadmap-claim-correction-prd.md`
  (head), `README.md`
- Root `Cargo.toml` and all six crate `Cargo.toml` files

Source (read in full or in targeted depth):

- harness-tui: `src/app.rs`, `src/app/transcript_cache.rs`,
  `src/app/transcript_state.rs` (cache-key derivation),
  `src/app/session_projection.rs` (struct + memory caps),
  `src/app/lifecycle.rs` (Focus/UiIntent/shell enums), `src/app/mouse_interaction.rs`
  (entry), `src/app/prompt_input.rs` (insert/submit paths), `src/ui_transcript.rs`,
  `src/ui_transcript_layout.rs`, `src/runtime.rs`, `src/event.rs`,
  `src/keybindings.rs` (Action enum + metadata ids), `src/ui.rs`
  (`render_app`), `src/layout.rs` / `src/theme.rs` (breakpoints, sidebar width
  threshold), `src/ui_permission_dock.rs` (grep: no diff/preview rendering),
  file-size census of all 100+ modules; negative greps for
  shell-mode/stash/leader/pin/favorite/queue-in-composer
- harness-core: `src/coord.rs` (module map, `CoordinatorConfig`, `Command`
  enum), `src/coord/state.rs` (`QueuedAgentTurn`, `queue_agent_turn`),
  `src/event.rs` (`EventV1` variants), `src/store.rs` (writer lock, append,
  crash-tail recovery surface), `src/session_title.rs`, `src/agent.rs`
  (streaming exports), file-size census of `coord/`, `proj/`, `event/`, `store/`
- harness-providers: `src/lib.rs` (`CompletionRequest`,
  `ProviderErrorCategory`, `ProviderStreamEvent`, `Provider` trait,
  `ProviderRouter`), retry-logic grep across `openai.rs`/`openai/*`
- harness-tools: `src/workspace_paths.rs`, `src/tool_catalog.rs` surface,
  file-size census; tool behavior cross-checked against
  `docs/native-tool-catalog.md`

### 2.2 Inspiration files inspected (read-only, local)

OpenCode TUI (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/`):

- `routes/home.tsx` (full): logo + centered prompt, per-mode placeholder
  lists, auto-submit of `--prompt`, footer slot.
- `routes/session/index.tsx` (2,572 lines; structure + targeted reads): the
  session command vocabulary (`sessionBindingCommands` — share, rename,
  timeline, fork, compact, unshare, undo, redo, sidebar/conceal/timestamps/
  thinking/actions/scrollbar/generic-tool-output toggles, first/last,
  message next/previous, last-user-message, copy, export, child
  first/next/previous/parent) and the global scroll vocabulary
  (page/line/half-page up/down); sticky-bottom follow
  (`stickyScroll`/`stickyStart="bottom"`); message-boundary jump
  implementation; scrollbar visibility toggle persisted via kv signal.
- `routes/session/sidebar.tsx` (full): fixed width 42, panel background,
  title block (bold title, session id on non-latest channels, workspace
  label, share URL), scroll-accelerated scrollbox, brand+version footer line.
- `routes/session/footer.tsx` (full): left cwd; right cluster — pending
  permission count with `△` warning glyph, LSP count with status dot, MCP
  count with `⊙` glyph (error-colored on any failed server), `/status` hint;
  disconnected-state "Get started /connect" rotation.
- `routes/session/permission.tsx` (729 lines; targeted reads): three-stage
  flow (`permission` → `always` → `reject`), per-permission icon+title
  builders (Edit with embedded diff component, Read/Glob/Grep/List/Task/
  WebFetch/external-directory/doom-loop/generic tool), "Allow once / Allow
  always / Reject" options, Esc = reject, fullscreen prompt layout, "always"
  stage explains scope and lists the exact patterns being granted.
- `component/prompt/index.tsx` (3,186-line component dir; targeted reads):
  shell mode entered by typing `!` at column 0 (placeholder set swaps,
  Esc or backspace-at-0 exits, submits through a shell endpoint), prompt
  stash commands (`prompt.stash`, `.pop`, `.list` + `DialogStash`), rotating
  random placeholders per mode, image/SVG paste handling, history,
  autocomplete, frecency.
- `component/command-palette.tsx` (full): reachable-command query from the
  keymap, per-row title/description/category/keybinding footer, dynamic
  "Suggested" category when filter is empty.
- `component/dialog-session-list.tsx`: pinned section first, pin/unpin via
  `session.pin.toggle` (ctrl+f), delete with two-press confirm (title swaps
  to "Press ctrl+d again to confirm", spinner while deleting), rename action
  opening `DialogSessionRename`, relative-updated-time footers, quick-switch
  footer hints.
- `component/dialog-model.tsx`: fuzzysort-based search, provider grouping,
  favorites toggle (ctrl+f), provider list jump (ctrl+a), recent models.
- `component/dialog-variant.tsx`: "Default" + named variants with current
  marker.
- `config/keybind.ts` (full): leader key default `ctrl+x`; ~190 command
  defaults including `<leader>m` models, `<leader>l` sessions, `<leader>n`
  new, `<leader>s` status, `<leader>b` sidebar, `<leader>c` compact,
  `<leader>g` timeline, `<leader>x` export, `<leader>y` copy message,
  `<leader>t` themes, `<leader>a` agents, `<leader>e` external editor,
  `ctrl+r` rename, `ctrl+p` commands, `tab`/`shift+tab` agent cycle,
  `ctrl+t` variant cycle, `f2`/`shift+f2` recent-model cycle, `escape`
  interrupt, `pageup/pagedown` + `ctrl+alt+{b,f,u,d,y,e}` scroll family,
  `ctrl+g`/`home` first message, `ctrl+alt+g`/`end` last; the full
  `input_*` editing vocabulary (move/select by char/word/line/visual-line/
  buffer, delete word/line/to-line-ends, undo `ctrl+-`, redo `ctrl+.`,
  select-all, newline variants, clear).
- UI primitives inventory: `ui/dialog-select.tsx`, `ui/toast.tsx`,
  `ui/dialog-help.tsx`, `component/spinner.tsx`, `component/startup-loading.tsx`,
  `component/todo-item.tsx`, `component/bg-pulse.tsx`,
  `routes/session/dialog-timeline.tsx`, `dialog-fork-from-timeline.tsx`,
  `dialog-subagent.tsx`, `subagent-footer.tsx`, `dialog-status.tsx`,
  `dialog-theme-list.tsx`, `dialog-agent.tsx`, `dialog-skill.tsx`,
  `dialog-stash.tsx`.

Parity screenshots: `inspirations/screenshots opencode ui parity/Opencode/*`
(start screen, chat example 1, commands menu) and `.../Harness project/*`
(current start screen, chat example 1, live_proxy captures);
`inspirations/opencode-ui-images/session-diff.png` (side-by-side diff with
line numbers, per-file `+/-` counts in sidebar, context token/cost block).

Pi: `inspirations/pi_agent_rust/src` inventory (`session_index.rs` API,
`compaction.rs` semantic-marker types), `inspirations/pi-mono/packages/coding-agent/src/core`
inventory, `core/agent-session.ts` auto-retry state machine
(`auto_retry_start`/`auto_retry_end`, `_retryAttempt`, exponential
`baseDelayMs * 2^(attempt-1)`, abort-aware sleep, reset on success).

Codex: `inspirations/codex/codex-rs/tui/src` inventory (markdown_stream,
diff_render, bottom_pane, key_hint, snapshot rigor).

OMO/Senpi/shuvcode: README-level identification (`oh-my-openagent` = heavy
OpenCode-based orchestration harness; `senpi` = light OMO on the pi-mono
runtime with builtin intent-gate/todo/compaction/prompt-preset extensions;
`shuvcode` = OpenCode fork). Used as corroboration that Harness's intent gate,
hashline editing, category routing, and skill systems already cover the
transferable ideas.

### 2.3 Commands, tests, and dogfooding performed

| Command | Result | What it changed in this PRD |
|---|---|---|
| `./target/debug/harness --config configs/harness.example.jsonc config validate` | PASS (merged repo `tui.jsonc`) | Confirms config path; no action |
| `./target/debug/harness --config configs/harness.example.jsonc doctor` | PASS 12/12, redacted OAuth status, "local readiness only" scope line | Doctor surface is release-grade; excluded from new scope |
| `harness run --mock "Hello from PRD audit"` | FAIL (exit 1): `mock fixture missing for request_digest=…` | Spawned DX task T-BE-05 (actionable mock-miss error) |
| `harness run --mock "Hello from PTY"` | PASS ("Hello world") | Confirms README path accurate as written |
| `harness run --scenario golden_path --deterministic` + `harness replay` | PASS; 26 events; replay summary with next_steps and resume-block reason | Confirms replay surfaces; informed §7 |
| PTY capture of `harness tui --replay <golden_path run>` (script(1), 120×40) | Renders header `Replay · read-only · run … · 26 ev`, tool rows with durations, sidebar (MCP/LSP/Modified Files `demo.txt +1 -1`), footer `Replay is read-only. ? shortcuts · ctrl+tab focus · r reload · q quit` | Confirms replay shell quality; informed §6.1/§7 |
| PTY capture of `harness tui` startup + `Ctrl+p` | Renders HARNESS logo, composer-first home with placeholder, agent/model/variant row inside composer, hints `ctrl+t variants · tab agents · ctrl+p commands`, footer with cwd; palette shows Suggested/Sessions/Agents/System categories with per-row keybinding hints | Start-screen skeleton parity already achieved; §6.1 scopes the remaining vocabulary/finish gaps; flagged stale parity screenshot |
| `cargo test -p harness-tui --test deterministic_render_test` | **FAIL on clean tree**: 7 passed, 2 failed (`command_palette_renders_without_pty` — committed snapshot lacks the live composer placeholder line `Type a prompt for the next turn…`; `tool_lifecycle_rows_stay_ordered_without_pty`) | Created P0 task T-TEST-01 (Phase 1); `.snap.new` artifacts deleted to leave tree clean |
| Negative greps in harness-tui (`shell_mode`, `stash`, `leader`, `pin`, `favorite`, queue-in-composer, diff in `ui_permission_dock.rs`) | No hits | Established the §6.1 gap matrix |
| `grep QueuedAgentTurn crates/harness-core/src/coord/state.rs` | Present (`queued_agent_turns`, `queue_agent_turn`) | Prompt-queue parity classified as TUI-side work (T-UI-12) |

Dogfooding limits: no live provider call was made (no credentials should be
exercised by an audit), no interactive permission modal was driven end-to-end
in a PTY (covered by existing deterministic tests
`permission_modal_preempts_palette_and_slash`,
`question_permission_prompt_renders_without_pty`), and the model switcher was
not opened in the PTY capture (covered by `model_switcher_metadata_test`).

---

## 3. Current architecture map

| Path | Current responsibility | Relevant invariants | Current risks | Recommendation |
|---|---|---|---|---|
| `crates/harness/src/lib.rs`, `main.rs`, `cli_io.rs` | Clap surface, command dispatch, injectable `CliIo`/`CliDeps` | `main.rs` stays thin; replay/session commands never execute live work | Low | **Keep** |
| `crates/harness/src/bootstrap.rs`, `runtime_catalog.rs` | Provider/model/profile assembly before core runtime | Canonical config contract lives in core/docs/configs | Low | **Keep** |
| `crates/harness/src/tui.rs`, `src/tui/` | CLI→TUI handoff: launch metadata, live update channel, auth backend bridge | Not TUI rendering ownership | Low | **Keep**; document the `LiveUpdate` channel contract (T-DOC-02) |
| `crates/harness-core/src/coord.rs` + `coord/` (17 submodules) | Single scheduling/event/permission/hook/compaction/lifecycle authority; `Command` enum at `coord.rs:296`; queued turns in `coord/state.rs` | All §0.3 invariants | `coord.rs` is large by design (per crate AGENTS); fine | **Keep / harden** per §12 |
| `crates/harness-core/src/event.rs` + `event/` | `EventEnvelopeV1`, 33 payload variants, builders | Additive serde-defaulted metadata; new variants need docs+drift tests | Low | **Keep**; prefer metadata fields over new variants for retry telemetry (§12) |
| `crates/harness-core/src/store.rs` | JSONL store, writer lock, crash-tail recovery (truncate-one-partial-line, normalize missing newline) | Append-only, contiguous seq | Low | **Keep** |
| `crates/harness-core/src/proj.rs`, `proj/`, `transcript_projection.rs`, `conversation.rs` | Pure replay projections (run/resume/catalog/background/transcript/provider context) | Never call providers/tools/hooks/network | Low | **Keep** |
| `crates/harness-core/src/session_lineage.rs` + impl, `session_title.rs`, `session_paths.rs` | Fork/clone materialization contract; hidden title agent; storage layout | Fork/clone validate artifacts, regenerate ids, clear correlation | Low | **Keep**; TUI rename rides existing `SessionTitleUpdated` (T-UI-06) |
| `crates/harness-providers/src/lib.rs` | `Provider` trait, `CompletionRequest` (+`ProviderRequestContext` cache key), `ProviderStreamEvent`, `ProviderErrorCategory` (8 categories with recovery hints), `ProviderRouter` | Redacted metadata; cassette replay-only | **No transport retry/backoff anywhere** (grep-verified) | **Harden** (§13): keep transport single-shot; retry policy belongs to the coordinator |
| `crates/harness-providers/src/openai.rs` + `openai/` | SSE transport, request building (cache key, reasoning effort, auth decoration), error mapping | No secrets persisted | Low | **Keep** |
| `crates/harness-providers/src/mock.rs`, `cassette.rs` | Digest-keyed fixtures; replay-only cassettes | Deterministic lanes never hit network | Mock-miss error is unactionable for new users | **Document/harden** (T-BE-05) |
| `crates/harness-tools/*` | 31-entry native registry; schemas, path safety (`workspace_paths.rs` canonicalize + prefix check), artifact spill, parity tests | Enforce, don't re-decide, permissions; no ad hoc path joins | Low | **Keep** |
| `crates/harness-tui/src/app.rs` (707 lines, `AppState` ~130 fields) + `app/` (29 modules) | All interactive state; `Deref`/`DerefMut` into `SessionProjection` | Compose-first home; replay read-only; avoid widening app.rs | Flat field soup; ten independent overlay-visibility booleans coexisting with `OverlayStack`; modal state scattered | **Split state into focused sub-structs** (§9), no behavior change, gated per §9.0 |
| `crates/harness-tui/src/app/session_projection.rs` | Event ingestion → events/activities/orchestration/permissions; memory caps with trim counters | UI memory caps are presentation-only, never compaction | Low | **Keep**; add per-activity revision counters (§10.3) |
| `crates/harness-tui/src/ui_transcript.rs` + `ui_transcript_*` (≈5,400 lines) | Section building, measurement, surfaces, selection grid, scrollbar, interaction targets; thread-local 4-entry layout cache | Measured layout / cache keys / selection model must stay coherent | Whole-transcript invalidation per event/animation-tick/hover (§10.1–10.5) | **Harden caching**; preserve module split |
| `crates/harness-tui/src/ui_tool_*`, `ui_diff*`, `ui_syntax_highlight.rs` | Tool rows, metadata, outputs, inline + side-by-side diffs (syntect + imara-diff) | Approved stack: syntect, imara-diff | Low | **Keep**; polish per §6.1 |
| `crates/harness-tui/src/runtime.rs` | Sync event loop; drain budget 16 events / 8 ms; 100 ms active poll; animation cadence decoupled from redraw; preserved-terminal handoff; full crossterm setup/teardown with fallback | Replay mode never submits | Per-event ingest bumps the global render epoch (cost lands in render); no panic-unwind terminal restore | **Harden** (§11) |
| `crates/harness-tui/src/event.rs` | Poll + normalize; coalesces resize and mouse-move/drag bursts with a one-slot stash | — | Low | **Keep** |
| `crates/harness-tui/src/layout.rs`, `theme.rs` | Breakpoints (`PERSISTENT_SIDEBAR_MIN_WIDTH=121`, `DIFF_SIDE_BY_SIDE_MIN_WIDTH=120`, lifecycle geometry tiers), full token-family theme | Geometry only in layout/theme | Single built-in palette; no `tui.json` theme key; no leader-key support in `KeyMap` | **Harden** per UI workstream tasks (T-UI-10, T-UI-09) |
| `crates/harness-tui/src/keybindings.rs` + `keybindings/command_registry.rs` | `Action` enum (~40 variants), `KeyMap` single-chord overrides, centralized palette command metadata | New bindings registered through configurable defaults | Vocabulary much smaller than OpenCode's; no leader sequences; several actions lack `metadata_id()` | **Harden** (T-UI-10, T-UI-04) |
| `crates/harness-testkit` | Deterministic fakes, simulation evidence, PTY/live/native lanes | Provenance classes never conflated | Low | **Keep** |

---

## 4. Product target

The user-visible end state this PRD drives toward:

1. **Fast under load.** A 500-message transcript with one active streaming
   message re-measures only the active section per delta; spinner ticks and
   hover changes never re-measure the transcript; selection over a long
   transcript does not allocate a full per-cell `String` grid per rebuild.
2. **Maintainable TUI internals.** Overlay visibility has one source of
   truth; modal/composer/transcript-view state live in focused structs; new
   surfaces land in focused modules instead of widening `app.rs`.
3. **An OpenCode-caliber local coding TUI.** For the selected local-coding
   surfaces (§6.5): OpenCode's interaction vocabulary (leader keymap, composer
   editing, shell mode, stash/queue), dialog finish (session list, model/
   variant/agent/theme), permission modal depth (typed titles, embedded edit
   diff, staged allow/reject), transcript navigation, footer status, and
   sidebar polish — implemented Harness-native, with documented adaptations.
4. **Harness-native Rust/Ratatui implementation.** All UI work uses the
   existing theme-token and layout-contract systems; no new UI framework, no
   source-ported components.
5. **Event-sourced sessions and replay as product strengths.** Replay, resume,
   fork/clone, lineage tree, and session tools keep working unchanged; every
   new operator surface renders from replay-derived state; hashline editing
   stays the normal edit path, feeding the diff renderer and the permission
   modal's new diff preview.
6. **Clear permissions.** The permission modal stays the single approval
   surface and gains typed titles, the embedded edit diff, and an explicit
   "always allow" explanation naming the recorded selectors; pending count
   surfaces in the footer so a paused turn is never missed.
7. **Reliable provider behavior.** Transient provider failures retry within
   coordinator-owned bounds with durable, redacted attempt metadata;
   failures are inspectable (error overlay with category + recovery hint) and
   manually resubmittable.
8. **Reliable subagent/child session behavior.** Background children keep
   coordinator-owned wakeups and replay-projected status; child navigation
   stays keyboard-first and gains palette discoverability.

---

## 5. Non-goals and scope guards

- **No full rewrite** of `AppState`, the transcript renderer, the coordinator,
  or any crate. Extractions are mechanical and behavior-preserving.
- **No global OpenCode clone.** OpenCode parity applies to the §6.5-selected
  TUI surfaces only; the PRD's other workstreams are not subordinate to it.
- **No copying source code or architecture from inspirations.** SolidJS
  reactive patterns, OpenCode's plugin-slot system, Pi's extension host, and
  Codex's app-server are all out of scope; parity is *observable behavior and
  visual design*, reimplemented natively (§0.6).
- **No moving coordinator authority out of `harness-core`.** The TUI never
  appends events, resolves permissions locally, retries providers itself, or
  executes tools. Shell-mode (`!`) submissions go through the coordinator's
  normal `bash` tool permission path, not a TUI-side shell.
- **No making replay effectful.** No new replay path may schedule provider
  work, execute tools, or mutate sessions; rename/pin/queue surfaces are live
  intents handled by the coordinator (or local TUI state files where §6.5 says
  so) and are unavailable or read-only in replay mode.
- **No arbitrary plugin host.** The descriptor-only extension manifest stays
  descriptor-only (roadmap: post-V1).
- **No replacing hashline editing** with patch-grammar or regex-based editing.
- **No cloud/share/account surfaces** even though OpenCode ships them
  (share/unshare, `/connect`, console-org, workspaces) — §6.5 excludes them;
  affected footer/dialog regions substitute Harness equivalents.
- **No weakening tests/snapshots.** Snapshot churn is resolved by review, and
  every accepted change is justified as behavior or fixture drift.
- **No broad agent-OS parity** (Team Mode worktrees, Ralph loop, swarm
  ledgers, todo enforcers, browser/media automation) — post-V1 per
  `docs/roadmap-v1.md`.
- **No new event schema variants** unless §12/§13 explicitly justifies one;
  prefer optional metadata on existing barriers per `docs/architecture.md`.
- **No second theme engine.** Theme work (T-UI-09) reuses the existing
  `Theme` token families and adds selection via `tui.json`.
- **Runtime config and TUI config stay separate public contracts**; new TUI
  settings (keybinds incl. leader, theme) belong to `tui.json{,c}` only.

---

## 6. Inspiration comparison

### 6.1 OpenCode UI workstream: parity targets for selected surfaces

> Scope note: this matrix is the **TUI workstream** of the PRD, not the whole
> plan. Classification (P1 selected parity target / P2 later polish /
> adapted / excluded) is finalized in the §6.5 decision table.
>
> Baseline note: the parity screenshot
> `inspirations/screenshots opencode ui parity/Harness project/Harness current start screen.png`
> is **stale**. The PTY dogfood (§2.3) shows the current startup shell already
> matches OpenCode's home *skeleton*. Entries below are the audited remaining
> gaps, ordered roughly by how much of the UX identity they carry. Each
> follows: reference behavior → inspected path → current Harness → gap →
> Ratatui-native target → files → acceptance → tests.

**U1. Keybinding scheme: leader key + OpenCode-like default keymap. [P1]**
- Reference behavior: OpenCode's keymap (`config/keybind.ts`) is built around
  a leader key (default `ctrl+x`) followed by a mnemonic: `<leader>m` models,
  `<leader>l` session list, `<leader>n` new session, `<leader>s` status,
  `<leader>b` sidebar toggle, `<leader>c` compact, `<leader>g` timeline,
  `<leader>x` export, `<leader>y` copy message, `<leader>t` themes,
  `<leader>a` agents; direct chords for high-frequency commands: `ctrl+p`
  palette, `tab`/`shift+tab` agent cycle, `ctrl+t` variant cycle,
  `f2`/`shift+f2` recent-model cycle, `ctrl+r` rename, `escape` interrupt; a
  scroll family (`pageup/pagedown`, `ctrl+alt+b/f`, `ctrl+alt+u/d` half-page,
  `ctrl+alt+y/e` line, `ctrl+g`/`home` first, `ctrl+alt+g`/`end` last). Every
  command is rebindable; multiple bindings per command are comma-separated.
- Inspected: `config/keybind.ts` (full), `component/command-palette.tsx`
  (bindings rendered as row footers).
- Current Harness: `KeyMap` supports single-chord bindings with `tui.json`
  overrides; ~40 `Action` variants; no key-sequence (leader) support; some
  OpenCode defaults coincidentally match (`ctrl+p`, `tab`/`shift+tab`,
  `ctrl+t`).
- Gap: the keybinding scheme carries OpenCode's interaction identity; without
  leader sequences Harness cannot mirror the default map.
- Target: extend `KeyMap` to support two-step sequences (leader + key) with a
  configurable leader (default `ctrl+x`), a pending-leader state with a brief
  footer hint (e.g. `ctrl+x …`), timeout/escape to cancel, and
  multi-binding-per-action support. Ship the OpenCode default map for every
  action Harness has (existing + new from this PRD), preserving current
  Harness bindings as *additional* bindings where non-conflicting. `tui.json`
  `keybinds` accepts `<leader>` syntax in values.
- Files: `crates/harness-tui/src/keybindings.rs`, `keybindings/`,
  `app/key_interaction.rs` (dispatch), `configs/tui.example.jsonc`,
  `docs/config.md` TUI section, `configs/tui.json` schema.
- Acceptance: `ctrl+x` then `m` opens the model switcher; `ctrl+x` then an
  unbound key cancels with no side effect; leader is rebindable; shipped
  defaults match `config/keybind.ts` for commands Harness supports (table
  recorded in docs and drift-tested); palette/help rows show leader bindings
  in OpenCode's display form (`ctrl+x m`).
- Tests: `keybindings/tests.rs` sequence dispatch + rebind tests; palette
  snapshot (one reviewed update); help overlay render test.

**U2. Composer/input editing vocabulary. [P1]**
- Reference behavior: OpenCode's input supports cursor movement by
  char/word/line/visual-line/buffer; **selection** variants of each
  (shift+arrows, shift+home/end, select-all); delete word forward/back
  (`alt+d`/`ctrl+w`), delete line, delete to line start/end
  (`ctrl+u`/`ctrl+k`); undo (`ctrl+-`)/redo (`ctrl+.`); newline via
  `shift+enter`/`ctrl+enter`/`alt+enter`/`ctrl+j`; clear (`ctrl+c`); history
  up/down at buffer edges.
- Inspected: `config/keybind.ts` `input_*` block; `component/prompt/index.tsx`.
- Current Harness: char-wise cursor, backspace/delete, history with draft
  preservation, file mentions; no selection, no word/line ops, no undo/redo.
- Gap: daily-driver typing ergonomics.
- Target: implement the `input_*` vocabulary on the Harness composer:
  selection model (anchor + cursor, rendered via theme selection style),
  word/line/buffer motions and their selecting variants, kill operations,
  bounded undo/redo stack (text+cursor+selection), select-all, the newline
  binding set, clear. Unicode-safe (grapheme-aware) boundaries. All via new
  `Action` variants registered in the command registry (input commands are
  not palette rows, matching the reference).
- Files: `app/prompt_input.rs` (grows; consider sibling
  `app/prompt_selection.rs` per the avoid-widening rule), `keybindings.rs`,
  `ui_composer.rs` (selection rendering).
- Acceptance: each binding in the `input_*` table performs the reference
  behavior on a multi-line buffer with wide chars; undo after word-delete
  restores text+cursor; history navigation still preserves drafts (existing
  tests stay green); file-mention tag offsets stay correct across
  word-deletes (extend `adjust_file_mention_tags_*` coverage).
- Tests: unit tests in `app/tests/prompt_input_tests.rs`; composer render
  snapshot with active selection.

**U3. Shell mode (`!` prefix). [P1, adapted]**
- Reference behavior: typing `!` in an empty prompt enters shell mode — the
  composer placeholder swaps to shell examples (`ls -la`, `git status`,
  `pwd`), Esc or backspace-at-column-0 exits, and submission executes the
  command as a shell call attributed to the session rather than a model turn.
- Inspected: `component/prompt/index.tsx` (mode store, placeholder swap,
  enter/exit bindings, shell submission); `routes/home.tsx` placeholders.
- Current Harness: no shell mode (grep negative); bash runs only as a
  model-initiated tool.
- Gap: operators can't run a quick command without leaving the TUI.
- Target (Harness-adapted, invariant-preserving): shell mode is a composer
  state that routes submission through the **coordinator's existing native
  `bash` tool path** — a `UiIntent::RunShellCommand { command }` handled by
  the CLI side as a coordinator `RequestToolCall` with the operator actor, so
  the normal `bash` permission policy, allowlist, blocked-command hints,
  output caps, artifacts, and events all apply, and the call renders as a
  standard bash tool row in the transcript. Visuals match the reference:
  distinct composer accent + placeholder set while in shell mode;
  Esc/backspace-at-0 exits. Disabled in replay mode; on the startup shell
  (no active run) entering `!` is rejected with a toast (adaptation recorded
  in §6.5).
- Files: `app/prompt_input.rs` (mode), `ui_composer.rs` (accent/placeholder),
  `app/lifecycle.rs` (intent), `crates/harness/src/tui/` (intent → coordinator
  wiring), `theme.rs` (placeholder copy tokens).
- Acceptance: `!` at column 0 enters shell mode with swapped placeholders;
  Esc and backspace-at-0 exit; submitting `git status` produces a normal
  coordinator-audited `bash` tool lifecycle (permission ask under
  `permission: ask`), rendered as a bash tool row; replay mode never enters
  shell mode.
- Tests: composer mode unit tests; deterministic render test for shell-mode
  composer; coordinator-side test that the intent path reuses
  `RequestToolCall` (no new execution path).

**U4. Prompt stash and queued prompts. [P1, adapted]**
- Reference behavior: (a) Stash — commands `prompt.stash` (save current
  prompt+cursor and clear), `prompt.stash.pop`, `prompt.stash.list` (dialog
  with delete via `ctrl+d`); (b) Queueing — submitting while the agent is busy
  queues the prompt; a management dialog lists queued prompts.
- Inspected: `component/prompt/index.tsx` + `component/prompt/stash.tsx`,
  `component/dialog-stash.tsx`; `config/keybind.ts`
  (`session_queued_prompts`).
- Current Harness: neither (greps negative). Backend already supports queued
  turns (`coord/state.rs` `QueuedAgentTurn`).
- Gap: both are core composer behaviors in the reference.
- Target: (a) stash as TUI-local state (session-scoped, persisted beside
  prompt history in `<session-dir>/tui/`, same versioned-JSON pattern as
  `prompt-history.json`), with stash/pop/list actions and a stash dialog in
  the shared select-dialog style; (b) submit-while-busy queues the prompt
  through the existing coordinator turn-queue path (verify the CLI handler
  defers to coordinator queueing rather than rejecting), composer shows a
  `queued N` indicator, and a management dialog lists/removes pending queued
  prompts **before** they are scheduled (removal is only legal pre-schedule;
  once scheduled, the existing cancel path applies).
- Files: `app/prompt_input.rs`, new `app/prompt_stash.rs`, `ui_overlays.rs`
  (stash + queue dialogs), `crates/harness/src/tui/` (queue wiring),
  `crates/harness-core/src/coord/` only if a list/remove-pending surface is
  missing (see §12 queued-turns item — keep coordinator-owned).
- Acceptance: stash → composer clears; pop restores text+cursor; stash list
  shows entries with delete; submitting during a running turn queues (no
  interrupt), indicator shows count, queued prompt runs after the turn ends;
  removing a queued prompt before scheduling prevents its execution and is
  event-auditable if a coordinator command was added.
- Tests: stash unit + dialog render tests; coordinator queue integration test
  for the TUI intent path.

**U5. Permission modal: typed titles, embedded edit diff, staged flow. [P1, adapted]**
- Reference behavior: per-permission header — warning glyph + "Permission
  required", then an icon + specific title (`Edit <path>` with an embedded
  scrollable diff view, `Read <path>`, `Glob "<pattern>"`,
  `Grep "<pattern>"`, `List <dir>`, `<Type> Task`, `WebFetch <url>`,
  external-directory with pattern list, generic `Call tool <name>`); options
  `Allow once / Allow always / Reject`; choosing "Allow always" shows a
  second stage explaining exactly what will be allowed (the pattern list, or
  "until restart" wording for `*`); a reject stage captures the reason; Esc
  rejects; fullscreen layout.
- Inspected: `routes/session/permission.tsx` (stages, info builders, header,
  options, diff embedding).
- Current Harness: modal with Allow once / Allow always (+scope) / Deny,
  two-stage confirm (`PermissionModalStage`), summary text, shortcuts,
  timeout countdown; **no per-tool icon/title forms, no embedded diff**
  (`ui_permission_dock.rs` grep negative).
- Gap: the edit-permission diff preview is the single highest-value approval
  improvement; the typed title forms carry the reference look.
- Target: build the per-permission header from the permission request's
  existing metadata (tool id, summary, request digest selectors, and — for
  `edit` — the proposed hashline operations/diff already available at
  `EditProposed`/permission time; if the diff artifact is written only after
  approval, render the preview from the proposed operations via the existing
  `ui_diff*` pipeline). "Allow always" stage explains the **Harness** grant
  semantics truthfully: run-scoped durable grant with the recorded selectors
  (command digest / workspace-relative path), listed the way the reference
  lists patterns — stronger semantics, reference presentation. Esc maps to
  Deny. Keep the existing countdown.
- Files: `app/permissions.rs`, `ui_overlays.rs`/`ui_permission_dock.rs`,
  `ui_diff*.rs` (reuse), `view_model.rs`;
  `crates/harness-core/src/coord/permission.rs` only if the request payload
  needs one more redacted display field (additive).
- Acceptance: edit permission shows the diff preview scrollable inside the
  modal; read/glob/grep/list/task/webfetch/bash requests show their typed
  icon+title; always-stage lists the exact recorded selectors; Esc denies;
  draft preserved (existing tests); replay renders resolved permissions
  unchanged.
- Tests: deterministic render tests per permission kind (fixture per kind);
  always-stage selector-list test; existing permission tests stay green.

**U6. Session list dialog: pin, delete-confirm, rename, footers. [P1, adapted]**
- Reference behavior: pinned sessions group first ("Pinned" category);
  `ctrl+f` pin/unpin; `ctrl+d` delete with two-press confirm (row title
  becomes "Press ctrl+d again to confirm", spinner during delete, failure
  dialog on error); `ctrl+r` rename opens a rename dialog; rows show title +
  relative updated time footer; quick-switch footer hints.
- Inspected: `component/dialog-session-list.tsx`,
  `dialog-session-rename.tsx`, `config/keybind.ts`.
- Current Harness: `/resume` picker with filtering, resumability reasons,
  titles, times; no pin, no delete, no rename, no quick-switch.
- Gap: list management actions.
- Target: extend the session-history picker: pin/unpin (TUI-local persisted
  state under the session dir's `tui/` folder — pins are presentation, not
  events), pinned group rendered first; rename via T-UI-06's coordinator
  command (rename dialog in the shared input-dialog style); delete with
  two-press confirm — **deletion is destructive**: implement as moving the
  run dir to a `trash/` sibling under the session root via a CLI-side intent
  handler (never from inside `harness-tui`), with the failure dialog on
  error; quick-switch slots deferred (P2, §6.5).
- Files: `app/session_history.rs`, `ui_overlays.rs`,
  `app/lifecycle.rs` (+`UiIntent::DeleteSession`), `crates/harness/src/tui/`
  + `crates/harness/src/sessions.rs` (trash-move helper, reusing session
  path safety and the lineage commands' active/writer-locked source checks),
  docs (`docs/sessions-and-replay.md` trash note).
- Acceptance: pin reorders and persists across restarts; first `ctrl+d` arms
  (title swap), second deletes (dir moved to trash, row disappears), any
  other key disarms; rename round-trips through `SessionTitleUpdated`;
  failure path shows the error dialog; replay-derived data is never mutated
  in place.
- Tests: picker render snapshots (pinned group, armed-delete row); intent
  handler test with a tempdir session corpus; rename integration test.

**U7. Model/variant/agent dialogs: favorites, recents, ranked search. [P1]**
- Reference behavior: fuzzy search over provider-grouped models; `ctrl+f`
  toggles favorite (favorites group/marker); `ctrl+a` jumps to a provider
  list; `f2`/`shift+f2` cycles recently used models *without* opening the
  dialog; variant dialog lists "Default" + named variants with the current
  one marked; `<leader>a` agent list dialog.
- Inspected: `component/dialog-model.tsx`, `dialog-variant.tsx`,
  `dialog-provider.tsx` usage, `dialog-agent.tsx` presence,
  `config/keybind.ts` (`model_*`, `agent_list`).
- Current Harness: provider-grouped search with variants and persisted recent
  selections (`model_switcher.rs`); subsequence filtering; agent cycling via
  `tab`; no favorites, no recent-cycling chord, no provider jump, no agent
  list dialog.
- Gap: favorites + recent-cycle + match ranking + list-dialog forms.
- Target: add favorite flags (TUI-local persisted state alongside recents),
  favorites group sorted first, `ctrl+f` toggle inside the dialog;
  `f2`/`shift+f2` global recent-model cycling emitting the existing
  `SwitchModel` intent with a toast naming the new model; upgrade filter to
  rank by match quality (subsequence scoring is fine; behavior-match, not
  library-match); variant list dialog with "Default" entry; thin agent list
  dialog over existing agent-cycle metadata.
- Files: `app/model_switcher.rs`, `app/model_metadata.rs`, `ui_overlays.rs`,
  `keybindings.rs`.
- Acceptance: favorite toggle persists and reorders; `f2` cycles the recent
  list most-recent-first and routes the next prompt to the selected model;
  variant dialog selects/clears variants; filter ranks prefix/word-boundary
  matches above scattered subsequence matches (table-driven test).
- Tests: extend `model_switcher_metadata_test` + lib tests; dialog snapshots.

**U8. Transcript navigation + display-toggle vocabulary. [P1]**
- Reference behavior: scroll family (page, half-page, line), first/last
  message (`ctrl+g`/`home`, `ctrl+alt+g`/`end`), next/previous message,
  last-user-message; sticky-bottom follow; toggles for timestamps, thinking,
  tool details, generic tool output, scrollbar visibility; copy message
  (`<leader>y`), copy/export session.
- Inspected: `routes/session/index.tsx` command lists + scroll helpers;
  `config/keybind.ts` `messages_*`.
- Current Harness: line scroll, wheel, scrollbar drag, follow mode; toggles
  exist for thinking/timestamps/tool details/generic output; no message
  jumps, no first/last chords, no half-page, no copy-message, no scrollbar
  toggle.
- Gap: navigation vocabulary + per-message copy.
- Target: implement the `messages_*` action family with the reference default
  bindings; message jumps use `MeasuredTranscriptLayout.sections[*].top_row`
  (cache-hit lookups, zero rebuilds — depends on Phase 2); `copy message`
  copies the selected/most-recent message's text via the existing clipboard
  module (OSC52 + fallback in `clipboard.rs`); `copy session` copies a
  plain-text transcript rendering; export maps to the existing
  `sessions export` surface via intent + toast with the output path.
  Conceal toggle is P2 (§6.5). Scrollbar visibility toggle persists in
  TUI-local state.
- Files: `keybindings.rs`, `app/key_interaction.rs`, `ui_transcript.rs`
  (section-top accessor), `app/transcript_state.rs`, `clipboard.rs`,
  `app/lifecycle.rs` (+`UiIntent::ExportSession` if absent).
- Acceptance: each binding performs the reference behavior on a 10-section
  fixture; jumps are O(sections) with zero section rebuilds; follow-mode
  re-engages at bottom jump; copy-message places exact message text on the
  clipboard (injected clipboard seam).
- Tests: unit tests over the measured layout; keybinding tests; render test
  for the scrollbar-hidden state.

**U9. Footer status cluster. [P1]**
- Reference behavior: left cwd; right cluster — `△ N Permission(s)` (warning
  color, only when N>0), `• N LSP` (dot green when >0), `⊙ N MCP`
  (error-colored glyph when any server failed), `/status` hint; home footer
  adds version; (share/connect regions excluded per §6.5).
- Inspected: `routes/session/footer.tsx`; start-screen screenshot.
- Current Harness: footer carries keybinding hints; status banner takes the
  line on errors; MCP/LSP only visible in the wide-layout sidebar
  (`PERSISTENT_SIDEBAR_MIN_WIDTH=121`).
- Gap: no narrow-layout status visibility; banner and hints compete.
- Target: implement the reference footer composition (left cwd, right
  cluster, same glyph language via `StatusGlyphs`); status banner renders on
  its own line above the footer only while present; degradation order at
  narrow widths drops hints → LSP → MCP, never the permission count.
- Files: `ui_chrome.rs`, `view_model.rs`, `layout.rs` (footer plan),
  `theme.rs` (glyph/copy tokens), `ui_secondary.rs` (share data source with
  sidebar).
- Acceptance: at 100×30 with 1 pending permission, 0 LSP, 1 failed MCP, the
  footer matches the reference composition (exact copy pinned in the
  view-model test); at ≥121 cols footer cluster and sidebar coexist; replay
  footer unchanged; 60×20 keeps the permission count.
- Tests: new `footer_status_cluster_renders_without_pty`;
  `ui_chrome_exact_tests.rs` extension.

**U10. Sidebar geometry/brand polish. [P1, mostly shipped]**
- Reference behavior: fixed width 42, panel background, bold title, workspace
  label line, scroll-accelerated content, bottom brand line
  (`• <Brand> <version>`); context block (tokens/percent/cost), MCP list with
  per-server status colors, LSP list, modified files with per-file `+N -M`
  counts (screenshots).
- Inspected: `routes/session/sidebar.tsx`; chat screenshots;
  `opencode-ui-images/session-diff.png`.
- Current Harness: sidebar content parity is **already strong** (Context
  tokens/%/$, MCP, LSP, Modified Files `+/-`, todo, subagents — dogfood +
  screenshots). Gaps: width/visual framing differences and the
  brand+version footer line.
- Target: align geometry (fixed content width matching the reference
  proportions via `layout.rs` constants), add the `• Harness <version>`
  footer line, verify section order/copy against screenshots at 159×40.
- Files: `layout.rs`, `ui_secondary.rs`, `theme.rs`.
- Acceptance: §18.4 comparison passes for the sidebar region;
  `operator_sidebar_preserves_section_order_and_copy` updated deliberately if
  order changes.
- Tests: existing sidebar tests + one reviewed snapshot update.

**U11. Theme parity pass + theme dialog. [P1 default-theme pass; P2 dialog]**
- Reference behavior: `<leader>t` opens a theme-list dialog with live
  preview-on-highlight; many built-in themes; the default dark theme defines
  the visual identity in all screenshots.
- Inspected: `component/dialog-theme-list.tsx` (presence + select-dialog
  pattern), screenshots for the default palette.
- Current Harness: single built-in dark theme close to the reference; no
  selection.
- Target: (a) **P1**: a palette parity pass on the default theme — compare
  token values against the reference screenshots (surface/panel bg, text
  tiers, accent, diff colors, status colors) and adjust `ThemePalette` so the
  §18.4 comparison passes; (b) **P2**: a theme dialog listing 2–4 built-in
  palettes with live preview on highlight and persistence via `tui.json`
  `theme` key (schema + docs updated together). All snapshots stay keyed to
  the default theme.
- Files: `theme.rs`/`theme/`, `ui_overlays.rs`, `configs/tui.json` schema,
  `configs/tui.example.jsonc`, `docs/config.md`.
- Acceptance: default-theme comparison passes; (if dialog ships) selecting a
  theme re-renders live and persists; deterministic tests unaffected.
- Tests: theme-dialog render test; config schema test for `theme` key.

**U12. Timeline framing for lineage/fork + child-session dialog. [P1, adapted]**
- Reference behavior: `<leader>g` timeline dialog lists session messages as a
  timeline; fork-from-timeline picks a message as the fork point; a
  child-session dialog lists subagent sessions; subagent footer shows
  parent/prev/next.
- Inspected: `routes/session/dialog-timeline.tsx`,
  `dialog-fork-from-timeline.tsx`, `dialog-subagent.tsx`,
  `subagent-footer.tsx` (presence + roles).
- Current Harness: `/tree` lineage browser, `/fork` selector with stable
  event cutoffs, subagent footer (`ui_subagent_footer.rs`), child navigation
  keys — functionally equivalent or stronger (stable-prefix validation).
- Gap: framing/binding only — message-anchored fork selection UX vs raw event
  cutoff, `<leader>g` binding, child-session dialog form.
- Target: bind `<leader>g` to the lineage browser; present fork-point
  selection as a message timeline (rows = user/assistant messages projected
  from replay data; chosen row maps to the existing stable event cutoff the
  fork selector already computes); add a child-session select dialog
  reachable from the palette listing children with status. Underlying
  fork/clone semantics unchanged.
- Files: `app/lineage.rs`, `view_model.rs`, `ui_overlays.rs`,
  `keybindings.rs`.
- Acceptance: timeline rows correspond 1:1 to projected messages; selecting a
  row forks at the same cutoff the current selector would choose for that
  boundary (equivalence test); replay mode keeps it read-only.
- Tests: `lineage_view_model_test` extension; fork-cutoff equivalence test.

**U13. Command palette: contextual Suggested. [P1, mostly shipped]**
- Reference behavior: palette prepends a "Suggested" category computed
  per-command (`command.suggested` boolean/function) when the filter is
  empty; rows carry title/description/category/keybinding footer.
- Inspected: `component/command-palette.tsx`.
- Current Harness: palette already renders categories and key hints
  (dogfood); `keybindings/command_registry.rs` centralizes metadata. Gap is
  *contextual* suggestion (e.g. "Resume session" on startup with history;
  "Show last error" after a failure; "Open diff review" after an edit).
- Target: a `suggested(app: &AppState) -> bool` predicate per registry entry,
  evaluated when the palette opens with an empty filter.
- Files: `keybindings/command_registry.rs`, `app/key_interaction.rs`,
  palette snapshot.
- Acceptance: with a failed last turn, "Show last error" appears under
  Suggested; with no condition met, Suggested matches today's static content
  (snapshot-reviewed once).
- Tests: registry unit tests per predicate; one reviewed palette snapshot.

**U14. Error-details overlay with manual resubmit. [P1]**
- Reference behavior: failures are inspectable and re-runnable
  (`dialog-retry-action.tsx`, error component) rather than a one-line status.
- Inspected: component inventory; `Harness chat example 1.png` (Harness shows
  `openai_compatible request failed with status 400` squeezed into the bottom
  line).
- Current Harness: `ProviderErrorCategory` recovery hints exist in
  `harness-providers/src/lib.rs` but the TUI shows only a status banner +
  transcript error block (`ui_tool_error.rs`).
- Target: an `OverlayKind::ErrorDetails` overlay (palette command "Show last
  error", suggested after a failure): category label, redacted message,
  recovery hint, correlated request id; live-mode-only "Resubmit last prompt"
  action via the existing `SubmitPrompt` intent; read-only in replay. **No
  automatic retry from the TUI** — automatic bounded retry is coordinator
  work (§12).
- Files: `overlay.rs`, `ui_overlays.rs`, `app/session_projection.rs`
  (last-error projection), `keybindings/command_registry.rs`.
- Acceptance: after a failed turn the overlay shows category+hint+request id;
  resubmit re-submits through the normal path; replay mode hides the action
  row; overlay sits below the permission modal in precedence.
- Tests: render test with `RunFailed` fixture; precedence test alongside
  `permission_modal_preempts_palette_and_slash`.

**U15. Toasts, spinner, busy-state polish. [verify only]**
- Toasts (info/error), streaming spinner, and startup behavior already exist;
  fold verification into the §18.4 comparison; differences become follow-up
  cards.

### 6.2 Pi backend/session/tooling patterns to adopt or adapt

**B1. Bounded automatic provider retry with operator-visible lifecycle.**
- Reference behavior: pi-mono `core/agent-session.ts` — on transient provider
  error, emit `auto_retry_start {attempt, maxAttempts, delayMs, errorMessage}`,
  sleep `baseDelayMs * 2^(attempt-1)` (abortable), resubmit; reset attempt
  counter on first successful assistant response; emit
  `auto_retry_end {success, attempt, finalError?}`; user input/abort cancels
  the pending retry.
- Local inspiration path inspected: `agent-session.ts` lines 147–148, 283–284,
  535–545, 2491–2525 (greps in §2.2).
- Current Harness behavior: `harness-providers` is single-shot by design;
  coordinator has exactly one retry path — one-shot overflow-compaction retry
  (`runtime.compaction.autoRetryOverflow`). Rate-limit and transport failures
  fail the turn immediately.
- Harness-native target: coordinator-owned bounded retry inside the agent-turn
  phase loop (`coord/agent_turn_phases.rs` / `agent_turn_runtime.rs`): when a
  provider stream fails with category `RateLimited` or `TransportFailure`
  **before any assistant content was committed**, schedule up to
  `runtime.provider_retry.max_retries` (default 2) re-invocations with
  exponential backoff (`base_delay_ms` default 2000, honoring a provider
  Retry-After when the transport surfaces one in redacted metadata), through a
  **fresh provider request id** on the same task. Durability: no new event
  variant — each attempt already records its own
  `ProviderRequestStarted/Finished` pair; add optional redacted
  `metadata.retry = {attempt, max_attempts, delay_ms, category}` to
  `ProviderRequestStartedMetadata`. Cancellation wins: `CancelTask` during
  backoff cancels cleanly via the existing `CancellationToken`; late results
  follow the existing `TaskResultLate` rule. Partial-stream failures (content
  already committed) are **not** retried automatically. TUI shows the
  existing status banner ("retrying (1/2) in 2s · rate_limited") sourced from
  the started-event metadata projection.
- Files: `crates/harness-core/src/coord/agent_turn_phases.rs`,
  `coord/agent_turn_runtime.rs`, `src/event.rs`
  (`ProviderRequestStartedMetadata` additive field), `src/config.rs`
  (`runtime.provider_retry` knobs + schema), `configs/config.json`,
  `docs/config.md`, `docs/architecture.md` (provider lifecycle metadata
  table), `crates/harness-tui/src/app/session_projection.rs` (status label).
- Acceptance: deterministic test with a scripted provider that fails twice
  with `RateLimited` then succeeds: events show three start/finish pairs with
  `retry.attempt` 0→2 metadata, one task, one final assistant message;
  cancel-during-backoff test ends the task as cancelled with no further
  provider events; old logs without the metadata replay unchanged
  (`native_metadata_replay_test` extended); headless `prompt` path unaffected
  when `max_retries=0`.
- Tests/signoff: `coord_test` additions; `event_docs_reference_test` and
  `config_docs_reference_test` for the contract updates.

**B2. Session-list scalability seam (incremental index) — adopt only the
contract, defer the implementation.**
- Reference behavior: pi_agent_rust `session_index.rs` maintains an on-disk
  session index with `refresh_incremental()`, `should_reindex(max_age)`, and
  snapshot-based indexing so `list_sessions` doesn't rescan every session log.
- Current Harness behavior: `proj/session_catalog_projection.rs` derives
  catalog entries from logs; the `perf` lane measures `sessions
  list`/`reopen` against a large corpus and fails closed on stale artifacts.
- Target: **P2 / deferred**. Record the seam decision only: if the perf
  lane's `large-session-surfaces.json` budget regresses, the remedy is a
  replay-derived sidecar index (cache of `SessionCatalogEntry` keyed by run
  dir + log length/mtime) owned by `harness-core::proj`, never a second
  source of truth. No work now beyond a note in
  `docs/sessions-and-replay.md`.
- Acceptance (if/when picked up): index is invalidation-correct under
  appended events, missing index = full rescan, corrupted index = rebuilt,
  `sessions list` output byte-identical with and without index.

**B3. Composer editing depth (kill-ring / word-nav / undo) — corroboration.**
Pi-mono's TUI ships `kill-ring.ts`, `word-navigation.ts`, `undo-stack.ts`;
this independently confirms the value of U2's composer vocabulary. No
additional work beyond U2.

**B4. Compaction quality markers — explicitly NOT adopted now.** Pi's
semantic compaction markers (`compaction.rs` marker kinds/severity/loss-class)
are interesting, but Harness compaction already records structured
summary-source, tail-boundary, and operational-memory metadata with
deterministic fallback. Adding a second quality taxonomy would churn a stable
contract for marginal benefit. Revisit post-V1.

### 6.3 OMO/Codex/Senpi patterns worth adapting

**C1. Codex: streaming-markdown commit discipline (adapt the idea).**
- Reference behavior: codex-rs TUI has dedicated `markdown_stream.rs` +
  `live_wrap.rs`: streamed text is appended to a raw buffer, only *committed*
  lines (ending in newline, outside an open code fence) are markdown-rendered
  into history; the volatile tail renders separately.
- Current Harness behavior: streaming deltas append to
  `ActivityEntry.transcript_text`; the whole text re-renders through
  `ui_markdown.rs` on every rebuild (which §10 makes per-section rather than
  per-transcript).
- Target: within the active streaming section only, split rendering into a
  committed-lines block (markdown-rendered once per committed-line increment,
  cached `Vec<Line>` keyed by committed length) and a tail block re-rendered
  per delta. Implemented entirely inside
  `ui_transcript_render.rs`/`ui_markdown.rs`; no event or projection change.
- Acceptance: a fixture streaming 1,000 lines in 4,000 deltas re-parses each
  committed line at most twice (counter-instrumented test), and the final
  rendered lines are byte-identical to a cold render of the full text.
- Priority: P2 (only after T-PERF-01..04 land and §10.7 measurement still
  shows active-section parse cost dominating).

**C2. Senpi/OMO: prompt/agent discipline features — already covered, do not
re-import.** Intent gate, hashline editing, category routing, todo tools,
skills with progressive disclosure, per-family prompt presets, and compaction
restoration already exist in Harness with tests and docs. §6.4/§6.5 record the
explicit exclusions.

**C3. Codex: snapshot rigor for new UI surfaces (process adoption).** Every
new overlay/footer/dialog/binding behavior in this PRD ships with a
deterministic render snapshot *and* an exact-text assertion where copy is
contractual, matching the existing `*_exact_tests.rs` style. Process rule for
§16 phases, not a code change.

### 6.4 Patterns explicitly not to copy

| Pattern | Source | Why excluded |
|---|---|---|
| Plugin slot system (`TuiPluginRuntime.Slot`) | OpenCode | Arbitrary plugin host is post-V1; conflicts with descriptor-only extension manifest invariant |
| Share/unshare, share URLs, `/connect`, console-org, cloud workspaces | OpenCode | Cloud surfaces are explicitly post-V1 non-goals; footer/sidebar regions substitute Harness equivalents |
| SolidJS reactive store architecture | OpenCode | Source architecture; Harness stays Ratatui immediate-mode with measured-layout caching |
| Message undo/redo (`messages_undo`/`redo` restoring file snapshots) | OpenCode | Depends on OpenCode's workspace snapshot system; Harness has no snapshot store and adding one is a coordinator-scale feature. Fork-from-timeline (U12) is the Harness answer. Revisit post-V1 |
| Image/SVG paste attachments | OpenCode | V1 provider path is text-first; multimodal input is post-V1 in the roadmap |
| External `session.shell` server endpoint semantics | OpenCode | Shell mode is adapted to the coordinator `bash` tool path (U3) — never a TUI-side executor |
| Extension host, JS/WASM hostcalls, swarm/validation-broker systems | pi_agent_rust | Post-V1 by roadmap |
| SQLite session store (`session_sqlite.rs`, `store_v2`) | pi_agent_rust | JSONL event store is the source-of-truth contract |
| `gpt-apply-patch` freeform patch grammar as an edit path | senpi | Competes with hashline editing — explicit non-goal |
| Todo enforcer / idle continuation loop / Ralph loop | OMO/senpi | Explicitly post-V1 in roadmap |
| Bazel build, app-server/IDE protocol layers | codex | Roadmap non-goal; IDE integration post-V1 |
| Auto-retrying *inside the provider transport* | pi-mono `ai` package | Retry must be coordinator-owned so attempts are event-durable and cancellation-safe (§6.2 B1) |

### 6.5 OpenCode UI workstream decision table (binding for the UI workstream)

Classifies the OpenCode TUI surfaces relevant to a local coding harness.
*P1 selected parity target* rows have task cards in this PRD; *adapted* rows
are selected with a recorded semantic difference; *P2 later polish* rows are
defer-able with written disposition; *excluded* rows must not be built. This
table governs the **UI workstream only**.

| OpenCode surface / command family | Classification | Where / why |
|---|---|---|
| Home screen (logo, prompt, placeholders, hints, footer) | P1 (mostly shipped) | U9 footer; placeholder rotation in U3 scope |
| Leader-key scheme + default keymap | P1 | T-UI-10 |
| Input editing vocabulary (`input_*`) | P1 | T-UI-11 |
| Shell mode (`!`) | P1, **adapted** | routes through coordinator `bash` permission path; startup-shell entry rejected with toast (T-UI-13) |
| Prompt stash (+dialog) | P1 | T-UI-12 |
| Queued prompts (+manage dialog) | P1, **adapted** | rides coordinator `QueuedAgentTurn`; removal only pre-schedule (T-UI-12) |
| Command palette (categories, suggested, key footers) | shipped + P1 delta | T-UI-05 contextual suggested |
| Session list: pin/delete/rename/footers | P1, **adapted** | pins are TUI-local state; delete = trash-move via CLI intent; rename = `SessionTitleUpdated` (T-UI-14, T-UI-06) |
| Session quick-switch slots 1–9 | P2 | needs pin/recent state first; card T-UI-15 |
| Model dialog: favorites, recents `f2`, provider jump | P1 | T-UI-16 |
| Variant dialog | P1 | T-UI-16 |
| Agent dialog (`<leader>a`) | P1 | thin select dialog over existing agent metadata (T-UI-16) |
| Theme: default-palette parity pass | P1 | T-UI-09 |
| Theme list dialog + `tui.json` theme key | P2 | T-UI-09 (dialog half) |
| Status dialog | shipped | `/status` exists; verify in §18.4 |
| Help dialog | shipped + binding parity | bind per keymap; verify copy in §18.4 |
| Permission modal (typed titles, diff, stages) | P1, **adapted** | always-stage describes Harness durable run-scoped grants truthfully (T-UI-17) |
| Question dialog | shipped | verify against `routes/session/question.tsx` in §18.4 |
| Transcript nav (`messages_*`) + toggles + copy/export | P1 | T-UI-02 |
| Conceal toggle (`<leader>h`) | P2 | renderer feature; card T-UI-18 |
| Scrollbar toggle | P1 | T-UI-02 |
| Timeline + fork-from-timeline | P1, **adapted** | message rows map to stable event cutoffs (T-UI-19) |
| Subagent dialog + footer | shipped + dialog form | T-UI-19 |
| Sidebar (width, content, brand line) | P1 (content shipped) | T-UI-08a geometry/brand pass |
| Footer status cluster | P1 | T-UI-01 |
| Error details / retry surface | P1, **adapted** | manual resubmit only; auto-retry is coordinator-owned (T-UI-03, T-BE-01) |
| Toast / spinner / startup-loading | shipped | §18.4 verification only |
| External editor for prompt (`<leader>e`) | P2 | suspend/restore TUI + `$EDITOR`; card T-UI-20 |
| Interrupt on Esc | shipped | align copy in §18.4 |
| Session background (`ctrl+b` background sync subagents) | P2 | maps to existing background task path; needs UX design; disposition in Phase 7 |
| Share/unshare, `/connect`, console-org, workspace dialogs | **excluded** | §6.4 cloud exclusion |
| Message undo/redo | **excluded** (this PRD) | §6.4 snapshot dependency |
| Image/SVG paste | **excluded** (this PRD) | §6.4 multimodal post-V1 |
| Debug panel / heap snapshot / console | **excluded** | OpenCode dev tooling; Harness has the event-log review surface |

---

## 7. TUI UX specification

The Harness-side contract per surface. Items marked **[shipped]** were
verified in the audit/dogfood and need preservation plus §18.4 verification
only. For §6.5 P1 rows, the OpenCode reference is the behavioral spec
(§0.6); for everything else this section is the spec.

- **First launch.** **[shipped]** Onboarding (provider pick → auth → skill
  selection → success) appears only when no usable credential/fallback exists;
  skippable; never blocks preconfigured users.
- **Startup shell.** **[shipped skeleton]** Compose-first home: logo, composer
  with placeholder, in-composer agent/model/variant row, hint row, footer.
  Workstream deltas: footer cluster + version (U9), placeholder rotation per
  mode (U3), leader-aware hint copy (U1).
- **Composer-first home.** Typing goes to the composer; slash menu on `/`;
  file mentions on `@` with frecency **[shipped]**; full input-editing
  vocabulary (U2), shell mode (U3), stash (U4), queue indicator (U4).
- **Live transcript.** **[shipped]** Sectioned activities: user surface,
  reasoning block (toggleable), assistant markdown, tool rows with
  disclosure, inline diffs, permission blocks. Deltas: `messages_*`
  navigation + toggles + copy (U8); performance contract per §10.
- **Replay mode.** **[shipped]** Read-only shell; reload; child navigation.
  New surfaces (error overlay, timeline) available read-only;
  rename/pin/delete/queue/shell-mode absent.
- **Session history.** Picker with filtering **[shipped]**; pinned group,
  rename, two-press delete, relative-time footers (U6).
- **Model switcher.** Provider-grouped search with variants and recents
  **[shipped]**; favorites, `f2` recent cycling, ranked filtering, variant
  dialog (U7).
- **Command palette.** **[shipped]** Categories + key footers; contextual
  Suggested (U13); rows for every new action (T-UI-04).
- **Permission modal.** Allow-once / allow-always (durable scoped grants) /
  deny, countdown, draft preservation **[shipped]**; typed icon+titles,
  embedded edit diff, always-stage selector explanation, Esc-rejects (U5).
- **Tool-call rows.** **[shipped]** Keep; snapshot drift reconciled Phase 1.
- **Diffs.** **[shipped]** Inline + side-by-side ≥120 cols with syntect;
  hunk navigation. Verify hunk-row mapping under §10 caching.
- **Operator sidebar.** **[shipped content]** Geometry/brand pass (U10).
- **Terminal panel.** **[shipped]** Unchanged.
- **Subagent/child session navigation.** **[shipped]** Keys + footer; child
  dialog + `<leader>g` timeline framing (U12).
- **Error/status/toast behavior.** Toasts **[shipped]**; error-details
  overlay (U14); banner/footer separation (U9).
- **Small terminal behavior.** Breakpoint tiers **[shipped]**; footer
  degradation order defined in U9; dialogs clamp to the overlay geometry
  contracts in `layout/overlays.rs`.
- **Accessibility/readability.** All new copy through theme text tokens; no
  color-only signaling (glyph + count everywhere); timestamps toggleable.

---

## 8. Ratatui implementation specification

- **Layout model.** Keep `FrameLayoutPlan::for_app` (`layout.rs:109`) as the
  single geometry source. New surfaces (footer cluster, dialogs, error
  overlay) get their rects from the plan, never ad hoc math in render
  helpers. Breakpoint changes only in `ShellBreakpoints`/`layout.rs`
  constants.
- **Render surfaces.** Preserve the `TranscriptRenderSurface` →
  `MeasuredTranscriptSurface` pipeline (`ui_transcript_types.rs`,
  `ui_transcript_layout.rs`). §10 changes *when* surfaces are rebuilt, not
  their shape. The `interaction_rows`/`selection_rows`/`diff_hunk_offsets`
  side-tables stay attached to measured surfaces.
- **Theme tokens.** All new colors/copy from existing token families
  (`StatusColors`, `LiveShellCopyTokens`, etc.). No literal `Color::…` in
  render helpers. The T-UI-09 parity pass adjusts token *values*, not the
  token system.
- **Dialog primitive.** OpenCode builds nearly every picker on one
  `DialogSelect` primitive (title, filter, grouped options with
  title/description/footer/category, footer hints). Harness's pickers are
  hand-rolled per overlay. Before adding the new dialogs (stash, queue,
  rename, theme, agent, variant, child-session), extract a shared
  select-dialog renderer + state helper (filter, grouping, selection,
  footer-hint row) in `ui_overlays/` reused by the new dialogs; migrate
  existing pickers opportunistically, never forcibly (no behavior change to
  shipped pickers without snapshot verdicts).
- **Transcript section model.** `build_transcript_sections(app)` remains the
  section source. Each section gains a stable identity + content revision
  (§10.3); section building for unchanged activities becomes reusable.
- **Tool rendering model.** `ui_transcript_tool_sections.rs` /
  `ui_tool_*.rs` stay the per-tool renderers; pure functions of
  (tool entry, theme, width, disclosure); cache-safe under the section cache.
- **Overlay stack.** `OverlayStack` (`overlay.rs`) becomes the **only** source
  of overlay visibility/precedence (§9.2). Precedence (top→bottom):
  permission/question modal → error details → palette → slash/file-mention →
  dialogs (session/model/variant/agent/theme/stash/queue/toggles/lineage/
  fork/rename) → status dialog.
- **Focus model.** Keep `Focus { List, Details, Terminal, Prompt }`; overlays
  capture input via the stack; `palette_focus_return` generalizes to the
  stack entry.
- **Keybinding model.** `KeyMap` gains leader sequences (U1): dispatch is a
  small state machine (idle → pending-leader(deadline) → resolve); every new
  `Action` gets enum variant + default binding(s) + `metadata_id()` +
  command-registry row + rebind coverage. No scattered `KeyCode::` matches
  outside `KeyMap` dispatch.
- **Mouse/selection model.** Character-cell transcript selection stays;
  snapshot becomes compact (§10.4). Composer gains its own selection state
  (U2) rendered via theme selection style — independent from transcript
  selection.
- **Snapshot/deterministic tests.** New behaviors land with
  `deterministic_render_test` cases (named `*_without_pty`) plus exact-text
  assertions where copy is contractual. PTY lane stays provenance-only.

---

## 9. AppState and TUI state refactor plan

**Goal:** reduce `AppState`'s flat field count and unify overlay state without
changing behavior, observable rendering, or public test APIs. **Not a
rewrite** — each step is a mechanical move with the compiler and existing
tests as the harness. The `Deref/DerefMut → SessionProjection` design stays.

### 9.0 Gating rules (when refactoring is allowed)

- Do **not** broadly refactor `AppState` before the Phase 1 baseline is green.
- Do **not** move transcript-view state (§9.4) before the §10 cache-key
  invalidation and equivalence tests exist — those tests are the guard
  against accidental semantic changes during the move.
- Extract a focused state struct **only when it reduces risk for active
  work**: §9.1 (composer) because U2/U3/U4 build on it; §9.2 (overlays)
  because every new dialog needs single-source visibility; §9.3
  (permission/question) because U5 reworks the modal; §9.4 (transcript view)
  because §10 rekeys its cache fields; §9.5 leaf states opportunistically.
  If a planned UI task is descoped, its enabling extraction is descoped with
  it.
- New dialog/feature state introduced by the UI workstream must land in
  focused modules from day one — never as new flat `AppState` fields.

### 9.1 Step 1 — `ComposerState` (enables U2/U3/U4)
- New module: `app/composer.rs` (or fold into existing `app/prompt_input.rs`).
- Fields moved: `prompt_buffer`, `prompt_cursor`, `prompt_history`,
  `prompt_history_index`, `prompt_history_path`, `prompt_history_draft`.
- Methods moved: cursor/insert/delete/history methods from
  `app/prompt_input.rs` + `app/prompt_history.rs`; `AppState` keeps thin
  delegating accessors used by tests. (U2's selection/undo and U3's mode flag
  land *in this struct* afterwards.)
- Invariants: history persistence path semantics unchanged; draft preserved
  across history navigation and permission modal (existing tests).
- Tests: `cargo test -p harness-tui` (lib), `deterministic_render_test`,
  `session_navigation_keybindings_test`.
- Risks: low; pure move.

### 9.2 Step 2 — overlay unification (`OverlayStack` as truth)
- Existing module: `overlay.rs` + new `app/overlays.rs` holder.
- Fields involved: replace the visibility booleans (`palette_visible`,
  `slash_visible`, `file_mention_visible`, `status_dialog_visible`,
  `session_history_visible`, `model_switcher_visible`,
  `toggles_menu_visible`, `lineage_browser_visible`, `fork_selector_visible`,
  `onboarding_visible`) with stack membership queries, keeping per-overlay
  *content* state in their existing structs/fields.
- Methods: open/close/toggle methods route through stack push/pop;
  `palette_focus_return` and `slash_draft_snapshot` move into `OverlayState`.
- Invariants (all currently tested): permission modal preempts palette/slash;
  `PermissionRequested` ingestion closes palette/slash/mentions
  (`app.rs:582–585`); terminal events close palette and review surface;
  exactly one menu-like overlay visible at a time; replay restrictions.
- Migration: stack-backed accessors that old booleans delegate to → port call
  sites → delete booleans.
- Tests: full `cargo test -p harness-tui`; new
  `overlay_stack_is_single_source_of_visibility`.
- Risks: medium (many call sites); mechanical per-overlay commits.

### 9.3 Step 3 — `PermissionPromptState` and `QuestionPromptState` (enables U5)
- Module: `app/permissions.rs` (exists) gains the structs; fields moved:
  `dismissed_permissions`, `submitted_permission_id`,
  `permission_modal_permission_id`, `permission_modal_stage`,
  `permission_modal_selection`, `permission_modal_confirm_selection`;
  question fields: `question_answer_permission_id`, `question_prompt_tab`,
  `question_prompt_selection`, `question_prompt_answers`,
  `question_prompt_custom`, `question_prompt_editing`,
  `question_answer_buffer`, `question_answer_cursor`,
  `question_answer_error`.
- Invariants: `update_transient_state_for_event` semantics byte-for-byte;
  `replace_events` reset list updated mechanically.
- Tests: permission/question deterministic render + lib tests.
- Risks: low-medium; the reset paths are the places to double-check.

### 9.4 Step 4 — `TranscriptViewState` (after §10 cache tests exist)
- New module: `app/transcript_view.rs`.
- Fields: `transcript_scroll`, `follow_mode`, `last_transcript_max_scroll`,
  `transcript_scrollbar_drag`, `transcript_selection`,
  `transcript_selection_dragging`, `hovered_transcript_target`,
  `hovered_subagent_footer_target`, `transcript_click_activated_on_down`,
  `selected_diff_hunk_row`, `transcript_animation_phase`, `transcript_cache`,
  display toggles (`show_transcript_thinking`, `show_transcript_timestamps`,
  `show_tool_details`, `show_generic_tool_output`,
  `stacked_transcript_diffs`, `expanded_tool_outputs`,
  `expanded_patch_file_outputs`).
- Invariants: cache-key derivation behavior changes belong to §10, not this
  step — this step moves fields only.
- Tests: transcript render/selection/scrollbar tests; `exact_tests.rs`.
- Risks: medium; gated on §10 tests per §9.0.

### 9.5 Step 5 — small leaf states (opportunistic)
- `OperatorSidebarState` → `app/operator_sidebar.rs` (new);
  `TerminalPanelState` → into `app/terminal_panel.rs`;
  `OnboardingState` → into `app/onboarding.rs`;
  `StartupState` (`startup_mode`, `startup_launcher_action`,
  `post_run_handoff_action`, `continued_*` flags) → into `app/lifecycle.rs`.
- Runtime toggles already live in `toggles::RuntimeTogglesState` — no change.
- Tests/risks: per-struct lib tests; low risk.

**Non-goals for §9:** no change to `UiIntent`, `LiveUpdate`, `Focus`, the
`SessionProjection` deref, render function signatures, or any snapshot. If a
step forces a snapshot change, the step is wrong — stop and re-split.

---

## 10. Transcript performance and rendering-cache plan

**This is the first major technical priority after baseline integrity.**

Audited code paths: `app/transcript_cache.rs` (epoch + single stamp→key memo),
`app/transcript_state.rs:44–120` (key derivation),
`ui_transcript.rs:144–146, 621–686` (`TRANSCRIPT_LAYOUT_CACHE`, 4 entries,
keyed app-instance/render-key/theme/width/base-surface),
`ui_transcript_layout.rs:69–126` (`measure_transcript_layout` — full rebuild
of every section's surfaces and wrap measurement),
`ui_transcript_selection.rs` (snapshot cache) + `ui_transcript.rs:480–575`
(selection grid construction), `runtime.rs` (drain/draw cadence),
`app/session_projection.rs` (memory caps; `MemoryCaps.max_events`,
`max_transcript_chars` with trim counters).

### 10.1 Existing caching to PRESERVE
- The thread-local measured-layout cache concept and its multi-width retention
  (`cache.retain` keeps other widths for the same render key — this is what
  makes the scrollbar two-pass measurement cheap). Keep the 4-entry bound.
- The selection-snapshot cache keyed by
  `TranscriptSelectionCacheKey { render_width, app_instance_id, render_key, theme, area, follow_mode, transcript_scroll }`.
- The `cache_key(stamp, build)` memo that avoids re-hashing within a frame.
- Event-seq dedup (`has_seen_seq`) and memory caps with trim-count adjustment.
- The viewport culling in `render_transcript_layout_surfaces`.

### 10.2 Finding A — animation phase and hover are in the measure key
- Current path: `hash_transcript_render_settings`
  (`app/transcript_state.rs:75–88`) hashes `transcript_animation_phase` and
  `hovered_transcript_target` into the *stamp and key* that gate
  `TRANSCRIPT_LAYOUT_CACHE`.
- Problem: every 100 ms spinner tick while streaming (and every hover-target
  change from mouse movement) is a guaranteed layout-cache miss → full
  `build_transcript_sections` + `measure_transcript_layout` + full content
  rehash (Finding B). Idle-with-toast also animates, so even toasts
  re-measure the transcript.
- Proposed change: split keys. The **measure key** covers content + width +
  theme + disclosure + geometry-affecting settings. Animation phase and hover
  move to a **decoration pass**: spinner glyph and hover emphasis are painted
  at render time from `app.transcript_animation_phase()` /
  `hovered_transcript_target()` (positions already known from measured
  surfaces / `interaction_rows`). Where the spinner is part of a `Line`'s
  text today, re-render only the *active section's* surfaces per tick
  (covered by 10.3's section cache), never the whole transcript.
- Acceptance criteria: with a static transcript and an active toast, 10
  consecutive animation ticks cause zero `build_transcript_sections` calls
  (instrument with the existing `build_count_for_test` pattern); hover
  movement across tool rows causes zero re-measures; spinner still animates
  (deterministic render test with phase stepping); selection/scroll behavior
  unchanged.
- Benchmark/manual test: §10.7 S3/S6.

### 10.3 Finding B — monolithic content hash and whole-transcript rebuild per event
- Current path: `bump_transcript_render_epoch()` on **every** ingested event
  (`app.rs:580`), and `compute_transcript_render_cache_key` →
  `hash_transcript_content` hashes every activity's full
  `transcript_text`/`thinking_text`/tool fields (O(total transcript chars))
  whenever the stamp changes; a key change rebuilds **all** sections.
- Problem: per streaming delta (post-drain, per frame), cost is O(entire
  transcript) for hashing + section building + line rendering + wrap
  measurement, instead of O(active section).
- Proposed change (the core of this plan):
  1. **Per-activity revision counters.** `SessionProjection` increments
     `activity.revision: u64` whenever an ingest mutates that activity. The
     global content hash becomes hashing `(request_id, revision)` pairs plus
     list length — O(activities), not O(chars).
  2. **Section-level cache.** Replace the single
     `Vec<TranscriptLayoutCacheEntry>` value with a per-section store:
     key = stable section id (activity `request_id` or synthetic lifecycle
     id) + section revision + width + theme-epoch + base surface +
     section-relevant disclosure bits; value = the built
     `Vec<TranscriptRenderSurface>` *and* its measured
     `MeasuredTranscriptSection`. `measure_transcript_layout` then only calls
     `render_surfaces` for sections whose key missed, and recomputes
     `top_row` prefix sums for all sections (cheap, O(sections)).
  3. Keep the outer whole-layout entry as today (so
     `with_measured_transcript_layout_for_width_on_surface` callers are
     unchanged), assembled from the section store.
- Correctness argument: section content is a pure function of (activity,
  theme, width, disclosure); cross-section interactions are limited to the
  leading gap and within-section surface kinds. Derived tables (diff-hunk
  rows, selection, interaction) come from measured sections and inherit
  correctness — add the hunk-row regression test after partial rebuilds.
- Acceptance criteria: streaming fixture with 500 settled sections + 1
  active: per delta, exactly one section rebuild (counter assertion);
  toggling `show_tool_details` rebuilds all tool sections; expanding one
  tool's output rebuilds only that section; final rendered lines
  byte-identical with the cache disabled (test-only escape hatch used by an
  equivalence test); all existing transcript snapshots unchanged.
- Benchmark/manual test: §10.7 S1/S3/S4.

### 10.4 Finding C — selection snapshot allocates a per-cell String grid
- Current path: `transcript_selection_rows` (`ui_transcript.rs:480–517`)
  builds `total_height × width` individual `String` cells every time the
  selection snapshot cache misses — which, with the current monolithic key,
  is every event/tick while a selection exists;
  `with_transcript_selection_snapshot` also rebuilds the measured layout at
  full width *and* render width.
- Proposed change: keep the snapshot API (`hit`, `selection_text`,
  `visible_rows`) but back it with a compact row representation: one `String`
  per row plus a `Vec<u16>` of cell→byte offsets (grapheme boundaries), and
  `continues_previous`/`copy_offset` as today. Build rows lazily per surface
  from the section cache; snapshot inherits 10.3's keying so settled
  sections' rows are reused.
- Acceptance criteria: selection drag over a 500-message transcript while one
  message streams allocates rows only for the active section per delta
  (counter test); `selection_text` output identical to the current
  implementation for multi-row, rail-prefixed, and wrapped-line selections
  (port existing selection tests as the oracle).
- Benchmark/manual test: §10.7 S6.

### 10.5 Finding D — wrap measurement approximation (document, don't change)
- Current path: `transcript_visual_rows` (`ui_transcript_layout.rs:202`)
  computes `line.width().div_ceil(viewport_width)` while painting uses
  `Paragraph::wrap{trim:false}` — word-wrap can produce more rows than the
  estimate for long words near boundaries; the codebase compensates by mostly
  pre-wrapping.
- Proposed change: none functional. Add a property test that for every
  surface produced by the standard fixtures, painted row count == measured
  row count (render to a test backend buffer and compare). If divergence is
  found, fix the *measurement* (pre-wrap the affected surface), never the
  test.
- Acceptance: property test green across fixture corpus at widths 28/80/159.

### 10.6 Finding E — drain/draw interaction (acceptable, document)
- Current: `runtime.rs` drains ≤16 updates / ≤8 ms per loop, draws once per
  changed batch; with §10.3 the per-frame cost during streaming becomes
  O(deltas in batch applied to one section) + O(sections) prefix sums.
- Acceptable; re-measure after Phase 2 (§10.7 S4) and only then consider
  raising `LIVE_UPDATE_DRAIN_MAX_PER_FRAME` for catch-up replays (historical
  ingest at startup already bypasses the drain budget).

### 10.7 Long-session scenarios (measurement harness)
Create `crates/harness-tui/tests/perf_transcript_test.rs` running under the
`perf` nextest profile (T4, `test(/perf_/)` per `.config/nextest.toml`).
Budgets set from measured baseline ×0.5 (record both numbers as constants
with provenance comments):

| Scenario | Fixture | Assertion |
|---|---|---|
| S1: 500-message conversation, cold render | 500 settled activities | full build+measure under budget; second render with no change performs zero section rebuilds |
| S2: large tool output with artifacts | 1 activity, 5 tool calls, 50 KB summaries + artifact refs | expanding one output rebuilds 1 section |
| S3: streaming assistant output | 500 settled + 1 active, 200 deltas | per-delta rebuilds == 1 section; animation ticks rebuild 0 |
| S4: many small provider deltas | 2,000 deltas in 125 drain batches | end-to-end ingest+render under budget; no O(n²) growth (time(last 100 deltas) < 2× time(first 100)) |
| S5: wide/narrow resize | S1 then width 159→80→159 | resize rebuilds all sections once per new width; returning to a cached width is a hit |
| S6: selection over long transcript | S3 + active selection drag | snapshot rebuild touches only active section per delta; allocation counter under budget |
| S7: scroll while streaming | S3 + PageUp (follow off) | scroll repaints without section rebuilds; follow re-enable jumps to bottom; no offset drift across deltas |

---

## 11. Runtime event loop and responsiveness plan

Audited: `crates/harness-tui/src/runtime.rs`, `src/event.rs`.

- **Live update drain budget.** 16 updates / 8 ms / frame with
  `budget_exhausted → poll(Duration::ZERO)` follow-up is sound and tested.
  Keep; re-measure after §10 (S4).
- **Redraw invalidation.** Single `redraw_requested` boolean + full-frame
  draw via ratatui diffing is the correct Ratatui model; per-widget damage
  tracking is **not** adopted. The expensive part was layout (§10). Ensure
  `handle_mouse` returns `false` when a Moved event changes nothing (audit
  return paths in `app/mouse_interaction.rs`; add an off-surface mouse-move
  no-redraw test) — T-RT-02.
- **Animation cadence.** 100 ms tick, deadline decoupled from redraws,
  slow-frame throttle tested. Keep; §10.2 makes ticks cheap.
- **Mouse movement.** Coalescing in `event.rs` (one-slot stash) is correct.
  Keep.
- **Paste handling.** Bracketed paste enabled with explicit fallback
  teardown; verify multi-line paste into the composer keeps newlines
  (composer-level test if missing) — part of T-RT-02. Large-paste summary
  behavior is P2 — disposition with T-UI-20.
- **Terminal setup/teardown.** Setup pushes keyboard-enhancement flags
  (best-effort), bracketed paste, mouse capture, alternate screen; failure
  path unwinds in reverse; teardown mirrors. Gap: a **panic** mid-loop skips
  teardown. Add a drop-guard that restores the terminal on unwind (plain
  `Drop` impl; workspace denies `unsafe`) — T-RT-01.
- **Preserved terminal session.** Process-global stdout-backed handoff is
  documented inline and intentional. Keep; T-DOC-02 documents the contract.
- **Startup→live→replay transitions.** `TuiMode` cleanly separates modes;
  replay gets no update channel (structurally read-only). Keep. The
  `take_pending_*` mailboxes are a pragmatic seam; document in T-DOC-02.
- **Blocking operations on the UI thread.**
  `event_log::load_events_from_run_dir` runs on the UI thread during reload
  (`runtime.rs:286–301`) and fork/clone intent assembly carries full event
  vectors. Acceptable for V1 sizes; pin with a budgeted perf assertion for
  reload of the perf corpus's largest session; if busted, move loading to the
  CLI side of the intent channel — T-RT-03 (measure first).
- **External editor suspend/resume (P2, T-UI-20).** If implemented:
  leave-alt-screen → disable raw mode → run `$EDITOR` on a temp file → full
  re-setup → repopulate composer. Must reuse the existing setup/teardown
  helpers; never partial-restore.

---

## 12. Backend/coordinator hardening plan

A standalone workstream, not an appendix to the UI work. The coordinator
audit found the documented invariants implemented and tested; items below are
additive hardening, and the coordinator remains the single authority for
every one of them.

- **Agent/session lifecycle.** No changes.
- **Provider lifecycle.** Add bounded transient retry per §6.2 B1 (T-BE-01):
  retries only before assistant content commit, only for
  `RateLimited`/`TransportFailure`, fresh request id per attempt, additive
  `metadata.retry` on `ProviderRequestStartedMetadata`, config knobs under
  `runtime.provider_retry` (`max_retries` default 2, `base_delay_ms` default
  2000, `max_delay_ms` default 30000), deterministic-clock tested.
- **Tool-call lifecycle.** No changes for model-initiated calls. Shell mode
  (T-UI-13) adds an *operator-initiated* `RequestToolCall` path from the TUI
  intent handler — this command already exists; verify the operator actor +
  permission flow and that the call renders as a normal tool row.
- **Permission lifecycle.** No semantic changes. T-UI-17 may need one
  additive redacted display field on the permission request payload (e.g.
  proposed-edit preview reference); keep it optional/serde-defaulted.
- **Cancellation and late results.** No changes; extend coord tests for
  cancel-during-retry-backoff (T-BE-01).
- **Background child wakeups.** No changes.
- **Queued turns.** Verified existing (`coord/state.rs`). T-UI-12 may need
  `Command::ListQueuedTurns`/`RemoveQueuedTurn` if no surface exists —
  coordinator-owned, event-auditable removal (prefer existing mechanisms
  found during implementation).
- **Session resume/replay.** No changes; T-BE-02 adds
  `Command::UpdateSessionTitle { title, respond_to }` → validates non-empty,
  caps length (reuse `clean_generated_title` limits), appends
  `SessionTitleUpdated` via existing event helpers; rejected when no run
  active. Replay projections already consume the event. This also makes the
  roadmap's "editable titles" claim true.
- **Session deletion (supporting T-UI-14).** Never a coordinator/runtime
  feature: implemented as a CLI-side trash-move on sessions that are not
  active and not writer-locked (reuse the lineage commands' source checks in
  `crates/harness/src/sessions.rs`).
- **Compaction.** No changes.
- **Redaction.** No changes; new retry metadata and permission display fields
  go through the existing redactor and the secret-scan fixture corpus.
- **Failure recovery.** Crash-tail recovery verified; no changes. T-BE-05
  improves the mock-provider miss error (name the fixture lookup root,
  suggest `run --scenario golden_path --deterministic`) — observed in
  dogfooding as `mock fixture missing for request_digest=…` with no guidance.

**Auditability note:** retry attempts are fully reconstructable from the event
log (multiple start/finish pairs per turn already legal); no replay semantic
change — old logs without `metadata.retry` replay identically.

---

## 13. Provider/model hardening plan

- **Provider trait.** Keep single-method `stream_completion`; no retry inside
  transports (decision recorded in §6.4).
- **OpenAI-compatible transport.** Keep. Additive: surface a redacted
  optional `retry_after_ms` hint on the stream `Error` event metadata when
  the transport observes one (never the raw header map). Used by T-BE-01's
  backoff. Files: `openai/error.rs`, `openai.rs`, `lib.rs`; cassette schema
  unaffected (serde defaults).
- **Streaming event normalization.** Verified complete. No changes.
- **Model catalog / variants.** Verified. No changes; T-UI-16's
  favorites/recents are TUI-local state, not catalog changes.
- **Auth seams.** Verified. No changes.
- **Prompt/cache behavior.** Verified. No changes.
- **Error mapping.** Verified 8-category taxonomy with recovery hints; reused
  by the U14 error overlay — expose `recovery_hint`/`from_str` publicly if
  not already (T-BE-04). Files: `harness-providers/src/lib.rs`,
  `harness-tui/src/app/session_projection.rs`.
- **Mock/cassette coverage.** Add mock-provider scripts for the retry tests
  (fail-N-times-then-succeed by digest sequence) without changing cassette
  semantics. T-BE-05 improves miss diagnostics.

---

## 14. Native tool hardening plan

The tool surface is in strong shape. Remaining items are small:

- **Tool schemas / catalog metadata.** No changes; no new tools. Shell mode
  reuses the existing `bash` tool.
- **Workspace safety.** No changes.
- **Permission/capability mapping.** No changes; canonical names stay
  `bash, edit, question, task, webfetch, websearch, codesearch, lsp`.
- **Hashline edit flow.** No changes; the permission modal's diff preview
  (T-UI-17) consumes proposed-edit data read-only.
- **Bash restrictions.** No changes; shell-mode submissions inherit the
  allowlist, blocked-command hints, timeout, and output caps unchanged.
- **AST-grep/LSP/MCP.** No changes.
- **Session inspection tools.** No changes.
- **Large output artifacts.** No changes; §10.7 S2 exercises the TUI side.
- **TUI rendering of tool outcomes.** Covered by Phase 1 snapshot
  reconciliation and §10 caching. Verify failed-tool subtitle visibility
  without expansion (`ui_tool_error.rs`) — T-UI-07.

---

## 15. Documentation and roadmap alignment

**Docs to preserve as-is:** `docs/architecture.md` invariants and event
contract; `docs/sessions-and-replay.md`; `docs/native-tool-catalog.md`;
`docs/permissions.md`; `docs/testing.md` owner map; crate `AGENTS.md` files.

**Docs to update together with implementation:**

| Change | Update |
|---|---|
| `runtime.provider_retry` knobs (T-BE-01) | `docs/config.md`, `configs/config.json` schema, `config_docs_reference_test`, `config_schema_cli_test` |
| `ProviderRequestStartedMetadata.retry`, `Error.retry_after_ms` | `docs/architecture.md` provider-lifecycle metadata table, `event_docs_reference_test` |
| `Command::UpdateSessionTitle` / `/rename` (T-BE-02/T-UI-06) | `docs/sessions-and-replay.md` resume/title note, README slash-command list |
| Leader key + new keybindings/actions (T-UI-10, T-UI-02, T-UI-11) | `configs/tui.example.jsonc`, `configs/tui.json` schema, `docs/config.md` TUI section (default-binding table, drift-tested) |
| `tui.json` `theme` key (T-UI-09, if dialog half ships) | `configs/tui.json` schema, `configs/tui.example.jsonc`, `docs/config.md` |
| Session trash-move deletion (T-UI-14) | `docs/sessions-and-replay.md` |
| Shell mode (T-UI-13) | README TUI section, `docs/native-tool-catalog.md` note that `bash` is also operator-invokable via `!` |
| Footer cluster + error overlay + permission modal flows | `docs/tui-signoff-manifest.v1.json` only if a required flow's owner test changes; otherwise new deterministic owner tests are additive |
| Mock-miss error text (T-BE-05) | README troubleshooting bullet |

**Stale or overstated claims found (correct during Phase 1):**

1. **Clean-tree test failures.** `deterministic_render_test` fails 2/9 on
   `dev` HEAD (§2.3). Roadmap claims `cargo test --workspace --all-features`
   passes; until reconciled, that claim is stale.
2. **"Editable titles."** Roadmap claims "generated **or editable** titles" —
   no edit surface exists. Land T-UI-06/T-BE-02 or reword per the
   claim-correction PRD's rules.
3. **`docs/architecture.md` team-events fragment.** Lines ~240–248 are
   orphaned sentence fragments — restore or remove (docs-only fix).
4. **`docs/roadmap-v1.md` truncated bullets.** Lines ~81 and ~99 are
   mid-sentence artifacts; repair wording without changing checked status.
5. **Stale parity screenshot.** `inspirations/screenshots opencode ui
   parity/Harness project/Harness current start screen.png` predates the
   current startup shell. `inspirations/` is read-only — noted here; fresh
   Harness-side captures are produced by the §18.4 review instead.

**Roadmap classification:** Phase 1 is V1-hardening (claim integrity). §10/§11
are V1 performance hardening backed by the existing "long-session performance
claims need measurement" roadmap line. The §6.1 UI workstream executes the
roadmap's "TUI polish inspired by source-reference terminal tools" line,
scoped to terminal-first V1 surfaces. §12 retry is reliability hardening.
Nothing here re-scopes the explicit post-V1 list (cloud/share, plugins,
multimodal, IDE).

---

## 16. Implementation roadmap

Order rationale: a green deterministic baseline gates everything; transcript
performance comes next because it is the top technical priority and because
new UI surfaces must not inherit the broken invalidation model; the minimal
state refactors come third because the §10 tests guard the riskiest move and
the UI workstream builds on the extracted states; then the two UI phases;
then backend hardening (largely parallelizable from Phase 2 onward); then
regression/evidence.

### Phase 1 — Baseline integrity
- Goal: clean-tree green deterministic lanes; honest docs.
- Tasks: T-TEST-01 (snapshot drift verdicts + reconciliation), T-DOC-01
  (doc fragments + stale claims).
- Acceptance: `scripts/test-lanes.sh fast` and
  `cargo test -p harness-tui --test deterministic_render_test` pass;
  behavior-vs-fixture verdicts recorded.
- Do not change: render code (unless the verdict is "behavior regressed").

### Phase 2 — Transcript performance foundation
- Goal: cache-key invalidation tests, cache-on/off equivalence test, wrap
  property test, perf harness, then the fixes: animation/hover decoration
  split (§10.2), per-activity revisions + section-level cache (§10.3),
  compact selection snapshot (§10.4).
- Tasks: T-PERF-01..05.
- Dependencies: Phase 1.
- Acceptance: §10.2/10.3/10.4 criteria + §10.7 table green under the `perf`
  profile; equivalence test green; existing snapshots unchanged.
- Risks: subtle measurement drift — mitigated by landing the equivalence and
  property tests *first*.

### Phase 3 — Minimal TUI state maintainability refactors
- Goal: only the extractions that reduce risk for active work (§9.0):
  `ComposerState` (§9.1, enables Phase 4 composer work), overlay-stack
  unification (§9.2, enables Phase 5 dialogs), permission/question state
  (§9.3, enables Phase 5 modal work), `TranscriptViewState` (§9.4 — now
  legal because Phase 2's tests exist), leaf states opportunistically
  (§9.5).
- Tasks: T-REF-01..05.
- Dependencies: Phase 2 (for §9.4); Phase 1 for everything.
- Acceptance: zero snapshot changes; new
  `overlay_stack_is_single_source_of_visibility`; `AppState` direct field
  count reduced ≥40.
- Risks: reset-path omissions (`replace_events`) — per-struct `reset()`
  methods called from one place.

### Phase 4 — OpenCode UI workstream: interaction vocabulary
- Goal: leader-key `KeyMap` + OpenCode-like default keymap (T-UI-10),
  composer input-editing vocabulary (T-UI-11), shell mode (T-UI-13),
  stash + queued prompts (T-UI-12), contextual Suggested palette (T-UI-05),
  palette/help metadata coverage (T-UI-04), footer status cluster (T-UI-01),
  sidebar geometry/brand pass (T-UI-08a), transcript
  navigation/toggles/copy/export (T-UI-02).
- Dependencies: Phases 2–3.
- Acceptance: §6.1 U1–U4, U8–U10, U13 criteria; leader hint state; rebind
  coverage; `docs/config.md` default-binding table drift-tested;
  small-terminal degradation test at 60×20.
- Risks: input-handling regressions — the existing
  draft/mention/permission-preemption tests are the safety net; extend before
  changing dispatch.

### Phase 5 — Dialog and permission polish
- Goal: shared select-dialog primitive (§8), session-list pin/two-press
  delete/rename (T-UI-14 + T-UI-06 frontend), model favorites/recents +
  variant/agent dialogs (T-UI-16), theme default-palette parity pass
  (T-UI-09 P1 half), permission modal typed titles + embedded diff + staged
  flow (T-UI-17), timeline framing + child-session dialog (T-UI-19),
  error-details overlay (T-UI-03, needs T-BE-04), failed-tool subtitle and
  compaction-display verifications (T-UI-07, T-UI-08).
- Dependencies: Phase 4 (leader bindings, dialog conventions); T-BE-02/
  T-BE-04 from Phase 6 may land earlier or concurrently.
- Acceptance: §6.1 U5–U7, U11(P1), U12, U14 criteria; replay-mode absence
  tests.
- Risks: snapshot churn — batch copy/theme changes into single reviewed
  commits.

### Phase 6 — Backend reliability hardening
- Goal: coordinator bounded provider retry (T-BE-01), `UpdateSessionTitle`
  (T-BE-02), `retry_after_ms` surfacing (T-BE-03), error-hint exposure
  (T-BE-04), queued-turn list/remove surface if required (with T-UI-12),
  mock-miss diagnostics (T-BE-05).
- Dependencies: Phase 1 only — **this phase is parallelizable with Phases
  2–5** if staffing allows; it is sequenced here only to keep review focus.
- Acceptance: §12/§13 criteria; `coord_test`, drift tests,
  `native_metadata_replay_test`, secret-scan green.
- Risks: retry/cancellation races — fake clock; never sleep in deterministic
  tests (quality-gates enforces this).

### Phase 7 — Full regression, dogfooding, docs, evidence
- Goal: run `scripts/test-lanes.sh all-deterministic`, `perf`,
  `signoff-binary`, `signoff-pty`; produce the §18.4 OpenCode comparison for
  the selected parity-now surfaces; update every §15 doc row; record §18.7
  dogfooding evidence; disposition all P2 cards and every *P2 later polish*
  row in §6.5.
- Acceptance: §18 in full.
- Do not change: anything new besides docs and evidence.

---

## 17. Detailed task backlog

> Priority: P0 = blocks everything / correctness of claims; P1 = the core
> value of this PRD; P2 = valuable, defer-able with written disposition.
> Area tags: baseline / perf / refactor / **UI workstream** / backend / docs /
> testing.

**T-TEST-01 · Reconcile failing deterministic render snapshots · P0 · baseline**
- Description: `command_palette_renders_without_pty` and
  `tool_lifecycle_rows_stay_ordered_without_pty` fail on clean `dev`; the
  palette diff shows the live composer placeholder present in the new render
  but absent from the committed snapshot.
- Files: the two `.snap` files; possibly fixtures in
  `tests/deterministic_render_test.rs`.
- Notes: `git log -p` the snapshots and fixture builders around commits
  `2cbfe31d`/`2555315c`/`9e5e7fb7`; write the behavior-vs-fixture verdict in
  the commit message; only then `cargo insta review`.
- Acceptance: tests green; verdict recorded; no assertion weakened.
- Validation: `cargo test -p harness-tui --test deterministic_render_test`;
  `scripts/test-lanes.sh fast`.

**T-DOC-01 · Repair doc fragments and stale claims · P0 · docs** — per §15
items 1–4.

**T-PERF-01 · Split measure key from decoration (animation/hover) · P0 · perf** — §10.2.
**T-PERF-02 · Per-activity revisions + section-level layout cache · P0 · perf** — §10.3.
**T-PERF-03 · Compact selection snapshot rows · P1 · perf** — §10.4.
**T-PERF-04 · Wrap-measurement property test · P1 · perf/testing** — §10.5
(lands before T-PERF-02).
**T-PERF-05 · Long-session perf harness (S1–S7) · P1 · perf/testing** — §10.7.

**T-REF-01 · Extract ComposerState · P1 · refactor** — §9.1 (enables T-UI-11/12/13).
**T-REF-02 · Overlay stack as single visibility source · P1 · refactor** — §9.2.
**T-REF-03 · Extract Permission/Question prompt state · P1 · refactor** — §9.3 (enables T-UI-17).
**T-REF-04 · Extract leaf states · P2 · refactor** — §9.5 (opportunistic).
**T-REF-05 · Extract TranscriptViewState · P1 · refactor** — §9.4 (gated on Phase 2 tests).
- (Shared) Acceptance: zero snapshot diffs; full `cargo test -p harness-tui`
  green per step.

**T-RT-01 · Terminal restore on panic (drop guard) · P1 · UI workstream/runtime** — §11.
**T-RT-02 · Mouse-move no-op and paste regression coverage · P2 · testing** — §11.
**T-RT-03 · Reload/fork event-load budget · P2 · perf** — §11 (measure first).

**T-UI-10 · Leader-key scheme + OpenCode-like default keymap · P1 · UI workstream**
- Per §6.1 U1. Files: `keybindings.rs`, `keybindings/`,
  `app/key_interaction.rs`, `configs/tui.example.jsonc`, `configs/tui.json`
  schema, `docs/config.md`.
- Acceptance: §6.1 U1 criteria; default-binding table in docs drift-tested
  against the registry.
- Non-goals: vim-style multi-count sequences; only leader+key two-step.

**T-UI-11 · Composer input-editing vocabulary · P1 · UI workstream**
- Per §6.1 U2. Files: `app/prompt_input.rs` (+ sibling
  `app/prompt_selection.rs`), `keybindings.rs`, `ui_composer.rs`.
- Acceptance: §6.1 U2 criteria incl. mention-tag offset integrity.

**T-UI-12 · Prompt stash + queued prompts · P1 · UI workstream (+backend verify)**
- Per §6.1 U4. Files: `app/prompt_stash.rs` (new), `app/prompt_input.rs`,
  `ui_overlays.rs`, `crates/harness/src/tui/`,
  `crates/harness-core/src/coord/` only if list/remove-pending is missing.
- Acceptance: §6.1 U4 criteria; queue indicator; coordinator-owned removal.

**T-UI-13 · Shell mode (`!`) via coordinator bash path · P1 · UI workstream (+backend verify)**
- Per §6.1 U3. Files: `app/prompt_input.rs`, `ui_composer.rs`,
  `app/lifecycle.rs`, `crates/harness/src/tui/`, `theme.rs`.
- Acceptance: §6.1 U3 criteria; permission flow exercised under `ask`;
  replay-mode and startup-shell guards.
- Non-goals: TUI-side execution; PTY-interactive shells.

**T-UI-14 · Session list pin / two-press delete / rename actions · P1 · UI workstream**
- Per §6.1 U6. Files: `app/session_history.rs`, `ui_overlays.rs`,
  `app/lifecycle.rs`, `crates/harness/src/tui/`,
  `crates/harness/src/sessions.rs` (trash-move), docs.
- Acceptance: §6.1 U6 criteria; trash-move safety (active/locked sources
  rejected); failure dialog.

**T-UI-15 · Session quick-switch slots · P2 · UI workstream** — `<leader>1..9`
jump to pinned/recent sessions; depends on T-UI-14 pins. Disposition in
Phase 7.

**T-UI-16 · Model favorites/recents + variant/agent dialogs · P1 · UI workstream**
- Per §6.1 U7. Files: `app/model_switcher.rs`, `app/model_metadata.rs`,
  `ui_overlays.rs`, `keybindings.rs`.
- Acceptance: §6.1 U7 criteria; `f2`/`shift+f2` global cycle with toast.

**T-UI-17 · Permission modal: typed titles, embedded diff, staged flow · P1 · UI workstream**
- Per §6.1 U5. Files: `app/permissions.rs`, `ui_permission_dock.rs`,
  `ui_overlays.rs`, `ui_diff*.rs` (reuse), `view_model.rs`;
  `harness-core/src/coord/permission.rs` (additive display field only if
  needed).
- Acceptance: §6.1 U5 criteria; per-kind render fixtures; Esc=deny; existing
  permission tests green.

**T-UI-18 · Conceal toggle (code-block concealment) · P2 · UI workstream** —
§6.5; disposition in Phase 7.

**T-UI-19 · Timeline framing + child-session dialog · P1 · UI workstream**
- Per §6.1 U12. Files: `app/lineage.rs`, `view_model.rs`, `ui_overlays.rs`,
  `keybindings.rs`.
- Acceptance: §6.1 U12 criteria incl. fork-cutoff equivalence test.

**T-UI-20 · External editor for prompt + large-paste summary · P2 · UI workstream** —
§6.5/§11; suspend/restore discipline per §11; disposition in Phase 7.

**T-UI-01 · Footer status cluster · P1 · UI workstream** — §6.1 U9.
**T-UI-02 · Transcript navigation/toggles/copy/export vocabulary · P1 · UI workstream** — §6.1 U8.
**T-UI-03 · Error-details overlay with manual resubmit · P1 · UI workstream** — §6.1 U14 (needs T-BE-04).
**T-UI-04 · Palette/help coverage for all actions · P1 · UI workstream**
- Give `SessionChildFirst/Cycle/CycleReverse/Parent`,
  `ToggleOperatorSidebar`, and every new action `metadata_id()`s + registry
  rows; document intentional chrome-only exceptions.
**T-UI-05 · Contextual Suggested palette predicates · P1 · UI workstream** — §6.1 U13.
**T-UI-06 · `/rename` + rename dialog wiring · P1 · UI workstream** — §6.1 U6 (needs T-BE-02).
**T-UI-07 · Failed-tool subtitle verification · P2 · testing** — §14.
**T-UI-08 · Compaction active-vs-cumulative display verification · P2 · testing** — cite or add the test.
**T-UI-08a · Sidebar geometry/brand parity pass · P1 · UI workstream** — §6.1 U10.
**T-UI-09 · Theme default-palette parity pass (P1) + theme dialog/`tui.json` key (P2) · UI workstream** — §6.1 U11.

**T-BE-01 · Coordinator bounded provider retry · P1 · backend** — §12/§6.2 B1.
**T-BE-02 · `Command::UpdateSessionTitle` · P1 · backend** — §12.
**T-BE-03 · Surface `retry_after_ms` in provider error metadata · P2 · backend** — §13.
**T-BE-04 · Expose error category recovery hints to TUI · P1 · backend** — §13.
**T-BE-05 · Actionable mock-fixture-miss error · P2 · backend/docs** — §12.

**T-DOC-02 · Document TUI runtime seams · P2 · docs** — `LiveUpdate` channel,
preserved-terminal handoff ownership, pending-launch mailboxes.

---

## 18. Acceptance criteria for final completion

1. **Baseline and tests.** `scripts/test-lanes.sh all-deterministic` and
   `perf` green; clippy `-D warnings` green; the Phase-1 snapshot verdicts
   recorded; no test weakened; no snapshot accepted without a verdict;
   quality-gates green; keybinding-docs drift test green.
2. **Transcript performance.** §10.7 S1–S7 budgets pass under the `perf`
   profile; per-delta work touches exactly one section in a
   500-message+streaming transcript; animation/hover cause zero re-measures;
   resize re-measures once per width; cache-on/off equivalence holds.
3. **Maintainability.** `AppState` direct fields reduced ≥40; overlay
   visibility has a single source of truth
   (`overlay_stack_is_single_source_of_visibility` green); new dialogs share
   the select-dialog primitive; no widened `app.rs`/`ui.rs`/large
   `ui_transcript_*`; all new feature state lives in focused modules.
4. **OpenCode UI workstream (scoped comparison).** All §6.5 *P1 selected
   parity target* and *adapted* rows are implemented per their §6.1
   acceptance criteria. The selected parity-now local-coding surfaces are
   compared against the OpenCode references (source + screenshots) at
   matching geometry; **differences are allowed if documented in §6.5 and
   justified by Harness architecture, scope, or safety**; excluded OpenCode
   features remain excluded. Comparison artifacts (PTY or native captures +
   a verdict table) are stored with the lane evidence; native screenshots
   follow `signoff-native` provenance rules, with PTY captures acceptable
   where native is unavailable. Cloning every OpenCode surface is **not**
   required.
5. **Backend behavior.** Transient provider failures retry within configured
   bounds with durable, redacted attempt metadata; cancellation beats
   backoff; replay of old and new logs is unchanged in semantics; rename
   appends exactly one event through the coordinator; shell-mode commands are
   fully event-audited bash tool calls; queued-prompt removal is
   coordinator-owned.
6. **Docs/config/schemas.** Every §15 row updated together with its code;
   roadmap/architecture doc fragments repaired; claim-evidence ledger updated
   for any roadmap box touched.
7. **Dogfooding evidence.** A recorded session (PTY captures + command
   transcript stored with lane artifacts) covering: startup → prompt →
   queued second prompt → tool call with permission ask (edit form with
   diff) → diff review → shell-mode command → induced provider failure →
   retry observation → error overlay → `/rename` → pin → session-list delete
   (on a scratch session) → model favorite + `f2` cycle → timeline fork →
   resume → replay → quit, at both 159×40 and 100×30.

---

## 19. Testing strategy

- **Unit tests** next to behavior per current convention: cache keys,
  revision counters, jump-target math, footer view model, leader-sequence
  dispatch, composer selection/undo, stash state, retry backoff arithmetic
  (pure function, fake clock).
- **Integration tests**: coordinator retry lifecycle in
  `crates/harness-core/tests/coord/` (scripted provider, fake clock,
  cancellation-during-backoff); `UpdateSessionTitle` append + projection;
  queued-turn submit-while-busy through the TUI intent path; shell-mode
  intent → `RequestToolCall` permission flow; trash-move deletion safety;
  `native_metadata_replay_test` extension for old-log compatibility.
- **Deterministic render tests**: one `*_without_pty` case per new visible
  surface (footer cluster, error overlay, suggested palette, shell-mode
  composer, stash/queue dialogs, rename dialog, armed-delete session row,
  permission modal per kind, timeline), plus small-terminal degradation at
  60×20.
- **Snapshot/golden tests**: insta snapshots for new surfaces; every change
  carries a behavior-vs-fixture verdict; orphan-snapshot static gate stays
  green.
- **PTY E2E**: provenance smoke only; re-capture `signoff-pty` artifacts in
  Phase 7; behavioral assertions stay in deterministic owners.
- **UI comparison evidence**: §18.4 captures for selected parity-now surfaces
  stored with lane artifacts; native captures only through `signoff-native`
  provenance rules.
- **Live/manual signoff**: provider-retry against a real provider is
  env-gated manual only; never required for deterministic completion.
- **Long-session performance tests**: `perf_transcript_test.rs` (T4 profile)
  per §10.7, plus the existing `perf` lane artifact freshness checks.
- **Regression tests for known rough edges**: wrap-measure property test;
  diff-hunk rows after partial section rebuilds; selection text equality
  oracle; off-surface mouse-move no-redraw; multi-line paste; panic-unwind
  terminal restore; mention-tag offsets under word-delete.

---

## 20. Risks and mitigations

| Risk | Mitigation |
|---|---|
| UI workstream crowding out performance/backend work | Phase order fixes priority (perf is Phase 2; backend is parallelizable from Phase 2); §0.5 requires all workstreams' P1s, not just UI |
| Over-copying inspirations (porting SolidJS structure, cloud surfaces, plugin slots) | §0.6 defines parity as observable behavior for selected surfaces; §6.4/§6.5 exclusion lists are normative |
| Parity drift on selected surfaces ("similar but not the same") | §0.6 verification + §18.4 scoped comparison with per-difference verdicts; reference re-read rule (§0.2.8) |
| Rewriting too much during §9 extractions | §9.0 gating rules; one mechanical move per commit; zero-snapshot-diff rule; stop on any forced snapshot change |
| Moving coordinator authority into TUI (retry, rename, shell, delete, queue) | Retry in `coord/agent_turn_*`; rename/queue are coordinator commands; shell mode reuses `RequestToolCall`; deletion is a CLI-side trash-move; TUI only emits intents — acceptance tests assert event provenance |
| Breaking replay safety | No new event variants without §12 justification; additive serde-defaulted metadata only; replay guards extended; mutation surfaces absent in replay mode |
| Making UI state more tangled (half-migrated overlays, new dialog state on flat AppState) | §9.2 single-source test; §9.0 rule that new feature state lands in focused modules; shared dialog primitive |
| Weakening cache correctness | Cache-on/off equivalence test; per-key-component invalidation tests; wrap property test lands before the cache change |
| Snapshot churn from UI/theme changes | Phase 1 verdict discipline; batch copy/theme changes; `cargo insta review` only |
| Async race conditions in retry/cancellation/queue | Fake clock; no sleeps in deterministic tests (quality-gate enforced); cancellation token is the only abort path; late results stay `TaskResultLate` |
| Keybinding regressions from the leader-key migration | Old bindings retained as additional bindings where non-conflicting; dispatch state machine unit-tested; rebind coverage in `keybindings/tests.rs` |
| Performance regressions elsewhere (section-cache memory, new dialogs) | 4-entry outer bound kept; section store bounded by capped activity count; S1–S7 budgets fail closed |
| Documentation drift | §15 update-together table mirrored into task checklists; docs-reference drift tests in `fast`; new keybinding-docs drift test |

---

## 21. Instructions for the future implementation agent

1. **Work incrementally.** One phase at a time (§16); one task card per
   PR-sized change; record evidence (command + result) per card before
   marking it done.
2. **Respect the balance.** This PRD has five workstreams. Do not let the UI
   workstream absorb the others: performance budgets (§10.7) and backend
   hardening (§12–§13) are P0/P1 in their own right, and Phase 6 may run in
   parallel from Phase 2 onward.
3. **Preserve invariants.** Re-read §0.3 before each phase. If a change seems
   to need a new event variant, a TUI-side permission decision, TUI-side
   execution, or replay-time mutation — stop; the design is wrong.
4. **Read the relevant `AGENTS.md` files before edits** — root first, then
   the crate you touch.
5. **Load required coding skills before coding.** `karpathy-guidelines` is
   mandatory for any code edit in this repo; include it in `load_skills` for
   delegated coding tasks.
6. **For OpenCode UI workstream tasks, the reference is the behavioral spec
   for that surface.** Before implementing any §6.1 item, re-open the cited
   files under `inspirations/opencode/...` and the parity screenshots,
   enumerate the observable behaviors, and write the Harness test list
   first. Implement from the behavior list — never by translating the
   TypeScript. Where the reference conflicts with a §0.3 invariant or §6.5
   adaptation, Harness wins and the difference is recorded in §6.5.
7. **Keep changes small and reviewable.** If a diff mixes a refactor with a
   behavior change, split it.
8. **Add tests before risky refactors.** The §10 cache-equivalence and
   property tests land *before* the cache rework; §9.4 lands after them;
   keybinding dispatch tests land before the leader migration.
9. **Do not weaken tests.** No `#[ignore]`, no assertion loosening, no
   baseline edits to dodge gates, no snapshot acceptance without a written
   behavior-vs-fixture verdict.
10. **Keep the implementation Rust-native and Ratatui-native.** Layout math
    in `layout.rs`/`theme.rs`; colors through theme tokens; keys through
    `KeyMap` + the command registry; geometry through `FrameLayoutPlan`;
    dialogs through the shared select-dialog primitive.
11. **Update docs/schemas/tests together** when public contracts change —
    §15's table is the checklist; run `fast` before declaring a card done.
12. **Record evidence after each phase**: commands run, results, artifact
    roots, snapshot verdicts, and — for the UI workstream — the §18.4
    capture pairs and verdict rows, in commit messages and a phase-closeout
    ledger following the `docs/pre-v1-enhancements-progress.md` format.
13. **Stop only when acceptance criteria are actually met** (§0.5, §18).
    "Mostly works", "close enough", and "will document later" are not done.
    If a card is genuinely blocked, write the blocker into the ledger and
    move to an independent card rather than halting or hacking around it.
